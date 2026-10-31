//! Appels HTTP vers Nexus Mods : API GraphQL v2 (publique, pour parcourir)
//! et API REST v1 (avec la clé API de l'utilisateur, pour télécharger).
//!
//! Chaque requête s'identifie avec `Application-Name` et
//! `Application-Version`, comme l'exigent les règles de l'API :
//! <https://help.nexusmods.com/article/114-api-acceptable-use-policy>

use std::time::Duration;

use reqwest::header::{HeaderMap, ACCEPT};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

pub const API_BASE: &str = "https://api.nexusmods.com";
const APP_NAME: &str = "ModStudio";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const USER_AGENT: &str = concat!("ModStudio/", env!("CARGO_PKG_VERSION"), " (client Nexus Mods non officiel)");
const DEFAULT_RETRY_SECS: u64 = 60;

pub struct NexusClient {
    http: Client,
    downloads: Client,
    base: String,
}

#[derive(Deserialize)]
struct GraphQlBody {
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Deserialize)]
struct GraphQlError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize, Default)]
struct RestError {
    #[serde(default)]
    message: Option<String>,
}

impl NexusClient {
    pub fn new(base: impl Into<String>) -> Self {
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
        Self {
            http,
            downloads,
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Client sans délai total, pour les gros téléchargements.
    pub fn downloads(&self) -> &Client {
        &self.downloads
    }

    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        self.http
            .request(method, url)
            .header(ACCEPT, "application/json")
            .header("Application-Name", APP_NAME)
            .header("Application-Version", APP_VERSION)
    }

    /// Requête GraphQL publique. `data` est décodé en `T`.
    pub async fn graphql<T: DeserializeOwned>(&self, query: &str, variables: Value) -> AppResult<T> {
        let url = format!("{}/v2/graphql", self.base);
        let response = self
            .request(Method::POST, &url)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|e| AppError::NexusNetwork(e.without_url().to_string()))?;
        let status = response.status();
        let retry = retry_after_secs(response.headers());
        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::NexusNetwork(e.without_url().to_string()))?;
        let body: Option<GraphQlBody> = serde_json::from_slice(&bytes).ok();

        if !status.is_success() {
            let message = body
                .as_ref()
                .and_then(|b| b.errors.first())
                .map(|e| e.message.clone())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| status.canonical_reason().unwrap_or(crate::i18n::pick("erreur", "error")).to_string());
            return Err(map_status(status, message, retry, false));
        }
        let body = body.ok_or_else(|| AppError::NexusDecode(crate::tr!("réponse GraphQL illisible", "unreadable GraphQL response")))?;
        match body.data {
            Some(data) if !data.is_null() => {
                serde_json::from_value(data).map_err(|e| AppError::NexusDecode(e.to_string()))
            }
            _ => Err(AppError::NexusApi {
                status: status.as_u16(),
                message: body
                    .errors
                    .first()
                    .map(|e| e.message.clone())
                    .unwrap_or_else(|| crate::tr!("réponse vide", "empty response")),
            }),
        }
    }

    /// GET d'un fichier public (catalogue des jeux Nexus, versions GitHub).
    /// L'erreur est un texte court, sans l'adresse.
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let response = self
            .http
            .get(url)
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let status = response.status();
        if !status.is_success() {
            return Err(crate::tr!("erreur {}", "error {}", status.as_u16()));
        }
        response.bytes().await.map(|b| b.to_vec()).map_err(|e| e.without_url().to_string())
    }

    pub async fn fetch_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let raw = self.fetch_bytes(url).await?;
        serde_json::from_slice(&raw).map_err(|e| crate::tr!("réponse illisible ({e})", "unreadable response ({e})"))
    }

    /// GET sur l'API REST v1 avec la clé API de l'utilisateur.
    pub async fn rest<T: DeserializeOwned>(&self, path: &str, key: &str, query: &[(&str, String)]) -> AppResult<T> {
        self.rest_with(Method::GET, path, key, query, &[]).await
    }

    /// POST / DELETE sur l'API REST v1 (formulaire `form`), avec la clé API.
    pub async fn rest_write<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        key: &str,
        query: &[(&str, String)],
        form: &[(&str, String)],
    ) -> AppResult<T> {
        self.rest_with(method, path, key, query, form).await
    }

    async fn rest_with<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        key: &str,
        query: &[(&str, String)],
        form: &[(&str, String)],
    ) -> AppResult<T> {
        if key.trim().is_empty() {
            return Err(AppError::NexusKeyMissing);
        }
        let url = format!("{}/v1{}", self.base, path);
        let mut request = self.request(method, &url).header("apikey", key).query(query);
        if !form.is_empty() {
            request = request.form(form);
        }
        let response = request
            .send()
            .await
            // L'URL peut contenir la clé d'un lien nxm : jamais dans un message.
            .map_err(|e| AppError::NexusNetwork(e.without_url().to_string()))?;
        let status = response.status();
        if status.is_success() {
            let bytes = response
                .bytes()
                .await
                .map_err(|e| AppError::NexusNetwork(e.without_url().to_string()))?;
            return serde_json::from_slice(&bytes).map_err(|e| AppError::NexusDecode(e.to_string()));
        }
        let retry = retry_after_secs(response.headers());
        let message = response
            .json::<RestError>()
            .await
            .ok()
            .and_then(|b| b.message)
            .unwrap_or_else(|| status.canonical_reason().unwrap_or(crate::i18n::pick("erreur", "error")).to_string());
        Err(map_status(status, message, retry, true))
    }
}

