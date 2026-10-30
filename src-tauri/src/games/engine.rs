//! Reconnaître le moteur d'un jeu installé (Unreal, Unity, Bethesda) et les
//! outils de mods déjà présents dans son dossier.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    Unreal,
    UnityMono,
    UnityIl2cpp,
    Bethesda,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X64,
    X86,
}

impl Arch {
    pub fn as_str(self) -> &'static str {
        match self {
            Arch::X64 => "x64",
            Arch::X86 => "x86",
        }
    }
}

/// Jeux Bethesda (moteur Creation / Gamebryo) : exécutable, script extender
/// (préfixe de ses fichiers) et format de `plugins.txt`.
pub struct BethesdaGame {
    pub domain: &'static str,
    pub exe: &'static str,
    /// Préfixe des fichiers du script extender (`skse64_loader.exe`…).
    pub extender: &'static str,
    pub extender_name: &'static str,
    /// Page du script extender sur Nexus (jeu, mod).
    pub extender_mod: Option<i64>,
    /// Dossier de `plugins.txt` sous `%LOCALAPPDATA%`.
    pub appdata: &'static str,
    /// `plugins.txt` marque les plugins actifs d'un `*`.
    pub star_format: bool,
}

pub const BETHESDA: &[BethesdaGame] = &[
    BethesdaGame { domain: "skyrimspecialedition", exe: "SkyrimSE.exe", extender: "skse64", extender_name: "SKSE64", extender_mod: Some(30379), appdata: "Skyrim Special Edition", star_format: true },
    BethesdaGame { domain: "fallout4", exe: "Fallout4.exe", extender: "f4se", extender_name: "F4SE", extender_mod: Some(42147), appdata: "Fallout4", star_format: true },
    BethesdaGame { domain: "starfield", exe: "Starfield.exe", extender: "sfse", extender_name: "SFSE", extender_mod: Some(106), appdata: "Starfield", star_format: true },
    BethesdaGame { domain: "newvegas", exe: "FalloutNV.exe", extender: "nvse", extender_name: "xNVSE", extender_mod: Some(67883), appdata: "FalloutNV", star_format: false },
    BethesdaGame { domain: "fallout3", exe: "Fallout3.exe", extender: "fose", extender_name: "FOSE", extender_mod: Some(8606), appdata: "Fallout3", star_format: false },
    BethesdaGame { domain: "oblivion", exe: "Oblivion.exe", extender: "obse", extender_name: "xOBSE", extender_mod: Some(37952), appdata: "Oblivion", star_format: false },
    BethesdaGame { domain: "skyrim", exe: "TESV.exe", extender: "skse", extender_name: "SKSE", extender_mod: None, appdata: "Skyrim", star_format: false },
];

pub fn bethesda(domain: &str) -> Option<&'static BethesdaGame> {
    BETHESDA.iter().find(|g| g.domain == domain)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub engine: Engine,
    /// Dossier du projet Unreal (`Pilgrim`), relatif au dossier du jeu.
    pub project: Option<String>,
    pub arch: Arch,
    /// Version de l'exécutable principal (jeux Bethesda), `1.6.1170.0`.
    pub exe_version: Option<String>,
    /// Outils de mods présents dans le dossier du jeu (`ue4ss`, `bepinex`…).
    pub tools: Vec<String>,
}

/// Dossier `Binaries/Win64` du projet Unreal, relatif au dossier du jeu.
pub fn unreal_win64(project: &str) -> String {
    format!("{project}/Binaries/Win64")
}

/// Dossier des mods UE4SS, relatif au dossier du jeu. Les versions récentes
/// rangent tout dans `ue4ss/`, les anciennes directement dans `Win64`.
pub fn ue4ss_mods_dir(root: &Path, project: &str) -> String {
    let win64 = unreal_win64(project);
    let old_layout = root.join(&win64).join("UE4SS.dll").is_file() && !root.join(&win64).join("ue4ss").is_dir();
    if old_layout { format!("{win64}/Mods") } else { format!("{win64}/ue4ss/Mods") }
}

fn is_dir_ci(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    entries
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name) && e.path().is_dir())
        .map(|e| e.path())
}

