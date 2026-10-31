//! Traduction des descriptions de mods sur l'ordinateur : les modèles de
//! Firefox (Bergamot) sont téléchargés une fois depuis le CDN de Mozilla,
//! vérifiés par leur empreinte et gardés dans le dossier de ModStudio ; la
//! traduction elle-même se fait dans l'interface (moteur WASM). Le texte des
//! descriptions ne quitte jamais l'ordinateur.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{redirect, Client};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::ipc::{Channel, Response};
use tauri::State;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::installer::InstallEvent;

/// Modèles retenus (empreintes épinglées), voir `translation-models.json`.
const TABLE: &str = include_str!("../translation-models.json");
/// Seul serveur d'où viennent les modèles.
const HOST: &str = "firefox-settings-attachments.cdn.mozilla.net";
const USER_AGENT: &str = concat!("ModStudio/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Deserialize)]
pub struct Table {
    pub source: String,
    pub models: Vec<Model>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub from: String,
    pub to: String,
    pub version: String,
    /// `model`, `lex`, et `vocab` (ou `srcvocab` et `trgvocab`).
    pub files: BTreeMap<String, ModelFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelFile {
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

pub fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(TABLE).expect("translation-models.json"))
}

impl Model {
    fn dir_name(&self) -> String {
        format!("{}-{}-{}", self.from, self.to, self.version)
    }
}

/// Un modèle à charger dans le moteur.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub from: String,
    pub to: String,
    pub version: String,
    pub files: Vec<String>,
}

/// Ce qu'il faut pour traduire d'une langue à l'autre.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPlan {
    /// Un modèle, ou deux en passant par l'anglais.
    pub steps: Vec<Step>,
    /// Octets encore à télécharger (0 : tout est prêt).
    pub missing: u64,
}

/// Les modèles pour traduire de `from` vers `to` : direct, sinon par l'anglais.
pub fn route<'a>(table: &'a Table, from: &str, to: &str) -> Option<Vec<&'a Model>> {
    let find = |f: &str, t: &str| table.models.iter().find(|m| m.from == f && m.to == t);
    if from == to {
        return Some(Vec::new());
    }
    if let Some(m) = find(from, to) {
        return Some(vec![m]);
    }
    if from != "en" && to != "en" {
        return Some(vec![find(from, "en")?, find("en", to)?]);
    }
    None
}

pub struct TranslateState {
    dir: PathBuf,
    http: Client,
    /// Un seul téléchargement de modèles à la fois.
    busy: tokio::sync::Mutex<()>,
}

