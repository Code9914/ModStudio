//! Jeux installés avec Steam : bibliothèques (`libraryfolders.vdf`) et
//! manifestes de jeux (`appmanifest_<id>.acf`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{InstalledGame, Launcher};

/// Lecture minimale du format texte « KeyValues » de Valve : renvoie les
/// paires clé → valeur du premier niveau sous la racine (`"AppState" { … }`).
pub fn top_level_values(vdf: &str) -> HashMap<String, String> {
    let tokens = tokenize(vdf);
    let mut out = HashMap::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Open => depth += 1,
            Token::Close => depth = depth.saturating_sub(1),
            Token::Text(key) => {
                if let Some(Token::Text(value)) = tokens.get(i + 1) {
                    if depth == 1 {
                        out.entry(key.to_ascii_lowercase()).or_insert_with(|| value.clone());
                    }
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

#[derive(Debug, PartialEq)]
enum Token {
    Text(String),
    Open,
    Close,
}

fn tokenize(s: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '"' => {
                let mut text = String::new();
                while let Some(ch) = chars.next() {
                    match ch {
                        '\\' => match chars.next() {
                            Some('n') => text.push('\n'),
                            Some('t') => text.push('\t'),
                            Some(other) => text.push(other),
                            None => {}
                        },
                        '"' => break,
                        other => text.push(other),
                    }
                }
                out.push(Token::Text(text));
            }
            '/' if chars.peek() == Some(&'/') => {
                // Commentaire jusqu'à la fin de la ligne.
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Dossiers de bibliothèques Steam déclarés dans `libraryfolders.vdf`.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    let tokens = tokenize(vdf);
    tokens
        .windows(2)
        .filter_map(|w| match (&w[0], &w[1]) {
            (Token::Text(k), Token::Text(v)) if k.eq_ignore_ascii_case("path") => Some(PathBuf::from(v)),
            _ => None,
        })
        .collect()
}

/// Dossiers où Steam peut être installé.
pub fn steam_roots() -> Vec<PathBuf> {
    let mut roots = registry_roots();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(p) = std::env::var(var) {
            roots.push(PathBuf::from(p).join("Steam"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(&home).join(".steam/steam"));
        roots.push(PathBuf::from(&home).join(".local/share/Steam"));
    }
    roots
}

/// Valeur `REG_SZ` dans la sortie de `reg query`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn reg_value(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut parts = line.trim().splitn(3, "    ");
        let (n, kind, value) = (parts.next()?, parts.next()?, parts.next()?);
        (n.eq_ignore_ascii_case(name) && kind.trim() == "REG_SZ").then(|| value.trim().to_string())
    })
}

/// Dossier de Steam enregistré dans le registre Windows (Steam installé ailleurs que dans Program Files).
#[cfg(windows)]
fn registry_roots() -> Vec<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let query = |key: &str, name: &str| -> Option<PathBuf> {
        let out = std::process::Command::new("reg")
            .args(["query", key, "/v", name])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        reg_value(&String::from_utf8_lossy(&out.stdout), name).map(PathBuf::from)
    };
    [
        query(r"HKCU\Software\Valve\Steam", "SteamPath"),
        query(r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(not(windows))]
fn registry_roots() -> Vec<PathBuf> {
    Vec::new()
}

/// Toutes les bibliothèques Steam (sans doublons), y compris le dossier de Steam.
pub fn libraries(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        let key = p.to_string_lossy().to_ascii_lowercase().replace('\\', "/").trim_end_matches('/').to_string();
        if p.join("steamapps").is_dir() && !libs.iter().any(|l| l.to_string_lossy().to_ascii_lowercase().replace('\\', "/").trim_end_matches('/') == key) {
            libs.push(p);
        }
    };
    for root in roots {
        push(root.clone());
        if let Ok(vdf) = fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
            for lib in parse_library_folders(&vdf) {
                push(lib);
            }
        }
    }
    libs
}

/// Jeux complètement installés dans une bibliothèque Steam.
pub fn games_in_library(library: &Path) -> Vec<InstalledGame> {
    let steamapps = library.join("steamapps");
    let Ok(entries) = fs::read_dir(&steamapps) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
            continue;
        }
        let Ok(raw) = fs::read_to_string(entry.path()) else { continue };
        let values = top_level_values(&raw);
        let (Some(app_id), Some(title), Some(dir)) = (values.get("appid"), values.get("name"), values.get("installdir")) else {
            continue;
        };
        // StateFlags 4 : entièrement installé (les autres états : en cours, à mettre à jour…).
        let flags: u32 = values.get("stateflags").and_then(|f| f.parse().ok()).unwrap_or(4);
        if flags & 4 == 0 {
            continue;
        }
        let path = steamapps.join("common").join(dir);
        if !path.is_dir() {
            continue;
        }
        out.push(InstalledGame {
            launcher: Launcher::Steam,
            app_id: app_id.clone(),
            name: title.clone(),
            path,
        });
    }
    out
}

/// Jeux Steam installés sur ce PC.
pub fn installed_games(roots: &[PathBuf]) -> Vec<InstalledGame> {
    libraries(roots).iter().flat_map(|lib| games_in_library(lib)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_steam_folder_from_the_registry() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    d:/jeux/steam\r\n\r\n";
        assert_eq!(reg_value(out, "SteamPath").as_deref(), Some("d:/jeux/steam"));
        assert_eq!(reg_value(out, "InstallPath"), None);
    }
    use crate::installer::tests::tempdir;

    const MANIFEST: &str = r#""AppState"
{
	"appid"		"1592190"
	"Universe"		"1"
	"name"		"BONELAB"
	"StateFlags"		"4"
	"installdir"		"BONELAB"
	"UserConfig"
	{
		"language"		"french"
		"name"		"pas celui-ci"
	}
}"#;

    #[test]
    fn reads_app_manifests() {
        let v = top_level_values(MANIFEST);
        assert_eq!(v["appid"], "1592190");
        assert_eq!(v["name"], "BONELAB", "la valeur du sous-bloc est ignorée");
        assert_eq!(v["installdir"], "BONELAB");
    }

    #[test]
    fn reads_library_folders() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t\t\"apps\" { \"1592190\" \"20000\" }\n\t}\n}";
        assert_eq!(
            parse_library_folders(vdf),
            vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]
        );
    }

    #[test]
    fn finds_installed_games() {
        let lib = tempdir("steamlib");
        let apps = lib.join("steamapps");
        fs::create_dir_all(apps.join("common/BONELAB")).unwrap();
        fs::write(apps.join("appmanifest_1592190.acf"), MANIFEST).unwrap();
        // Jeu en cours de téléchargement : ignoré.
        fs::create_dir_all(apps.join("common/Pending")).unwrap();
        fs::write(
            apps.join("appmanifest_5.acf"),
            "\"AppState\" { \"appid\" \"5\" \"name\" \"Pending\" \"StateFlags\" \"1026\" \"installdir\" \"Pending\" }",
        )
        .unwrap();
        // Manifeste dont le dossier n'existe plus : ignoré.
        fs::write(
            apps.join("appmanifest_6.acf"),
            "\"AppState\" { \"appid\" \"6\" \"name\" \"Gone\" \"StateFlags\" \"4\" \"installdir\" \"Gone\" }",
        )
        .unwrap();
        let games = installed_games(&[lib.clone()]);
        assert_eq!(games.len(), 1);
        assert_eq!((games[0].app_id.as_str(), games[0].name.as_str()), ("1592190", "BONELAB"));
        assert_eq!(games[0].path, apps.join("common/BONELAB"));
    }
}