/// Projet Unreal : un dossier qui contient `Binaries/Win64` et `Content`.
pub fn unreal_project(root: &Path) -> Option<String> {
    let entries = fs::read_dir(root).ok()?;
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.eq_ignore_ascii_case("Engine"))
        .filter(|name| root.join(name).join("Binaries").join("Win64").is_dir() && root.join(name).join("Content").is_dir())
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Architecture d'un exécutable ou d'une DLL Windows (en-tête PE).
pub fn pe_arch(path: &Path) -> Option<Arch> {
    let mut f = File::open(path).ok()?;
    let mut mz = [0u8; 64];
    f.read_exact(&mut mz).ok()?;
    if &mz[0..2] != b"MZ" {
        return None;
    }
    let pe_offset = u32::from_le_bytes(mz[0x3C..0x40].try_into().ok()?) as u64;
    f.seek(SeekFrom::Start(pe_offset)).ok()?;
    let mut head = [0u8; 6];
    f.read_exact(&mut head).ok()?;
    if &head[0..4] != b"PE\0\0" {
        return None;
    }
    match u16::from_le_bytes([head[4], head[5]]) {
        0x8664 | 0xAA64 => Some(Arch::X64),
        0x014C => Some(Arch::X86),
        _ => None,
    }
}

/// Version d'un exécutable Windows, lue dans sa ressource `VS_FIXEDFILEINFO`.
pub fn exe_version(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;
    const SIG: [u8; 4] = [0xBD, 0x04, 0xEF, 0xFE];
    let at = data.windows(4).position(|w| w == SIG)?;
    let field = |i: usize| -> Option<u32> {
        let start = at + i * 4;
        Some(u32::from_le_bytes(data.get(start..start + 4)?.try_into().ok()?))
    };
    let (ms, ls) = (field(2)?, field(3)?);
    Some(format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF))
}

/// Outils de mods présents dans le dossier du jeu.
pub fn present_tools(root: &Path, project: Option<&str>, domain: &str) -> Vec<String> {
    let mut tools = Vec::new();
    if let Some(p) = project {
        let win64 = root.join(unreal_win64(p));
        if win64.join("ue4ss").join("UE4SS.dll").is_file() || win64.join("UE4SS.dll").is_file() {
            tools.push("ue4ss".to_string());
        }
    }
    let core = root.join("BepInEx").join("core");
    if core.join("BepInEx.dll").is_file() || core.join("BepInEx.Core.dll").is_file() {
        tools.push("bepinex".to_string());
    }
    if root.join("MelonLoader").is_dir() && root.join("version.dll").is_file() {
        tools.push("melonloader".to_string());
    }
    if let Some(g) = bethesda(domain) {
        if root.join(format!("{}_loader.exe", g.extender)).is_file() {
            tools.push(g.extender.to_string());
        }
    }
    tools
}

