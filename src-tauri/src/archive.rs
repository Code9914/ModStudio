//! Extraction des archives téléchargées sur Nexus Mods (zip et 7z), avec les
//! mêmes garde-fous que pour mod.io : pas de chemin qui sorte du dossier, pas
//! d'archive démesurée, annulation possible.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{AppError, AppResult};
use crate::installer::{self, io_err};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    SevenZip,
    Rar,
    Unknown,
}

/// Reconnaît le format d'après les premiers octets du fichier.
pub fn detect(path: &Path) -> AppResult<ArchiveKind> {
    let mut head = [0u8; 8];
    let mut file = File::open(path).map_err(io_err)?;
    let n = file.read(&mut head).map_err(io_err)?;
    let head = &head[..n];
    Ok(if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        ArchiveKind::Zip
    } else if head.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        ArchiveKind::SevenZip
    } else if head.starts_with(b"Rar!\x1A\x07") {
        ArchiveKind::Rar
    } else {
        ArchiveKind::Unknown
    })
}

/// Extrait `archive` dans `dest` selon son format.
pub fn extract(
    archive: &Path,
    dest: &Path,
    max_total: u64,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, u64),
) -> AppResult<()> {
    match detect(archive)? {
        ArchiveKind::Zip => installer::extract_zip(archive, dest, max_total, cancel, progress),
        ArchiveKind::SevenZip => extract_7z(archive, dest, max_total, cancel, progress),
        ArchiveKind::Rar => Err(AppError::UnsupportedArchive("RAR".into())),
        ArchiveKind::Unknown => Err(AppError::UnsupportedArchive("ce n'est pas une archive zip ou 7z".into())),
    }
}

/// Nom d'entrée d'archive → chemin relatif sûr. `None` si le chemin sortirait
/// du dossier (`..`, chemin absolu, lecteur Windows, flux NTFS…).
pub fn safe_rel(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            p if p.contains(':') || p.chars().any(char::is_control) => return None,
            p => out.push(p),
        }
    }
    // Ceinture et bretelles : uniquement des composants normaux.
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(out)
}

