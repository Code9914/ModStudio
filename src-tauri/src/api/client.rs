//! Appels HTTP vers l'API REST mod.io.

use std::time::Duration;

use reqwest::header::{HeaderMap, ACCEPT};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;

use super::models::ApiErrorBody;
use crate::error::{AppError, AppResult};

/// Au-delà de ce délai, on ne patiente pas en silence : on laisse l'interface
/// afficher un compte à rebours.
const MAX_SILENT_RETRY_SECS: u64 = 5;

/// Plateforme demandée à mod.io : les fichiers PC (y compris pour jouer via
/// Proton sous Linux).
pub const TARGET_PLATFORM: &str = "windows";
const DEFAULT_RETRY_SECS: u64 = 60;

/// Clé + chemin d'API utilisés pour une requête (copiés depuis les réglages).
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub api_key: String,
    pub api_path: String,
}

/// Comment une requête s'authentifie auprès de mod.io.
#[derive(Debug, Clone, Copy)]
pub enum Auth<'a> {
    /// Clé API en paramètre : lecture seule et flux de connexion.
    ApiKey,
    /// Jeton OAuth 2 de l'utilisateur, dans l'en-tête `Authorization`.
    Token(&'a str),
}

pub struct ModioClient {
    http: Client,
    /// Client sans délai total, pour les gros téléchargements.
    downloads: Client,
}

const USER_AGENT: &str = concat!("ModStudio/", env!("CARGO_PKG_VERSION"), " (client mod.io non officiel)");

impl ModioClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("client HTTP");
        let downloads = Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("client HTTP");
        Self { http, downloads }
    }

    pub fn downloads(&self) -> &Client {
        &self.downloads
    }

    /// GET avec la clé API.
    pub async fn get<T: DeserializeOwned>(
        &self,
        cfg: &ApiConfig,
        path: &str,
        query: &[(&str, String)],
    ) -> AppResult<T> {
        self.get_as(cfg, Auth::ApiKey, path, query).await
    }

    /// GET avec l'authentification choisie, et un nouvel essai automatique si
    /// mod.io demande d'attendre quelques secondes.
    pub async fn get_as<T: DeserializeOwned>(
        &self,
        cfg: &ApiConfig,
        auth: Auth<'_>,
        path: &str,
        query: &[(&str, String)],
    ) -> AppResult<T> {
        match self.send(cfg, auth, Method::GET, path, query, None).await {
            Err(AppError::RateLimited(secs)) if secs <= MAX_SILENT_RETRY_SECS => {
                tokio::time::sleep(Duration::from_secs(secs.max(1))).await;
                self.send(cfg, auth, Method::GET, path, query, None).await
            }
            other => other,
        }
    }

    /// POST `application/x-www-form-urlencoded`.
    pub async fn post_form<T: DeserializeOwned>(
        &self,
        cfg: &ApiConfig,
        auth: Auth<'_>,
        path: &str,
        form: &[(&str, String)],
    ) -> AppResult<T> {
        self.send(cfg, auth, Method::POST, path, &[], Some(form)).await
    }

    pub async fn delete<T: DeserializeOwned>(
        &self,
        cfg: &ApiConfig,
        auth: Auth<'_>,
        path: &str,
    ) -> AppResult<T> {
        // mod.io attend le même type de contenu que pour un POST, même vide.
        self.send(cfg, auth, Method::DELETE, path, &[], Some(&[])).await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        cfg: &ApiConfig,
        auth: Auth<'_>,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        form: Option<&[(&str, String)]>,
    ) -> AppResult<T> {
        let url = format!("{}{}", cfg.api_path, path);
        let mut request = self.http.request(method, &url).header(ACCEPT, "application/json");
        // Les jeux qui publient un fichier par plateforme (PC, Quest…) ne
        // renvoient le fichier actuel d'un mod que si on dit laquelle on veut.
        // Pas sur /me ni /oauth : /me/subscribed exigerait alors un filtre de jeu.
        if !path.starts_with("/me") && !path.starts_with("/oauth") {
            request = request.header("X-Modio-Platform", TARGET_PLATFORM);
        }

        request = match auth {
            Auth::ApiKey => {
                if cfg.api_key.trim().is_empty() {
                    return Err(AppError::MissingApiKey);
                }
                request.query(&[("api_key", cfg.api_key.as_str())])
            }
            Auth::Token(token) => request.bearer_auth(token),
        };
        request = request.query(query);
        if let Some(form) = form {
            request = request.form(form);
        }

        let response = request
            .send()
            .await
            // `without_url` : l'URL contient la clé API, elle ne doit jamais
            // apparaître dans un message d'erreur.
            .map_err(|e| AppError::Network(e.without_url().to_string()))?;

        let status = response.status();
        if status.is_success() {
            let body = response
                .bytes()
                .await
                .map_err(|e| AppError::Network(e.without_url().to_string()))?;
            // 204 No Content : rien à lire.
            let body: &[u8] = if body.iter().all(u8::is_ascii_whitespace) { b"null" } else { &body };
            return serde_json::from_slice(body).map_err(|e| AppError::Decode(e.to_string()));
        }

        let retry_after = retry_after_secs(response.headers());
        let detail = response
            .json::<ApiErrorBody>()
            .await
            .map(|b| b.error)
            .unwrap_or_default();
        let message = detail
            .message
            .unwrap_or_else(|| status.canonical_reason().unwrap_or("erreur inconnue").to_string());
        Err(map_status(status, detail.error_ref, message, retry_after, auth))
    }
}

