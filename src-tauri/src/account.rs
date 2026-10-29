//! Compte mod.io : connexion, déconnexion et abonnements.
//!
//! La logique vit dans des fonctions qui prennent `&AppState` (testables sans
//! Tauri) ; les commandes `#[tauri::command]` en bas du fichier les appellent.

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::IgnoredAny;
use tauri::State;

use crate::api::Auth;
use crate::api::models::{AccessToken, Game, Mod, Page, User};
use crate::auth::{self, StoredToken, TokenSource};
use crate::commands::AppState;
use crate::error::{AppError, AppResult};

/// Pages de 100 lues au maximum pour la liste complète des abonnements.
const MAX_SUBSCRIPTION_PAGES: u32 = 30;
/// Pages de 100 lues au maximum pour les abonnements d'un seul jeu.
const MAX_GAME_SUBSCRIPTION_PAGES: u32 = 20;

#[derive(Default)]
pub struct AuthState {
    /// Le trousseau a déjà été lu.
    loaded: bool,
    token: Option<StoredToken>,
    user: Option<User>,
    /// Le jeton est bien enregistré dans le trousseau.
    persisted: bool,
    /// Pourquoi le trousseau n'a pas pu être utilisé, le cas échéant.
    storage_warning: Option<String>,
    /// La dernière session a expiré (pour l'expliquer dans l'interface).
    expired: bool,
}

/// Ce que l'interface voit de la connexion. Jamais le jeton lui-même.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthView {
    pub logged_in: bool,
    pub user: Option<User>,
    pub source: Option<TokenSource>,
    pub persisted: bool,
    pub storage_warning: Option<String>,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MyMods {
    pub games: Vec<Game>,
    pub mods: Vec<Mod>,
    /// Vrai si la liste a été coupée (plus de 3 000 abonnements).
    pub truncated: bool,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl AppState {
    /// Lit le trousseau une seule fois, au premier besoin.
    fn ensure_auth_loaded(&self) {
        if self.auth.read().expect("auth").loaded {
            return;
        }
        let loaded = self.secrets.load();
        let mut a = self.auth.write().expect("auth");
        if a.loaded {
            return;
        }
        a.loaded = true;
        match loaded {
            Ok(Some(raw)) => match StoredToken::from_json(&raw) {
                Some(t) if t.is_expired(now()) => {
                    let _ = self.secrets.clear();
                    a.expired = true;
                }
                Some(t) => {
                    a.token = Some(t);
                    a.persisted = true;
                }
                None => {
                    let _ = self.secrets.clear();
                }
            },
            Ok(None) => {}
            Err(e) => a.storage_warning = Some(format!("Trousseau du système inaccessible : {e}")),
        }
    }

    pub(crate) fn token(&self) -> AppResult<String> {
        self.ensure_auth_loaded();
        self.auth
            .read()
            .expect("auth")
            .token
            .as_ref()
            .map(|t| t.access_token.clone())
            .ok_or(AppError::NotLoggedIn)
    }

    fn auth_view(&self) -> AuthView {
        let a = self.auth.read().expect("auth");
        AuthView {
            logged_in: a.token.is_some(),
            user: a.user.clone(),
            source: a.token.as_ref().map(|t| t.source),
            persisted: a.token.is_some() && a.persisted,
            storage_warning: a.storage_warning.clone(),
            expired: a.expired,
        }
    }

    fn start_session(&self, token: StoredToken, user: User) {
        let saved = self.secrets.save(&token.to_json());
        let mut a = self.auth.write().expect("auth");
        a.loaded = true;
        a.persisted = saved.is_ok();
        a.storage_warning = saved.err().map(|e| {
            format!("Ta connexion n'a pas pu être enregistrée dans le trousseau du système ({e}). Tu devras te reconnecter au prochain lancement.")
        });
        a.token = Some(token);
        a.user = Some(user);
        a.expired = false;
    }

    fn end_session(&self, expired: bool) {
        let _ = self.secrets.clear();
        let mut a = self.auth.write().expect("auth");
        a.loaded = true;
        a.token = None;
        a.user = None;
        a.persisted = false;
        a.expired = expired;
    }

    /// Si mod.io dit que la session n'est plus valable, on l'oublie.
    pub(crate) fn check_session<T>(&self, result: AppResult<T>) -> AppResult<T> {
        if matches!(result, Err(AppError::SessionExpired)) {
            self.end_session(true);
        }
        result
    }
}

// ---------------------------------------------------------------- Connexion

pub async fn status(state: &AppState) -> AppResult<AuthView> {
    let token = match state.token() {
        Ok(t) => t,
        Err(_) => return Ok(state.auth_view()),
    };
    let has_user = state.auth.read().expect("auth").user.is_some();
    if !has_user {
        let me: AppResult<User> = state
            .client
            .get_as(&state.api_config(), Auth::Token(&token), "/me", &[])
            .await;
        match state.check_session(me) {
            Ok(user) => state.auth.write().expect("auth").user = Some(user),
            Err(AppError::SessionExpired) => {}
            // Hors ligne : on reste connecté, le nom s'affichera plus tard.
            Err(_) => {}
        }
    }
    Ok(state.auth_view())
}

pub async fn request_code(state: &AppState, email: &str) -> AppResult<()> {
    let email = auth::normalize_email(email)
        .ok_or_else(|| AppError::InvalidArgument("cette adresse e-mail n'est pas valide.".into()))?;
    let result: AppResult<IgnoredAny> = state
        .client
        .post_form(&state.api_config(), Auth::ApiKey, "/oauth/emailrequest", &[("email", email)])
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(e @ (AppError::Unauthorized { .. } | AppError::Forbidden { .. })) => {
            let detail = match &e {
                AppError::Unauthorized { message, .. } | AppError::Forbidden { message, .. } => message.clone(),
                _ => unreachable!(),
            };
            Err(AppError::EmailLoginUnavailable(detail))
        }
        Err(e) => Err(e),
    }
}