/// Moteur et outils d'un jeu installé dans `root`.
pub fn detect(root: &Path, domain: &str) -> EngineInfo {
    if let Some(g) = bethesda(domain) {
        let exe = root.join(g.exe);
        return EngineInfo {
            engine: Engine::Bethesda,
            project: None,
            arch: pe_arch(&exe).unwrap_or(Arch::X64),
            exe_version: exe_version(&exe),
            tools: present_tools(root, None, domain),
        };
    }

    // Unity : UnityPlayer.dll à la racine, données dans `<Jeu>_Data`.
    if root.join("UnityPlayer.dll").is_file() {
        let data_dir = fs::read_dir(root)
            .ok()
            .and_then(|it| {
                it.flatten()
                    .find(|e| e.path().is_dir() && e.file_name().to_string_lossy().to_ascii_lowercase().ends_with("_data"))
            })
            .map(|e| e.path());
        let mono = data_dir
            .as_ref()
            .is_some_and(|d| is_dir_ci(d, "Managed").is_some_and(|m| m.join("Assembly-CSharp.dll").is_file()));
        let engine = if root.join("GameAssembly.dll").is_file() && !mono {
            Engine::UnityIl2cpp
        } else {
            Engine::UnityMono
        };
        return EngineInfo {
            engine,
            project: None,
            arch: pe_arch(&root.join("UnityPlayer.dll")).unwrap_or(Arch::X64),
            exe_version: None,
            tools: present_tools(root, None, domain),
        };
    }

    if let Some(project) = unreal_project(root) {
        return EngineInfo {
            engine: Engine::Unreal,
            tools: present_tools(root, Some(&project), domain),
            project: Some(project),
            arch: Arch::X64,
            exe_version: None,
        };
    }

    EngineInfo {
        engine: Engine::Unknown,
        project: None,
        arch: Arch::X64,
        exe_version: None,
        tools: present_tools(root, None, domain),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::installer::tests::tempdir;

    /// Petit fichier PE : en-tête MZ, signature PE et type de machine.
    pub fn fake_pe(machine: u16, version: Option<(u16, u16, u16, u16)>) -> Vec<u8> {
        let mut v = vec![0u8; 0x100];
        v[0] = b'M';
        v[1] = b'Z';
        v[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        v[0x80..0x84].copy_from_slice(b"PE\0\0");
        v[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        if let Some((a, b, c, d)) = version {
            v.extend_from_slice(&[0xBD, 0x04, 0xEF, 0xFE]);
            v.extend_from_slice(&0x0001_0000u32.to_le_bytes());
            v.extend_from_slice(&(((a as u32) << 16) | b as u32).to_le_bytes());
            v.extend_from_slice(&(((c as u32) << 16) | d as u32).to_le_bytes());
        }
        v
    }

    fn touch(root: &Path, rel: &str, data: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }

    #[test]
    fn recognizes_unreal_games() {
        let root = tempdir("ue");
        touch(&root, "Engine/Binaries/ThirdParty/x.dll", b"");
        touch(&root, "Pilgrim/Binaries/Win64/Pilgrim-Win64-Shipping.exe", b"");
        touch(&root, "Pilgrim/Content/Paks/Pilgrim-Windows.pak", b"");
        touch(&root, "Pilgrim.exe", b"");
        let info = detect(&root, "pilgrim");
        assert_eq!(info.engine, Engine::Unreal);
        assert_eq!(info.project.as_deref(), Some("Pilgrim"));
        assert!(info.tools.is_empty());
        assert_eq!(ue4ss_mods_dir(&root, "Pilgrim"), "Pilgrim/Binaries/Win64/ue4ss/Mods");

        // UE4SS ancien format (tout dans Win64).
        touch(&root, "Pilgrim/Binaries/Win64/UE4SS.dll", b"");
        assert_eq!(detect(&root, "pilgrim").tools, vec!["ue4ss"]);
        assert_eq!(ue4ss_mods_dir(&root, "Pilgrim"), "Pilgrim/Binaries/Win64/Mods");
    }

    #[test]
    fn recognizes_unity_games() {
        let root = tempdir("unity");
        touch(&root, "UnityPlayer.dll", &fake_pe(0x8664, None));
        touch(&root, "Game_Data/Managed/Assembly-CSharp.dll", b"");
        let info = detect(&root, "somegame");
        assert_eq!((info.engine, info.arch), (Engine::UnityMono, Arch::X64));

        let il2cpp = tempdir("il2cpp");
        touch(&il2cpp, "UnityPlayer.dll", &fake_pe(0x014C, None));
        touch(&il2cpp, "GameAssembly.dll", b"");
        touch(&il2cpp, "Game_Data/il2cpp_data/Metadata/global-metadata.dat", b"");
        touch(&il2cpp, "MelonLoader/net6/MelonLoader.dll", b"");
        touch(&il2cpp, "version.dll", b"");
        let info = detect(&il2cpp, "othergame");
        assert_eq!((info.engine, info.arch), (Engine::UnityIl2cpp, Arch::X86));
        assert_eq!(info.tools, vec!["melonloader"]);
    }

    #[test]
    fn recognizes_bethesda_games_and_reads_exe_versions() {
        let root = tempdir("sse");
        touch(&root, "SkyrimSE.exe", &fake_pe(0x8664, Some((1, 6, 1170, 0))));
        touch(&root, "Data/Skyrim.esm", b"");
        let info = detect(&root, "skyrimspecialedition");
        assert_eq!(info.engine, Engine::Bethesda);
        assert_eq!(info.exe_version.as_deref(), Some("1.6.1170.0"));
        assert!(info.tools.is_empty());
        touch(&root, "skse64_loader.exe", b"");
        assert_eq!(detect(&root, "skyrimspecialedition").tools, vec!["skse64"]);
        assert!(bethesda("pilgrim").is_none());
    }

    #[test]
    fn unknown_folders_stay_unknown() {
        let root = tempdir("unknown");
        touch(&root, "game.exe", b"");
        assert_eq!(detect(&root, "x").engine, Engine::Unknown);
        assert!(pe_arch(&root.join("game.exe")).is_none());
    }
}
