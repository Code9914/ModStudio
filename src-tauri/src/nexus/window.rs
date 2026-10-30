//! Téléchargements sans compte Premium : la page du fichier s'affiche dans
//! ModStudio, l'utilisateur clique sur « Slow download » comme sur le site,
//! et le lien `nxm://` produit par la page est capté ici au lieu d'ouvrir un
//! autre gestionnaire de mods.
//!
//! La page s'affiche dans une vue intégrée à la fenêtre principale (au-dessus
//! de la zone de contenu, aux coordonnées données par l'interface), ou, si ce
//! n'est pas possible, dans une fenêtre séparée. Dans les deux cas, la page
//! ne peut appeler aucune commande de l'appli : Tauri refuse les appels
//! venant d'un site distant. Seul le lien `nxm://` remonte, sous forme
//! d'évènement.

use serde::{Deserialize, Serialize};
use tauri::webview::WebviewBuilder;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use super::browse::check_domain;
use crate::error::{AppError, AppResult};

/// Fenêtre séparée (solution de repli).
pub const NEXUS_WINDOW: &str = "nexus";
/// Vue intégrée à la fenêtre principale.
pub const NEXUS_EMBED: &str = "nexus-embed";
/// Évènement envoyé à la fenêtre principale quand un lien `nxm://` est capté.
pub const NXM_EVENT: &str = "nexus-nxm";
/// Évènement envoyé quand la page Nexus se ferme d'elle-même (lien capté).
pub const SITE_CLOSED_EVENT: &str = "nexus-site-closed";

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

/// Zone de la fenêtre principale où afficher la page (pixels CSS).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    /// Valeurs finies et raisonnables, sinon `None`.
    fn sane(self) -> Option<Bounds> {
        let ok = [self.x, self.y, self.width, self.height].iter().all(|v| v.is_finite())
            && self.x >= 0.0
            && self.y >= 0.0
            && self.width >= 200.0
            && self.height >= 150.0
            && self.width <= 20_000.0
            && self.height <= 20_000.0;
        ok.then_some(self)
    }
}

/// Où la page s'est ouverte.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Embedded,
    Window,
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

/// Seules les pages web s'ouvrent (le site, sa connexion…).
fn allowed_navigation(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "about" | "blob" | "data")
}

/// Page de connexion / clés API du site, pour la première connexion : la
/// session ouverte dans ModStudio sert ensuite à chaque téléchargement.
pub const LOGIN_PAGE: &str = "https://www.nexusmods.com/settings/api-keys";

/// Page d'un fichier à télécharger.
pub fn file_page(domain: &str, mod_id: i64, file_id: i64) -> AppResult<String> {
    check_domain(domain)?;
    if mod_id <= 0 || file_id <= 0 {
        return Err(AppError::InvalidArgument("fichier inconnu".into()));
    }
    Ok(download_page(domain, mod_id, file_id))
}

/// N'accepte que des pages du site Nexus Mods, en HTTPS.
fn nexus_page(url: &str) -> Option<Url> {
    let page = Url::parse(url.trim()).ok()?;
    let host = page.host_str()?.to_ascii_lowercase();
    (page.scheme() == "https" && (host == "nexusmods.com" || host.ends_with(".nexusmods.com"))).then_some(page)
}

/// Navigation dans la page Nexus : capte les liens `nxm://`, n'autorise que le web.
fn navigation_handler(handle: AppHandle) -> impl Fn(&Url) -> bool + Send + 'static {
    move |url: &Url| {
        if url.scheme() == "nxm" {
            if let Some(link) = parse_nxm(url) {
                let _ = handle.emit_to("main", NXM_EVENT, link);
                // Le lien est capté : la page a fini son travail.
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    close_all(&h);
                    let _ = h.emit_to("main", SITE_CLOSED_EVENT, ());
                    if let Some(main) = h.get_webview_window("main") {
                        let _ = main.set_focus();
                    }
                });
            }
            return false;
        }
        allowed_navigation(url)
    }
}