pub async fn exchange_code(state: &AppState, code: &str) -> AppResult<AuthView> {
    let code = auth::normalize_code(code)
        .ok_or_else(|| AppError::InvalidCode("Ce code n'a pas le bon format : recopie les caractères reçus par e-mail.".into()))?;
    let cfg = state.api_config();
    let token: AccessToken = state
        .client
        .post_form(&cfg, Auth::ApiKey, "/oauth/emailexchange", &[("security_code", code)])
        .await
        .map_err(|e| match e.error_ref() {
            Some(11011) => AppError::InvalidCode("Ce code a déjà été utilisé. Demande un nouveau code.".into()),
            Some(11012) => AppError::InvalidCode("Ce code a expiré. Demande un nouveau code.".into()),
            Some(11013) => AppError::InvalidCode(
                "Ce code a été demandé avec une autre clé API. Demande un nouveau code.".into(),
            ),
            Some(11014) => AppError::InvalidCode("Ce code n'est pas valide. Vérifie-le ou demande un nouveau code.".into()),
            _ if e.status() == Some(401) => AppError::InvalidCode("mod.io a refusé ce code.".into()),
            _ => e,
        })?;
    if token.access_token.trim().is_empty() {
        return Err(AppError::Decode("mod.io n'a pas renvoyé de jeton.".into()));
    }

    let user: User = state
        .client
        .get_as(&cfg, Auth::Token(&token.access_token), "/me", &[])
        .await?;
    state.start_session(
        StoredToken {
            access_token: token.access_token,
            date_expires: token.date_expires,
            source: TokenSource::Email,
        },
        user,
    );
    Ok(state.auth_view())
}

pub async fn use_token(state: &AppState, token: &str) -> AppResult<AuthView> {
    let token = auth::normalize_token(token).ok_or(AppError::InvalidToken)?;
    let user: User = state
        .client
        .get_as(&state.api_config(), Auth::Token(&token), "/me", &[])
        .await
        .map_err(|e| match e {
            AppError::SessionExpired | AppError::Forbidden { .. } => AppError::InvalidToken,
            other => other,
        })?;
    state.start_session(
        StoredToken {
            access_token: token,
            date_expires: None,
            source: TokenSource::Manual,
        },
        user,
    );
    Ok(state.auth_view())
}