impl TranslateState {
    pub fn new(dir: PathBuf) -> Self {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .redirect(redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("client HTTP");
        Self { dir, http, busy: tokio::sync::Mutex::new(()) }
    }

    fn file_path(&self, model: &Model, kind: &str) -> PathBuf {
        self.dir.join(model.dir_name()).join(kind)
    }

    /// Le fichier est là, de la bonne taille (son empreinte a été vérifiée au téléchargement).
    fn present(&self, model: &Model, kind: &str, file: &ModelFile) -> bool {
        fs::metadata(self.file_path(model, kind)).is_ok_and(|m| m.is_file() && m.len() == file.size)
    }

    fn missing(&self, model: &Model) -> u64 {
        model.files.iter().filter(|(k, f)| !self.present(model, k, f)).map(|(_, f)| f.size).sum()
    }

    pub fn plan(&self, table: &Table, from: &str, to: &str) -> AppResult<TranslationPlan> {
        let models = route(table, from, to).ok_or_else(|| AppError::TranslationUnsupported(from.to_string()))?;
        let missing = models.iter().map(|m| self.missing(m)).sum();
        let steps = models
            .iter()
            .map(|m| Step { from: m.from.clone(), to: m.to.clone(), version: m.version.clone(), files: m.files.keys().cloned().collect() })
            .collect();
        Ok(TranslationPlan { steps, missing })
    }

    /// Télécharge les fichiers manquants, chacun vérifié par son empreinte.
    pub async fn download(&self, table: &Table, from: &str, to: &str, emit: &(dyn Fn(InstallEvent) + Send + Sync)) -> AppResult<()> {
        let _guard = self.busy.lock().await;
        let models = route(table, from, to).ok_or_else(|| AppError::TranslationUnsupported(from.to_string()))?;
        let total: u64 = models.iter().map(|m| self.missing(m)).sum();
        let mut received = 0u64;
        emit(InstallEvent::Downloading { received, total });
        for model in models {
            for (kind, file) in &model.files {
                if self.present(model, kind, file) {
                    continue;
                }
                let base = received;
                let data = self
                    .fetch(&table.source, file, &mut |n| emit(InstallEvent::Downloading { received: base + n, total }))
                    .await?;
                let path = self.file_path(model, kind);
                let parent = path.parent().expect("dossier du modèle");
                fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
                let tmp = path.with_extension("part");
                let mut out = fs::File::create(&tmp).map_err(|e| AppError::Io(e.to_string()))?;
                out.write_all(&data).map_err(|e| AppError::Io(e.to_string()))?;
                drop(out);
                fs::rename(&tmp, &path).map_err(|e| AppError::Io(e.to_string()))?;
                received += file.size;
            }
        }
        emit(InstallEvent::Finishing);
        Ok(())
    }

    async fn fetch(&self, source: &str, file: &ModelFile, progress: &mut (dyn FnMut(u64) + Send)) -> AppResult<Vec<u8>> {
        let fail = |detail: String| AppError::TranslationUnavailable(detail);
        if !safe_name(&file.file) {
            return Err(fail("nom de fichier".into()));
        }
        let url = reqwest::Url::parse(&format!("{source}{}", file.file)).map_err(|e| fail(e.to_string()))?;
        if url.scheme() != "https" || url.host_str() != Some(HOST) {
            // Tests : faux serveur local ; sinon, seul le CDN de Mozilla.
            if !cfg!(test) {
                return Err(fail(url.host_str().unwrap_or("?").to_string()));
            }
        }
        let mut response = self.http.get(url).send().await.map_err(|e| fail(e.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(fail(format!("HTTP {}", response.status().as_u16())));
        }
        let mut data = Vec::with_capacity(file.size as usize);
        while let Some(chunk) = response.chunk().await.map_err(|e| fail(e.without_url().to_string()))? {
            data.extend_from_slice(&chunk);
            if data.len() as u64 > file.size {
                return Err(fail(crate::tr!("fichier plus gros que prévu", "file larger than expected")));
            }
            progress(data.len() as u64);
        }
        let sum: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
        if data.len() as u64 != file.size || sum != file.sha256 {
            return Err(fail(crate::tr!("fichier abîmé (empreinte différente)", "damaged file (checksum mismatch)")));
        }
        Ok(data)
    }

    /// Un fichier d'un modèle téléchargé, pour le moteur de l'interface.
    pub fn read(&self, table: &Table, from: &str, to: &str, kind: &str) -> AppResult<Vec<u8>> {
        let model = table.models.iter().find(|m| m.from == from && m.to == to).ok_or(AppError::NotFound)?;
        let file = model.files.get(kind).ok_or(AppError::NotFound)?;
        if !self.present(model, kind, file) {
            return Err(AppError::NotFound);
        }
        fs::read(self.file_path(model, kind)).map_err(|e| AppError::Io(e.to_string()))
    }

    /// Place prise par les modèles téléchargés.
    pub fn storage(&self) -> u64 {
        fn walk(p: &Path) -> u64 {
            let Ok(entries) = fs::read_dir(p) else { return 0 };
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => walk(&e.path()),
                    Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
                    _ => 0,
                })
                .sum()
        }
        walk(&self.dir)
    }

    pub fn clear(&self) -> AppResult<()> {
        match fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::Io(e.to_string())),
        }
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

/// Langue d'un texte (codes des modèles : `en`, `fr`, `zh-Hans`…), quand elle
/// est sûre et qu'un modèle sait la traduire.
pub fn detect(text: &str) -> Option<String> {
    use whatlang::Lang::*;
    let info = whatlang::detect(text)?;
    if !info.is_reliable() {
        return None;
    }
    let code = match info.lang() {
        Eng => "en",
        Fra => "fr",
        Deu => "de",
        Spa => "es",
        Ita => "it",
        Por => "pt",
        Rus => "ru",
        Pol => "pl",
        Ukr => "uk",
        Ces => "cs",
        Nld => "nl",
        Swe => "sv",
        Tur => "tr",
        Cmn => "zh-Hans",
        Jpn => "ja",
        Kor => "ko",
        Hun => "hu",
        Ron => "ro",
        Dan => "da",
        Fin => "fi",
        Ell => "el",
        Bul => "bg",
        Vie => "vi",
        Ind => "id",
        Ara => "ar",
        Heb => "he",
        Hin => "hi",
        Ben => "bn",
        Pes => "fa",
        Tha => "th",
        Cat => "ca",
        Est => "et",
        Lit => "lt",
        Lav => "lv",
        Slk => "sk",
        Slv => "sl",
        Hrv => "hr",
        Srp => "sr",
        Bel => "be",
        Aze => "az",
        Afr => "af",
        Tgl => "tl",
        Urd => "ur",
        Mar => "mr",
        Tam => "ta",
        Tel => "te",
        Kan => "kn",
        Mal => "ml",
        Guj => "gu",
        Nob => "nb",
        _ => return None,
    };
    let known = code == "en" || table().models.iter().any(|m| m.from == code);
    known.then(|| code.to_string())
}

// ---------------------------------------------------------------- Commandes

#[tauri::command]
pub fn translation_detect(text: String) -> Option<String> {
    // Un extrait suffit (et reste rapide).
    let sample: String = text.chars().take(3000).collect();
    detect(&sample)
}