fn map_status(
    status: StatusCode,
    error_ref: Option<i64>,
    message: String,
    retry_after: Option<u64>,
    auth: Auth<'_>,
) -> AppError {
    match status {
        // Avec un jeton, un 401 veut dire que la session n'est plus valable.
        StatusCode::UNAUTHORIZED if matches!(auth, Auth::Token(_)) => AppError::SessionExpired,
        StatusCode::UNAUTHORIZED => AppError::Unauthorized { message, error_ref },
        StatusCode::FORBIDDEN => AppError::Forbidden { message, error_ref },
        StatusCode::NOT_FOUND => AppError::NotFound,
        StatusCode::TOO_MANY_REQUESTS => AppError::RateLimited(retry_after.unwrap_or(DEFAULT_RETRY_SECS)),
        other => AppError::Api {
            status: other.as_u16(),
            message,
            error_ref,
        },
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
    use crate::testutil::{fake_server, json_response, ApiConfigExt};
    use reqwest::header::HeaderValue;
    use serde::de::IgnoredAny;
    use std::net::TcpListener;

    #[test]
    fn reads_retry_after_headers() {
        let mut h = HeaderMap::new();
        assert_eq!(retry_after_secs(&h), None);
        h.insert("x-ratelimit-retryafter", HeaderValue::from_static("12"));
        assert_eq!(retry_after_secs(&h), Some(12));
        h.insert("retry-after", HeaderValue::from_static(" 3 "));
        assert_eq!(retry_after_secs(&h), Some(3));
    }

    #[tokio::test]
    async fn sends_key_and_query_and_parses_page() {
        let body = r#"{"data":[{"id":1,"name":"Un jeu"}],"result_count":1,"result_offset":0,"result_limit":1,"result_total":9}"#;
        let server = fake_server(vec![json_response(200, body)]);
        let page: Page<Game> = ModioClient::new()
            .get(&ApiConfig::test(&server.base), "/games", &[("_q", "été cool".into())])
            .await
            .unwrap();
        assert_eq!(page.result_total, 9);
        assert_eq!(page.data[0].name.as_deref(), Some("Un jeu"));
        let req = server.request(0);
        assert!(req.line.starts_with("GET /v1/games?api_key=secretkey1234&_q="), "{}", req.line);
        assert!(req.header("authorization").is_none());
        assert_eq!(req.header("x-modio-platform").as_deref(), Some("windows"));
    }

    #[tokio::test]
    async fn maps_401_without_leaking_key() {
        let body = r#"{"error":{"code":401,"error_ref":11002,"message":"Invalid API key."}}"#;
        let server = fake_server(vec![json_response(401, body)]);
        let err = ModioClient::new()
            .get::<Page<Game>>(&ApiConfig::test(&server.base), "/games", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized { ref message, error_ref: Some(11002) } if message == "Invalid API key."));
        assert!(!err.to_string().contains("secretkey1234"));
    }

    #[tokio::test]
    async fn token_requests_use_bearer_and_401_means_session_expired() {
        let body = r#"{"error":{"code":401,"error_ref":11005,"message":"Expired."}}"#;
        let server = fake_server(vec![json_response(401, body)]);
        let err = ModioClient::new()
            .get_as::<IgnoredAny>(&ApiConfig::test(&server.base), Auth::Token("tok123"), "/me", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::SessionExpired));
        let req = server.request(0);
        assert_eq!(req.header("authorization").as_deref(), Some("Bearer tok123"));
        assert!(!req.line.contains("api_key"), "{}", req.line);
        // /me/subscribed refuserait une plateforme sans filtre de jeu.
        assert!(req.header("x-modio-platform").is_none());
    }

    #[tokio::test]
    async fn posts_form_and_accepts_empty_204() {
        let server = fake_server(vec![
            json_response(200, r#"{"code":200,"message":"ok"}"#),
            "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_string(),
        ]);
        let client = ModioClient::new();
        let cfg = ApiConfig::test(&server.base);
        let _: IgnoredAny = client
            .post_form(&cfg, Auth::ApiKey, "/oauth/emailrequest", &[("email", "a+b@exemple.fr".into())])
            .await
            .unwrap();
        let _: IgnoredAny = client
            .delete(&cfg, Auth::Token("t"), "/games/1/mods/2/subscribe")
            .await
            .unwrap();
        let post = server.request(0);
        assert!(post.line.starts_with("POST /v1/oauth/emailrequest?api_key=secretkey1234"), "{}", post.line);
        assert_eq!(post.header("content-type").as_deref(), Some("application/x-www-form-urlencoded"));
        assert_eq!(post.body, "email=a%2Bb%40exemple.fr");
        assert!(server.request(1).line.starts_with("DELETE /v1/games/1/mods/2/subscribe"));
    }

    #[tokio::test]
    async fn retries_once_after_short_rate_limit() {
        let limited = "HTTP/1.1 429 Too Many Requests\r\nretry-after: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
        let server = fake_server(vec![limited, json_response(200, r#"{"data":[],"result_total":0}"#)]);
        let page: Page<Game> = ModioClient::new()
            .get(&ApiConfig::test(&server.base), "/games", &[])
            .await
            .unwrap();
        assert!(page.data.is_empty());
        assert_eq!(server.count(), 2);
    }

    #[tokio::test]
    async fn long_rate_limit_is_reported() {
        let limited = "HTTP/1.1 429 Too Many Requests\r\nretry-after: 45\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
        let server = fake_server(vec![limited]);
        let err = ModioClient::new()
            .get::<Page<Game>>(&ApiConfig::test(&server.base), "/games", &[])
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
            .get::<Page<Game>>(&ApiConfig::test(&format!("http://{addr}/v1")), "/games", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Network(_)));
        assert!(!err.to_string().contains("secretkey1234"));
    }
}
