//! Appels HTTP vers l'API REST mod.io.

use std::time::Duration;

use reqwest::header::{HeaderMap, ACCEPT};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;

use super::models::ApiErrorBody;
use crate::error::{AppError, AppResult};

/// Au-delà de ce délai, on ne patiente pas en silence : on laisse l'interface
/// afficher un compte à rebours.
const MAX_SILENT_RETRY_SECS: u64 = 5;
const DEFAULT_RETRY_SECS: u64 = 60;

/// Clé + chemin d'API utilisés pour une requête (copiés depuis les réglages).
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub api_key: String,
    pub api_path: String,
}

pub struct ModioClient {
    http: Client,
}

impl ModioClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent(concat!("Atelier/", env!("CARGO_PKG_VERSION"), " (client mod.io non officiel)"))
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("client HTTP");
        Self { http }
    }

    /// GET `{api_path}{path}?api_key=…&query…`, avec un nouvel essai automatique
    /// si mod.io demande d'attendre quelques secondes.
    pub async fn get<T: DeserializeOwned>(
        &self,
        cfg: &ApiConfig,
        path: &str,
        query: &[(&str, String)],
    ) -> AppResult<T> {
        match self.get_once(cfg, path, query).await {
            Err(AppError::RateLimited(secs)) if secs <= MAX_SILENT_RETRY_SECS => {
                tokio::time::sleep(Duration::from_secs(secs.max(1))).await;
                self.get_once(cfg, path, query).await
            }
            other => other,
        }
    }

    async fn get_once<T: DeserializeOwned>(
        &self,
        cfg: &ApiConfig,
        path: &str,
        query: &[(&str, String)],
    ) -> AppResult<T> {
        if cfg.api_key.trim().is_empty() {
            return Err(AppError::MissingApiKey);
        }

        let url = format!("{}{}", cfg.api_path, path);
        let response = self
            .http
            .get(&url)
            .header(ACCEPT, "application/json")
            .query(&[("api_key", cfg.api_key.as_str())])
            .query(query)
            .send()
            .await
            // `without_url` : l'URL contient la clé API, elle ne doit jamais
            // apparaître dans un message d'erreur.
            .map_err(|e| AppError::Network(e.without_url().to_string()))?;

        let status = response.status();
        if status.is_success() {
            return response
                .json::<T>()
                .await
                .map_err(|e| AppError::Decode(e.without_url().to_string()));
        }

        let retry_after = retry_after_secs(response.headers());
        let message = response
            .json::<ApiErrorBody>()
            .await
            .ok()
            .and_then(|body| body.error.message)
            .unwrap_or_else(|| status.canonical_reason().unwrap_or("erreur inconnue").to_string());

        Err(match status {
            StatusCode::UNAUTHORIZED => AppError::Unauthorized(message),
            StatusCode::FORBIDDEN => AppError::Forbidden(message),
            StatusCode::NOT_FOUND => AppError::NotFound,
            StatusCode::TOO_MANY_REQUESTS => {
                AppError::RateLimited(retry_after.unwrap_or(DEFAULT_RETRY_SECS))
            }
            other => AppError::Api {
                status: other.as_u16(),
                message,
            },
        })
    }
}

fn retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    ["retry-after", "x-ratelimit-retryafter"]
        .iter()
        .find_map(|name| headers.get(*name)?.to_str().ok()?.trim().parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Game, Page};
    use reqwest::header::HeaderValue;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn reads_retry_after_headers() {
        let mut h = HeaderMap::new();
        assert_eq!(retry_after_secs(&h), None);
        h.insert("x-ratelimit-retryafter", HeaderValue::from_static("12"));
        assert_eq!(retry_after_secs(&h), Some(12));
        h.insert("retry-after", HeaderValue::from_static(" 3 "));
        assert_eq!(retry_after_secs(&h), Some(3));
    }

    /// Petit faux serveur HTTP : renvoie les réponses fournies dans l'ordre
    /// et garde la ligne de requête reçue.
    fn fake_server(responses: Vec<&'static str>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_thread = seen.clone();
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                seen_thread.lock().unwrap().push(line.trim().to_string());
                // Vide les en-têtes.
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" || h.is_empty() {
                        break;
                    }
                }
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    fn cfg(path: String) -> ApiConfig {
        ApiConfig {
            api_key: "secretkey1234".into(),
            api_path: path,
        }
    }

    #[tokio::test]
    async fn sends_key_and_query_and_parses_page() {
        let body = r#"{"data":[{"id":1,"name":"Un jeu"}],"result_count":1,"result_offset":0,"result_limit":1,"result_total":9}"#;
        let resp: &'static str = Box::leak(
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)
                .into_boxed_str(),
        );
        let (path, seen) = fake_server(vec![resp]);
        let page: Page<Game> = ModioClient::new()
            .get(&cfg(path), "/games", &[("_q", "été cool".into())])
            .await
            .unwrap();
        assert_eq!(page.result_total, 9);
        assert_eq!(page.data[0].name.as_deref(), Some("Un jeu"));
        let line = seen.lock().unwrap()[0].clone();
        assert!(line.starts_with("GET /v1/games?api_key=secretkey1234&_q="), "{line}");
    }

    #[tokio::test]
    async fn maps_401_without_leaking_key() {
        let body = r#"{"error":{"code":401,"error_ref":11000,"message":"Invalid API key."}}"#;
        let resp: &'static str = Box::leak(
            format!("HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)
                .into_boxed_str(),
        );
        let (path, _) = fake_server(vec![resp]);
        let err = ModioClient::new()
            .get::<Page<Game>>(&cfg(path), "/games", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(ref m) if m == "Invalid API key."));
        assert!(!err.to_string().contains("secretkey1234"));
    }

    #[tokio::test]
    async fn retries_once_after_short_rate_limit() {
        let ok = r#"{"data":[],"result_total":0}"#;
        let ok_resp: &'static str = Box::leak(
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", ok.len(), ok)
                .into_boxed_str(),
        );
        let limited = "HTTP/1.1 429 Too Many Requests\r\nretry-after: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (path, seen) = fake_server(vec![limited, ok_resp]);
        let page: Page<Game> = ModioClient::new().get(&cfg(path), "/games", &[]).await.unwrap();
        assert!(page.data.is_empty());
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn long_rate_limit_is_reported() {
        let limited = "HTTP/1.1 429 Too Many Requests\r\nretry-after: 45\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (path, _) = fake_server(vec![limited]);
        let err = ModioClient::new()
            .get::<Page<Game>>(&cfg(path), "/games", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::RateLimited(45)));
    }

    #[tokio::test]
    async fn network_errors_hide_the_url() {
        // Port fermé : la connexion échoue.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let err = ModioClient::new()
            .get::<Page<Game>>(&cfg(format!("http://{addr}/v1")), "/games", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Network(_)));
        assert!(!err.to_string().contains("secretkey1234"));
    }
}