#[tauri::command]
pub fn translation_plan(state: State<'_, AppState>, from: String, to: String) -> AppResult<TranslationPlan> {
    state.translate.plan(table(), &from, &to)
}

#[tauri::command]
pub async fn translation_download(state: State<'_, AppState>, from: String, to: String, on_event: Channel<InstallEvent>) -> AppResult<()> {
    let emit = move |e: InstallEvent| {
        let _ = on_event.send(e);
    };
    state.translate.download(table(), &from, &to, &emit).await
}

/// Un fichier de modèle, en binaire (sans passer par JSON).
#[tauri::command]
pub fn translation_file(state: State<'_, AppState>, from: String, to: String, kind: String) -> AppResult<Response> {
    state.translate.read(table(), &from, &to, &kind).map(Response::new)
}

#[tauri::command]
pub fn translation_storage(state: State<'_, AppState>) -> u64 {
    state.translate.storage()
}

#[tauri::command]
pub fn translation_clear(state: State<'_, AppState>) -> AppResult<()> {
    state.translate.clear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;
    use crate::testutil::fake_server_with;

    #[test]
    fn the_pinned_table_reaches_french_and_english_from_every_language() {
        let t = table();
        assert!(t.source.starts_with(&format!("https://{HOST}/")));
        for m in &t.models {
            assert!(m.files.contains_key("model") && m.files.contains_key("lex"), "{}-{}", m.from, m.to);
            assert!(m.files.contains_key("vocab") || (m.files.contains_key("srcvocab") && m.files.contains_key("trgvocab")));
            assert!(m.files.values().all(|f| safe_name(&f.file) && f.sha256.len() == 64 && f.size > 0));
        }
        assert_eq!(route(t, "en", "fr").unwrap().len(), 1);
        assert_eq!(route(t, "fr", "en").unwrap().len(), 1);
        let zh: Vec<_> = route(t, "zh-Hans", "fr").unwrap().iter().map(|m| format!("{}-{}", m.from, m.to)).collect();
        assert_eq!(zh, ["zh-Hans-en", "en-fr"]);
        assert!(route(t, "fr", "fr").unwrap().is_empty());
        assert!(route(t, "xx", "fr").is_none());
    }

    #[test]
    fn languages_are_detected_only_when_sure() {
        assert_eq!(detect("This mod adds more players to the lobby and new cosmetics for everyone.").as_deref(), Some("en"));
        assert_eq!(detect("Ce mod ajoute plus de joueurs dans le salon et de nouveaux cosmétiques pour tout le monde.").as_deref(), Some("fr"));
        assert_eq!(detect("Dieser Mod fügt der Lobby mehr Spieler und neue Kosmetik für alle hinzu.").as_deref(), Some("de"));
        assert_eq!(detect("这个模组为大厅增加了更多的玩家，并为所有人提供了新的装饰。").as_deref(), Some("zh-Hans"));
        assert_eq!(detect("ok"), None);
    }

    fn small_table(base: &str, body: &[u8]) -> Table {
        let sum: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
        let file = |name: &str| ModelFile { file: name.into(), size: body.len() as u64, sha256: sum.clone() };
        let files = ["lex", "model", "vocab"].iter().map(|k| (k.to_string(), file(&format!("{k}.bin")))).collect();
        Table { source: format!("{base}/"), models: vec![Model { from: "en".into(), to: "fr".into(), version: "2.0".into(), files }] }
    }

    #[test]
    fn models_are_downloaded_once_and_checked() {
        let body = b"modele".to_vec();
        let ok = |b: &[u8]| {
            let mut s = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len());
            s.extend(b.iter().map(|&c| c as char));
            s
        };
        let bad = ok(b"abime!");
        let server = fake_server_with(|_| vec![ok(&body), ok(&body), bad.clone(), ok(&body)]);
        let table = small_table(server.base.trim_end_matches("/v1"), &body);
        let state = TranslateState::new(tempdir("translate-models"));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert_eq!(state.plan(&table, "en", "fr").unwrap().missing, 18);
        // Le troisième fichier est abîmé : refusé, les deux premiers sont gardés.
        let err = rt.block_on(state.download(&table, "en", "fr", &|_| {})).unwrap_err();
        assert!(matches!(err, AppError::TranslationUnavailable(_)), "{err:?}");
        assert_eq!(state.plan(&table, "en", "fr").unwrap().missing, 6);
        rt.block_on(state.download(&table, "en", "fr", &|_| {})).unwrap();
        assert_eq!(state.plan(&table, "en", "fr").unwrap().missing, 0);
        assert_eq!(state.read(&table, "en", "fr", "model").unwrap(), body);
        assert!(state.read(&table, "en", "fr", "../x").is_err());
        assert_eq!(state.storage(), 18);
        state.clear().unwrap();
        assert_eq!(state.plan(&table, "en", "fr").unwrap().missing, 18);
    }
}
