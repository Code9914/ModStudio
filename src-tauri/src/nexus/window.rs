//! Téléchargements sans compte Premium : la page du fichier s'ouvre dans une
//! fenêtre de ModStudio, l'utilisateur clique sur « Slow download » comme sur
//! le site, et le lien `nxm://` produit par la page est capté ici au lieu
//! d'ouvrir un autre gestionnaire de mods.
//!
//! La fenêtre n'a aucune capacité Tauri : la page ne peut appeler aucune
//! commande de l'appli. Seul le lien `nxm://` remonte, sous forme d'évènement.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use super::browse::check_domain;
use crate::error::{AppError, AppResult};

pub const NEXUS_WINDOW: &str = "nexus";
/// Évènement envoyé à la fenêtre principale quand un lien `nxm://` est capté.
pub const NXM_EVENT: &str = "nexus-nxm";

/// Contenu d'un lien `nxm://bonelab/mods/148/files/316?key=…&expires=…`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NxmLink {
    pub domain: String,
    pub mod_id: i64,
    pub file_id: i64,
    pub key: String,
    pub expires: i64,
}

pub fn parse_nxm(url: &Url) -> Option<NxmLink> {
    if url.scheme() != "nxm" {
        return None;
    }
    let domain = url.host_str()?.to_ascii_lowercase();
    check_domain(&domain).ok()?;
    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let [mods, mod_id, files, file_id] = segments.as_slice() else { return None };
    if *mods != "mods" || *files != "files" {
        return None;
    }
    let mod_id = mod_id.parse::<i64>().ok().filter(|n| *n > 0)?;
    let file_id = file_id.parse::<i64>().ok().filter(|n| *n > 0)?;
    let mut key = None;
    let mut expires = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "key" => key = Some(v.into_owned()),
            "expires" => expires = v.parse::<i64>().ok(),
            _ => {}
        }
    }
    let key = key.filter(|k| !k.is_empty() && k.len() <= 256 && k.chars().all(|c| c.is_ascii_alphanumeric() || "-_=+/".contains(c)))?;
    Some(NxmLink {
        domain,
        mod_id,
        file_id,
        key,
        expires: expires.filter(|e| *e > 0)?,
    })
}

/// Page d'un fichier, réglée pour produire un lien `nxm://` (`nmm=1`).
pub fn download_page(domain: &str, mod_id: i64, file_id: i64) -> String {
    format!("https://www.nexusmods.com/{domain}/mods/{mod_id}?tab=files&file_id={file_id}&nmm=1")
}

/// Seules les pages web s'ouvrent dans la fenêtre (le site, sa connexion…).
fn allowed_navigation(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "about" | "blob" | "data")
}

pub fn open(app: &AppHandle, domain: &str, mod_id: i64, file_id: i64) -> AppResult<()> {
    check_domain(domain)?;
    if mod_id <= 0 || file_id <= 0 {
        return Err(AppError::InvalidArgument("fichier inconnu".into()));
    }
    let page = Url::parse(&download_page(domain, mod_id, file_id)).map_err(|e| AppError::InvalidArgument(e.to_string()))?;

    if let Some(window) = app.get_webview_window(NEXUS_WINDOW) {
        window.navigate(page).map_err(|e| AppError::Io(e.to_string()))?;
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }

    let handle = app.clone();
    WebviewWindowBuilder::new(app, NEXUS_WINDOW, WebviewUrl::External(page))
        .title("Télécharger sur Nexus Mods")
        .inner_size(1200.0, 860.0)
        .min_inner_size(800.0, 600.0)
        .on_navigation(move |url| {
            if url.scheme() == "nxm" {
                if let Some(link) = parse_nxm(url) {
                    let _ = handle.emit_to("main", NXM_EVENT, link);
                    // Le lien est capté : la fenêtre a fini son travail.
                    let h = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(w) = h.get_webview_window(NEXUS_WINDOW) {
                            let _ = w.close();
                        }
                        if let Some(main) = h.get_webview_window("main") {
                            let _ = main.set_focus();
                        }
                    });
                }
                return false;
            }
            allowed_navigation(url)
        })
        .build()
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn parses_nxm_links() {
        let link = parse_nxm(&url("nxm://bonelab/mods/148/files/316?key=AbC-123_x&expires=1793300000&user_id=7")).unwrap();
        assert_eq!(
            link,
            NxmLink {
                domain: "bonelab".into(),
                mod_id: 148,
                file_id: 316,
                key: "AbC-123_x".into(),
                expires: 1793300000
            }
        );
        assert!(parse_nxm(&url("nxm://bonelab/mods/148/files/316?expires=1")).is_none(), "sans clé");
        assert!(parse_nxm(&url("nxm://bonelab/mods/x/files/316?key=a&expires=1")).is_none());
        assert!(parse_nxm(&url("nxm://bonelab/collections/abc/revisions/3")).is_none());
        assert!(parse_nxm(&url("https://bonelab/mods/1/files/2?key=a&expires=1")).is_none());
        assert!(parse_nxm(&url("nxm://bonelab/mods/1/files/2?key=a%20b&expires=1")).is_none());
    }

    #[test]
    fn builds_the_file_page_and_limits_navigation() {
        assert_eq!(
            download_page("bonelab", 148, 316),
            "https://www.nexusmods.com/bonelab/mods/148?tab=files&file_id=316&nmm=1"
        );
        assert!(allowed_navigation(&url("https://users.nexusmods.com/auth/sign_in")));
        assert!(!allowed_navigation(&url("http://www.nexusmods.com/")));
        assert!(!allowed_navigation(&url("file:///C:/Windows")));
        assert!(!allowed_navigation(&url("ms-settings:privacy")));
    }
}