pub async fn logout(state: &AppState) -> AppResult<AuthView> {
    state.ensure_auth_loaded();
    let token = state.auth.read().expect("auth").token.clone();
    if let Some(t) = token {
        // Un jeton obtenu par e-mail appartient à l'appli : on le révoque.
        // Un jeton créé à la main sur mod.io reste à l'utilisateur : on l'oublie seulement.
        if t.source == TokenSource::Email {
            let _: AppResult<IgnoredAny> = state
                .client
                .post_form(&state.api_config(), Auth::Token(&t.access_token), "/oauth/logout", &[])
                .await;
        }
    }
    state.end_session(false);
    Ok(state.auth_view())
}

// ---------------------------------------------------------------- Abonnements

/// Tous les abonnements, avec les jeux correspondants.
pub async fn my_mods(state: &AppState) -> AppResult<MyMods> {
    let token = state.token()?;
    let cfg = state.api_config();
    let mut mods: Vec<Mod> = Vec::new();
    let mut truncated = false;

    for page_index in 0..MAX_SUBSCRIPTION_PAGES {
        let page: Page<Mod> = state.check_session(
            state
                .client
                .get_as(
                    &cfg,
                    Auth::Token(&token),
                    "/me/subscribed",
                    &[
                        ("_sort", "name".into()),
                        ("_limit", "100".into()),
                        ("_offset", (page_index * 100).to_string()),
                    ],
                )
                .await,
        )?;
        let received = page.data.len();
        mods.extend(page.data.into_iter().map(Mod::into_summary));
        if received == 0 || mods.len() as u32 >= page.result_total {
            break;
        }
        if page_index + 1 == MAX_SUBSCRIPTION_PAGES {
            truncated = true;
        }
    }

    let game_ids: Vec<i64> = mods
        .iter()
        .map(|m| m.game_id)
        .filter(|id| *id > 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut games = Vec::new();
    for chunk in game_ids.chunks(100) {
        let ids = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let page: Page<Game> = state
            .client
            .get(&cfg, "/games", &[("id-in", ids), ("_limit", "100".into())])
            .await?;
        games.extend(page.data);
    }
    Ok(MyMods { games, mods, truncated })
}

/// Identifiants des mods suivis dans un jeu (vide si non connecté).
pub async fn subscribed_ids(state: &AppState, game_id: i64) -> AppResult<Vec<i64>> {
    let Ok(token) = state.token() else { return Ok(Vec::new()) };
    let cfg = state.api_config();
    let mut ids = Vec::new();
    for page_index in 0..MAX_GAME_SUBSCRIPTION_PAGES {
        let page: Page<Mod> = state.check_session(
            state
                .client
                .get_as(
                    &cfg,
                    Auth::Token(&token),
                    "/me/subscribed",
                    &[
                        ("game_id", game_id.to_string()),
                        ("_limit", "100".into()),
                        ("_offset", (page_index * 100).to_string()),
                    ],
                )
                .await,
        )?;
        let received = page.data.len();
        ids.extend(page.data.iter().map(|m| m.id));
        if received == 0 || ids.len() as u32 >= page.result_total {
            break;
        }
    }
    Ok(ids)
}

pub async fn subscribe(state: &AppState, game_id: i64, mod_id: i64) -> AppResult<()> {
    check_ids(game_id, mod_id)?;
    let token = state.token()?;
    let result: AppResult<IgnoredAny> = state
        .client
        .post_form(
            &state.api_config(),
            Auth::Token(&token),
            &format!("/games/{game_id}/mods/{mod_id}/subscribe"),
            &[],
        )
        .await;
    match state.check_session(result) {
        Ok(_) => Ok(()),
        // Déjà abonné : le résultat voulu est atteint.
        Err(e) if e.status() == Some(400) => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn unsubscribe(state: &AppState, game_id: i64, mod_id: i64) -> AppResult<()> {
    check_ids(game_id, mod_id)?;
    let token = state.token()?;
    let result: AppResult<IgnoredAny> = state
        .client
        .delete(
            &state.api_config(),
            Auth::Token(&token),
            &format!("/games/{game_id}/mods/{mod_id}/subscribe"),
        )
        .await;
    match state.check_session(result) {
        Ok(_) => Ok(()),
        // Pas abonné (15005) : le résultat voulu est atteint.
        Err(e) if e.status() == Some(400) => Ok(()),
        Err(e) => Err(e),
    }
}

fn check_ids(game_id: i64, mod_id: i64) -> AppResult<()> {
    if game_id > 0 && mod_id > 0 {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(format!("identifiants {game_id}/{mod_id}")))
    }
}

// ---------------------------------------------------------------- Commandes Tauri

#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> AppResult<AuthView> {
    status(&state).await
}

#[tauri::command]
pub async fn auth_request_code(state: State<'_, AppState>, email: String) -> AppResult<()> {
    request_code(&state, &email).await
}

#[tauri::command]
pub async fn auth_exchange_code(state: State<'_, AppState>, code: String) -> AppResult<AuthView> {
    exchange_code(&state, &code).await
}

#[tauri::command]
pub async fn auth_use_token(state: State<'_, AppState>, token: String) -> AppResult<AuthView> {
    use_token(&state, &token).await
}

#[tauri::command]
pub async fn auth_logout(state: State<'_, AppState>) -> AppResult<AuthView> {
    logout(&state).await
}

#[tauri::command]
pub async fn list_my_mods(state: State<'_, AppState>) -> AppResult<MyMods> {
    my_mods(&state).await
}

#[tauri::command]
pub async fn list_subscribed_ids(state: State<'_, AppState>, game_id: i64) -> AppResult<Vec<i64>> {
    subscribed_ids(&state, game_id).await
}

#[tauri::command]
pub async fn subscribe_mod(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<()> {
    subscribe(&state, game_id, mod_id).await
}

#[tauri::command]
pub async fn unsubscribe_mod(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<()> {
    unsubscribe(&state, game_id, mod_id).await
}

// ---------------------------------------------------------------- Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::settings::Settings;
    use crate::testutil::{fake_server, json_response};
    use std::sync::Arc;

    /// `SecretStore` partagé pour pouvoir inspecter ce que l'appli a rangé.
    struct Shared(Arc<MemoryStore>);
    impl crate::auth::SecretStore for Shared {
        fn load(&self) -> Result<Option<String>, String> {
            self.0.load()
        }
        fn save(&self, v: &str) -> Result<(), String> {
            self.0.save(v)
        }
        fn clear(&self) -> Result<(), String> {
            self.0.clear()
        }
    }

    fn state(base: &str, store: Arc<MemoryStore>) -> AppState {
        let settings = Settings {
            api_key: "secretkey1234".into(),
            api_path: base.to_string(),
            hide_mature: false,
        };
        AppState::new(settings, std::env::temp_dir().join("modstudio-test.json"), Box::new(Shared(store)))
    }

    const ME: &str = r#"{"id":7,"username":"Code9914","avatar":{"thumb_50x50":"https://img/a.png"}}"#;

    #[tokio::test]
    async fn email_login_stores_token_in_keyring() {
        let server = fake_server(vec![
            json_response(200, r#"{"code":200,"access_token":"tok-email-123","date_expires":4102444800}"#),
            json_response(200, ME),
        ]);
        let store = Arc::new(MemoryStore::default());
        let s = state(&server.base, store.clone());

        let view = exchange_code(&s, " p8e5t ").await.unwrap();
        assert!(view.logged_in && view.persisted);
        assert_eq!(view.source, Some(TokenSource::Email));
        assert_eq!(view.user.unwrap().username.as_deref(), Some("Code9914"));
        assert_eq!(server.request(0).body, "security_code=P8E5T");
        assert_eq!(server.request(1).header("authorization").as_deref(), Some("Bearer tok-email-123"));

        let saved = StoredToken::from_json(&store.get().unwrap()).unwrap();
        assert_eq!(saved.access_token, "tok-email-123");
        // Le jeton n'apparaît jamais dans ce que voit l'interface.
        assert!(!serde_json::to_string(&s.auth_view()).unwrap().contains("tok-email-123"));
    }

    #[tokio::test]
    async fn expired_code_gets_a_clear_message() {
        let server = fake_server(vec![json_response(
            401,
            r#"{"error":{"code":401,"error_ref":11012,"message":"expired"}}"#,
        )]);
        let s = state(&server.base, Arc::new(MemoryStore::default()));
        let err = exchange_code(&s, "ABCDE").await.unwrap_err();
        assert!(matches!(err, AppError::InvalidCode(ref m) if m.contains("expiré")), "{err}");
    }

    #[tokio::test]
    async fn email_request_refused_suggests_token() {
        let server = fake_server(vec![json_response(
            403,
            r#"{"error":{"code":403,"error_ref":11016,"message":"Game API key required."}}"#,
        )]);
        let s = state(&server.base, Arc::new(MemoryStore::default()));
        let err = request_code(&s, "moi@exemple.fr").await.unwrap_err();
        assert!(matches!(err, AppError::EmailLoginUnavailable(_)), "{err}");
        assert!(request_code(&s, "pas-un-email").await.is_err());
    }

    #[tokio::test]
    async fn manual_token_is_checked_then_saved() {
        let server = fake_server(vec![
            json_response(401, r#"{"error":{"code":401,"message":"no"}}"#),
            json_response(200, ME),
        ]);
        let store = Arc::new(MemoryStore::default());
        let s = state(&server.base, store.clone());
        let bad = "b".repeat(30);
        assert!(matches!(use_token(&s, &bad).await, Err(AppError::InvalidToken)));
        assert!(store.get().is_none());

        let good = "g".repeat(30);
        let view = use_token(&s, &good).await.unwrap();
        assert_eq!(view.source, Some(TokenSource::Manual));
        assert!(store.get().unwrap().contains(&good));
    }

    #[tokio::test]
    async fn saved_session_is_restored_and_expired_one_is_dropped() {
        let server = fake_server(vec![json_response(200, ME)]);
        let saved = StoredToken {
            access_token: "tok-saved-1".into(),
            date_expires: None,
            source: TokenSource::Manual,
        };
        let s = state(&server.base, Arc::new(MemoryStore::with(Some(saved.to_json()))));
        let view = status(&s).await.unwrap();
        assert!(view.logged_in);
        assert_eq!(view.user.unwrap().id, 7);

        let old = StoredToken {
            date_expires: Some(1),
            ..saved
        };
        let store = Arc::new(MemoryStore::with(Some(old.to_json())));
        let s = state("http://127.0.0.1:9/v1", store.clone());
        let view = status(&s).await.unwrap();
        assert!(!view.logged_in && view.expired);
        assert!(store.get().is_none());
    }

    #[tokio::test]
    async fn revoked_session_is_forgotten() {
        let server = fake_server(vec![json_response(401, r#"{"error":{"code":401,"error_ref":11005,"message":"revoked"}}"#)]);
        let saved = StoredToken {
            access_token: "tok-revoked".into(),
            date_expires: None,
            source: TokenSource::Email,
        };
        let store = Arc::new(MemoryStore::with(Some(saved.to_json())));
        let s = state(&server.base, store.clone());
        let err = subscribe(&s, 1, 2).await.unwrap_err();
        assert!(matches!(err, AppError::SessionExpired));
        assert!(store.get().is_none());
        assert!(matches!(subscribe(&s, 1, 2).await, Err(AppError::NotLoggedIn)));
    }

    #[tokio::test]
    async fn subscribe_and_unsubscribe_are_idempotent() {
        let server = fake_server(vec![
            json_response(201, r#"{"id":2,"game_id":1}"#),
            json_response(400, r#"{"error":{"code":400,"error_ref":15004,"message":"already"}}"#),
            "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_string(),
            json_response(400, r#"{"error":{"code":400,"error_ref":15005,"message":"not subscribed"}}"#),
        ]);
        let saved = StoredToken {
            access_token: "tok".into(),
            date_expires: None,
            source: TokenSource::Manual,
        };
        let s = state(&server.base, Arc::new(MemoryStore::with(Some(saved.to_json()))));
        subscribe(&s, 1, 2).await.unwrap();
        subscribe(&s, 1, 2).await.unwrap();
        unsubscribe(&s, 1, 2).await.unwrap();
        unsubscribe(&s, 1, 2).await.unwrap();
        assert!(server.request(0).line.starts_with("POST /v1/games/1/mods/2/subscribe"));
        assert!(server.request(2).line.starts_with("DELETE /v1/games/1/mods/2/subscribe"));
    }

    #[tokio::test]
    async fn my_mods_pages_and_fetches_games() {
        let page1 = format!(
            r#"{{"data":[{}],"result_count":100,"result_offset":0,"result_limit":100,"result_total":101}}"#,
            (0..100)
                .map(|i| format!(r#"{{"id":{},"game_id":{},"name":"M{i}","description":"<p>lourd</p>"}}"#, 1000 + i, 1 + i % 2))
                .collect::<Vec<_>>()
                .join(",")
        );
        let page2 = r#"{"data":[{"id":5000,"game_id":3,"name":"Dernier"}],"result_count":1,"result_offset":100,"result_limit":100,"result_total":101}"#;
        let games = r#"{"data":[{"id":1,"name":"A"},{"id":2,"name":"B"},{"id":3,"name":"C"}],"result_total":3}"#;
        let server = fake_server(vec![json_response(200, &page1), json_response(200, page2), json_response(200, games)]);
        let saved = StoredToken {
            access_token: "tok".into(),
            date_expires: None,
            source: TokenSource::Manual,
        };
        let s = state(&server.base, Arc::new(MemoryStore::with(Some(saved.to_json()))));
        let mine = my_mods(&s).await.unwrap();
        assert_eq!(mine.mods.len(), 101);
        assert_eq!(mine.games.len(), 3);
        assert!(!mine.truncated);
        assert!(mine.mods[0].description.is_none(), "les descriptions sont retirées");
        assert!(server.request(1).line.contains("_offset=100"));
        assert!(server.request(2).line.contains("id-in=1%2C2%2C3"), "{}", server.request(2).line);
    }

    #[tokio::test]
    async fn logout_revokes_email_tokens_only() {
        let server = fake_server(vec![json_response(200, r#"{"code":200,"message":"ok"}"#)]);
        let email = StoredToken {
            access_token: "tok-e".into(),
            date_expires: None,
            source: TokenSource::Email,
        };
        let store = Arc::new(MemoryStore::with(Some(email.to_json())));
        let s = state(&server.base, store.clone());
        let view = logout(&s).await.unwrap();
        assert!(!view.logged_in);
        assert!(store.get().is_none());
        assert!(server.request(0).line.starts_with("POST /v1/oauth/logout"));

        // Jeton manuel : aucun appel réseau (le serveur n'a plus de réponse prévue).
        let manual = StoredToken {
            source: TokenSource::Manual,
            ..email
        };
        let store = Arc::new(MemoryStore::with(Some(manual.to_json())));
        let s = state("http://127.0.0.1:9/v1", store.clone());
        logout(&s).await.unwrap();
        assert!(store.get().is_none());
    }

    #[tokio::test]
    async fn keyring_failure_keeps_session_in_memory_with_warning() {
        let server = fake_server(vec![json_response(200, ME)]);
        let store = Arc::new(MemoryStore::failing());
        let s = state(&server.base, store);
        let view = use_token(&s, &"z".repeat(30)).await.unwrap();
        assert!(view.logged_in);
        assert!(!view.persisted);
        assert!(view.storage_warning.unwrap().contains("prochain lancement"));
    }
}