/// Affiche une page Nexus : dans la fenêtre principale si `bounds` est donné
/// et que c'est possible, sinon dans une fenêtre séparée.
pub fn show(app: &AppHandle, url: &str, title: &str, bounds: Option<Bounds>) -> AppResult<Placement> {
    let page = nexus_page(url)
        .ok_or_else(|| AppError::InvalidArgument("seules les pages de Nexus Mods peuvent s'ouvrir ici.".into()))?;
    if let Some(b) = bounds.and_then(Bounds::sane) {
        if embed(app, page.clone(), b).is_ok() {
            // Une éventuelle fenêtre séparée restée ouverte n'a plus lieu d'être.
            if let Some(w) = app.get_webview_window(NEXUS_WINDOW) {
                let _ = w.close();
            }
            return Ok(Placement::Embedded);
        }
    }
    open_window(app, page, title)?;
    Ok(Placement::Window)
}

fn embed(app: &AppHandle, page: Url, b: Bounds) -> AppResult<()> {
    let io = |e: tauri::Error| AppError::Io(e.to_string());
    if let Some(view) = app.get_webview(NEXUS_EMBED) {
        view.navigate(page).map_err(io)?;
        set_view_bounds(&view, b)?;
        let _ = view.show();
        let _ = view.set_focus();
        return Ok(());
    }
    let main = app
        .get_window("main")
        .ok_or_else(|| AppError::Io("fenêtre principale introuvable".into()))?;
    let builder = WebviewBuilder::new(NEXUS_EMBED, WebviewUrl::External(page)).on_navigation(navigation_handler(app.clone()));
    let view = main
        .add_child(builder, LogicalPosition::new(b.x, b.y), LogicalSize::new(b.width, b.height))
        .map_err(io)?;
    let _ = view.set_focus();
    Ok(())
}

fn set_view_bounds(view: &tauri::Webview, b: Bounds) -> AppResult<()> {
    let io = |e: tauri::Error| AppError::Io(e.to_string());
    view.set_position(LogicalPosition::new(b.x, b.y)).map_err(io)?;
    view.set_size(LogicalSize::new(b.width, b.height)).map_err(io)
}

/// Suit la zone de contenu quand la fenêtre change de taille.
pub fn set_bounds(app: &AppHandle, bounds: Bounds) -> AppResult<()> {
    match (app.get_webview(NEXUS_EMBED), bounds.sane()) {
        (Some(view), Some(b)) => set_view_bounds(&view, b),
        _ => Ok(()),
    }
}

/// Ferme la page Nexus, intégrée ou séparée.
pub fn close_all(app: &AppHandle) {
    if let Some(view) = app.get_webview(NEXUS_EMBED) {
        let _ = view.close();
    }
    if let Some(w) = app.get_webview_window(NEXUS_WINDOW) {
        let _ = w.close();
    }
}

fn open_window(app: &AppHandle, page: Url, title: &str) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(NEXUS_WINDOW) {
        window.navigate(page).map_err(|e| AppError::Io(e.to_string()))?;
        let _ = window.set_title(title);
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, NEXUS_WINDOW, WebviewUrl::External(page))
        .title(title)
        .inner_size(1200.0, 860.0)
        .min_inner_size(800.0, 600.0)
        .on_navigation(navigation_handler(app.clone()))
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

    #[test]
    fn bounds_must_be_sane() {
        let ok = Bounds { x: 216.0, y: 56.0, width: 1064.0, height: 800.0 };
        assert_eq!(ok.sane(), Some(ok));
        assert!(Bounds { width: f64::NAN, ..ok }.sane().is_none());
        assert!(Bounds { x: -5.0, ..ok }.sane().is_none());
        assert!(Bounds { width: 10.0, ..ok }.sane().is_none());
        assert!(file_page("bonelab", 0, 1).is_err());
        assert!(file_page("Bad Domain", 1, 1).is_err());
    }

    #[test]
    fn the_window_only_opens_nexus_pages() {
        assert!(nexus_page(LOGIN_PAGE).is_some());
        assert!(nexus_page(&download_page("bonelab", 1, 2)).is_some());
        assert!(nexus_page("http://www.nexusmods.com/").is_none());
        assert!(nexus_page("https://nexusmods.com.evil.example/").is_none());
        assert!(nexus_page("https://evil.example/?nexusmods.com").is_none());
    }
}