fn extract_7z(
    archive: &Path,
    dest: &Path,
    max_total: u64,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, u64),
) -> AppResult<()> {
    use sevenz_rust2::{ArchiveReader, Error as SzError, Password};

    let mut reader = ArchiveReader::open(archive, Password::empty()).map_err(|e| AppError::BadArchive(e.to_string()))?;
    fs::create_dir_all(dest).map_err(io_err)?;
    let total = reader.archive().files.len() as u64;
    let mut done: u64 = 0;
    let mut written: u64 = 0;
    // Nos propres erreurs, transmises hors de la fermeture.
    let mut failure: Option<AppError> = None;
    let stop = || SzError::Other("arrêt de l'extraction".into());

    let result = reader.for_each_entries(|entry, data| {
        if cancel.load(Ordering::Relaxed) {
            failure = Some(AppError::Cancelled);
            return Err(stop());
        }
        let name = entry.name().to_string();
        if entry.is_anti_item {
            return Ok(true);
        }
        let Some(rel) = safe_rel(&name) else {
            failure = Some(AppError::BadArchive(format!("chemin dangereux dans l'archive : {name}")));
            return Err(stop());
        };
        done += 1;
        if rel.as_os_str().is_empty() {
            return Ok(true);
        }
        let out = dest.join(&rel);
        if entry.is_directory() {
            if let Err(e) = fs::create_dir_all(&out) {
                failure = Some(io_err(e));
                return Err(stop());
            }
            return Ok(true);
        }
        let copied = (|| -> io::Result<u64> {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut target = File::create(&out)?;
            let remaining = max_total.saturating_sub(written);
            io::copy(&mut data.take(remaining + 1), &mut target)
        })();
        match copied {
            Ok(n) => written += n,
            Err(e) => {
                failure = Some(AppError::Io(format!("{name} : {e}")));
                return Err(stop());
            }
        }
        if written > max_total {
            failure = Some(AppError::BadArchive("contenu bien plus gros qu'annoncé".into()));
            return Err(stop());
        }
        progress(done, total);
        Ok(true)
    });

    if let Some(err) = failure {
        return Err(err);
    }
    result.map_err(|e| match e {
        SzError::PasswordRequired | SzError::MaybeBadPassword(_) => {
            AppError::UnsupportedArchive("archive protégée par un mot de passe".into())
        }
        other => AppError::BadArchive(other.to_string()),
    })?;
    progress(total, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::{make_zip, tempdir};

    #[test]
    fn safe_paths_only() {
        assert_eq!(safe_rel("a/b.txt").unwrap(), PathBuf::from("a/b.txt"));
        assert_eq!(safe_rel("a\\b\\c.dll").unwrap(), PathBuf::from("a/b/c.dll"));
        assert_eq!(safe_rel("./a//b").unwrap(), PathBuf::from("a/b"));
        assert!(safe_rel("../evil").is_none());
        assert!(safe_rel("a/../../evil").is_none());
        assert!(safe_rel("C:\\Windows\\x").is_none());
        assert!(safe_rel("file.txt:stream").is_none());
        assert_eq!(safe_rel("/abs/path").unwrap(), PathBuf::from("abs/path"));
    }

    #[test]
    fn detects_formats() {
        let dir = tempdir("detect");
        let zip = dir.join("a.bin");
        fs::write(&zip, make_zip(&[("x.txt", b"x")])).unwrap();
        assert_eq!(detect(&zip).unwrap(), ArchiveKind::Zip);
        let sz = dir.join("b.bin");
        fs::write(&sz, [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0, 4]).unwrap();
        assert_eq!(detect(&sz).unwrap(), ArchiveKind::SevenZip);
        let rar = dir.join("c.bin");
        fs::write(&rar, b"Rar!\x1A\x07\x01\x00").unwrap();
        assert_eq!(detect(&rar).unwrap(), ArchiveKind::Rar);
        let txt = dir.join("d.bin");
        fs::write(&txt, b"hello").unwrap();
        assert_eq!(detect(&txt).unwrap(), ArchiveKind::Unknown);
        let never = AtomicBool::new(false);
        let err = extract(&rar, &dir.join("out"), 1 << 20, &never, &|_, _| {}).unwrap_err();
        assert!(matches!(err, AppError::UnsupportedArchive(_)));
    }

    #[test]
    fn extracts_7z_archives() {
        // Archive 7z construite avec le compresseur de la bibliothèque.
        let dir = tempdir("sz");
        let src = dir.join("src");
        fs::create_dir_all(src.join("Author.Pallet")).unwrap();
        fs::write(src.join("Author.Pallet/Author.Pallet.pallet.json"), b"{}").unwrap();
        fs::write(src.join("Author.Pallet/data.bundle"), vec![3u8; 5000]).unwrap();
        let archive = dir.join("mod.7z");
        write_7z(&src, &archive);
        assert_eq!(detect(&archive).unwrap(), ArchiveKind::SevenZip);

        let out = dir.join("out");
        let never = AtomicBool::new(false);
        extract(&archive, &out, 1 << 20, &never, &|_, _| {}).unwrap();
        assert_eq!(fs::read(out.join("Author.Pallet/data.bundle")).unwrap(), vec![3u8; 5000]);

        // Trop gros pour la limite annoncée.
        let err = extract(&archive, &dir.join("out2"), 100, &never, &|_, _| {}).unwrap_err();
        assert!(matches!(err, AppError::BadArchive(_)), "{err}");

        // Annulé avant de commencer.
        let err = extract(&archive, &dir.join("out3"), 1 << 20, &AtomicBool::new(true), &|_, _| {}).unwrap_err();
        assert!(matches!(err, AppError::Cancelled));
    }

    /// Écrit une archive 7z (LZMA2) contenant les fichiers de `src`.
    fn write_7z(src: &Path, dest: &Path) {
        use sevenz_rust2::{ArchiveEntry, ArchiveWriter};
        let mut writer = ArchiveWriter::create(dest).unwrap();
        for rel in installer::walk_files(src).unwrap() {
            let name = installer::rel_string(&rel);
            let data = fs::read(src.join(&rel)).unwrap();
            writer
                .push_archive_entry(ArchiveEntry::new_file(&name), Some(io::Cursor::new(data)))
                .unwrap();
        }
        writer.finish().unwrap();
    }
}