fn map_status(status: StatusCode, message: String, retry: Option<u64>, with_key: bool) -> AppError {
    match status {
        StatusCode::UNAUTHORIZED if with_key => AppError::NexusKeyInvalid,
        StatusCode::NOT_FOUND => AppError::NexusNotFound,
        StatusCode::TOO_MANY_REQUESTS => AppError::NexusRateLimited(retry.unwrap_or(DEFAULT_RETRY_SECS)),
        other => AppError::NexusApi {
            status: other.as_u16(),
            message,
        },
    }
}

fn retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    headers.get("retry-after")?.to_str().ok()?.trim().parse::<u64>().ok()
}

/// Le lien de téléchargement doit pointer vers Nexus Mods (site ou CDN).
pub fn download_url_allowed(url: &str, api_base: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    let Some(host) = u.host_str().map(str::to_ascii_lowercase) else { return false };
    let api_host = reqwest::Url::parse(api_base).ok().and_then(|a| a.host_str().map(str::to_ascii_lowercase));
    // Serveur de test local : même hôte que l'API.
    if api_host.as_deref() == Some(host.as_str()) && host == "127.0.0.1" {
        return true;
    }
    u.scheme() == "https"
        && (host == "nexusmods.com"
            || host.ends_with(".nexusmods.com")
            || host == "nexus-cdn.com"
            || host.ends_with(".nexus-cdn.com"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fake_server, json_response};

    #[tokio::test]
    async fn graphql_sends_app_headers_and_reads_data() {
        let server = fake_server(vec![
            json_response(200, r#"{"data":{"game":{"id":4832}}}"#),
            json_response(200, r#"{"data":null,"errors":[{"message":"Cannot query field"}]}"#),
        ]);
        let client = NexusClient::new(server.base.trim_end_matches("/v1"));
        let v: Value = client.graphql("query { game { id } }", json!({})).await.unwrap();
        assert_eq!(v["game"]["id"], 4832);
        let req = server.request(0);
        assert!(req.line.starts_with("POST /v2/graphql"));
        assert_eq!(req.header("application-name").as_deref(), Some("ModStudio"));
        assert!(req.header("application-version").is_some());
        assert!(req.body.contains("\"query\""));

        let err = client.graphql::<Value>("x", json!({})).await.unwrap_err();
        assert!(matches!(err, AppError::NexusApi { ref message, .. } if message.contains("Cannot query")), "{err}");
    }

    #[tokio::test]
    async fn rest_uses_the_key_header_and_maps_errors() {
        let server = fake_server(vec![
            json_response(200, r#"{"name":"Code"}"#),
            json_response(401, r#"{"message":"Please provide a valid API Key"}"#),
            json_response(403, r#"{"code":403,"message":"premium users only"}"#),
            json_response(429, r#"{"message":"slow down"}"#),
        ]);
        let client = NexusClient::new(server.base.trim_end_matches("/v1"));
        let v: Value = client.rest("/users/validate.json", "cle-secrete", &[]).await.unwrap();
        assert_eq!(v["name"], "Code");
        let req = server.request(0);
        assert!(req.line.starts_with("GET /v1/users/validate.json"));
        assert_eq!(req.header("apikey").as_deref(), Some("cle-secrete"));
        assert!(!req.line.contains("cle-secrete"), "la clé ne passe jamais dans l'URL");

        assert!(matches!(client.rest::<Value>("/x", "k", &[]).await, Err(AppError::NexusKeyInvalid)));
        assert!(matches!(
            client.rest::<Value>("/x", "k", &[]).await,
            Err(AppError::NexusApi { status: 403, .. })
        ));
        assert!(matches!(client.rest::<Value>("/x", "k", &[]).await, Err(AppError::NexusRateLimited(60))));
        assert!(matches!(client.rest::<Value>("/x", " ", &[]).await, Err(AppError::NexusKeyMissing)));
    }

    #[test]
    fn downloads_must_come_from_nexus() {
        let api = API_BASE;
        assert!(download_url_allowed("https://cf-files.nexusmods.com/cdn/4832/148/x.zip?md5=a", api));
        assert!(download_url_allowed("https://supporter-files.nexus-cdn.com/4832/148/x.zip", api));
        assert!(!download_url_allowed("http://cf-files.nexusmods.com/x.zip", api));
        assert!(!download_url_allowed("https://nexusmods.com.evil.example/x.zip", api));
        assert!(!download_url_allowed("https://evil.example/?nexusmods.com", api));
        assert!(download_url_allowed("http://127.0.0.1:9/f", "http://127.0.0.1:9"));
    }
}
