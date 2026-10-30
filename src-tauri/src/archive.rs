//! Extraction des archives téléchargées sur Nexus Mods (zip, 7z et RAR), avec
//! les mêmes garde-fous que pour mod.io : pas de chemin qui sorte du dossier,
//! pas d'archive démesurée, annulation possible.

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
        ArchiveKind::Rar => extract_rar(archive, dest, max_total, cancel, progress),
        ArchiveKind::Unknown => Err(AppError::UnsupportedArchive("ce n'est pas une archive zip, 7z ou RAR".into())),
    }
}

/// Fichiers qui accompagnent souvent des archives sans rien installer
/// (lisez-moi, captures d'écran).
const SIDE_FILES: &[&str] = &["txt", "md", "pdf", "rtf", "nfo", "url", "htm", "html", "png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Archives que contient un mod fait seulement d'archives (« une archive par
/// version ou par option ») : à la racine, ou dans l'unique dossier qui
/// l'enveloppe, avec au plus des lisez-moi et des images à côté. Vide si le
/// mod contient autre chose.
pub fn nested_archives(dir: &Path) -> Vec<PathBuf> {
    let list = |d: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(d).map(|it| it.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        v.sort();
        v
    };
    let mut entries = list(dir);
    if let [only] = entries.as_slice()
        && only.is_dir()
    {
        entries = list(&only.clone());
    }
    let mut archives = Vec::new();
    for path in entries {
        if path.is_dir() {
            return Vec::new();
        }
        if detect(&path).is_ok_and(|k| k != ArchiveKind::Unknown) {
            archives.push(path);
            continue;
        }
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !SIDE_FILES.contains(&ext.as_str()) {
            return Vec::new();
        }
    }
    archives
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

/// Extraction RAR (RAR 4 et 5, archives « solides ») avec le décompresseur
/// officiel. Les liens symboliques ne sont jamais suivis : un lien créé par
/// l'extraction est supprimé aussitôt.
fn extract_rar(
    archive: &Path,
    dest: &Path,
    max_total: u64,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, u64),
) -> AppResult<()> {
    use unrar::error::{Code, UnrarError};

    let map = |e: UnrarError| match e.code {
        Code::MissingPassword | Code::BadPassword => AppError::UnsupportedArchive("archive protégée par un mot de passe".into()),
        Code::EOpen => AppError::UnsupportedArchive("archive RAR en plusieurs parties : il manque les autres parties".into()),
        _ => AppError::BadArchive(format!("archive RAR illisible ({e})")),
    };

    // Premier passage : liste et taille totale, avant d'écrire quoi que ce soit.
    let mut total: u64 = 0;
    let mut size: u64 = 0;
    let mut listing = unrar::Archive::new(archive).open_for_listing().map_err(map)?;
    if listing.is_locked() || listing.has_encrypted_headers() {
        return Err(AppError::UnsupportedArchive("archive protégée par un mot de passe".into()));
    }
    for entry in &mut listing {
        let entry = entry.map_err(map)?;
        if entry.is_split() {
            return Err(AppError::UnsupportedArchive("archive RAR en plusieurs parties : il manque les autres parties".into()));
        }
        if entry.is_encrypted() {
            return Err(AppError::UnsupportedArchive("archive protégée par un mot de passe".into()));
        }
        total += 1;
        size = size.saturating_add(entry.unpacked_size);
    }
    if total == 0 {
        return Err(AppError::BadArchive("archive RAR vide ou illisible".into()));
    }
    if size > max_total {
        return Err(AppError::BadArchive("contenu bien plus gros qu'annoncé".into()));
    }

    fs::create_dir_all(dest).map_err(io_err)?;
    let mut done: u64 = 0;
    let mut written: u64 = 0;
    let mut open = unrar::Archive::new(archive).open_for_processing().map_err(map)?;
    while let Some(header) = open.read_header().map_err(map)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let name = header.entry().filename.to_string_lossy().into_owned();
        let Some(rel) = safe_rel(&name) else {
            return Err(AppError::BadArchive(format!("chemin dangereux dans l'archive : {name}")));
        };
        done += 1;
        let out = dest.join(&rel);
        if rel.as_os_str().is_empty() || header.entry().is_directory() {
            if !rel.as_os_str().is_empty() {
                fs::create_dir_all(&out).map_err(io_err)?;
            }
            open = header.skip().map_err(map)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        open = header.extract_to(&out).map_err(map)?;
        match fs::symlink_metadata(&out) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let _ = fs::remove_file(&out);
                return Err(AppError::BadArchive(format!("lien symbolique dans l'archive : {name}")));
            }
            Ok(meta) => written += meta.len(),
            Err(_) => {}
        }
        if written > max_total {
            return Err(AppError::BadArchive("contenu bien plus gros qu'annoncé".into()));
        }
        progress(done, total);
    }
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
        assert!(matches!(err, AppError::BadArchive(_)), "archive tronquée : {err}");
    }

    /// Archive RAR de test (bibliothèque unrar), copiée sous le nom du fichier téléchargé.
    fn rar_copy(name: &str, dir: &Path) -> PathBuf {
        let path = dir.join(format!("{name}.part"));
        fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/rar").join(name), &path).unwrap();
        path
    }

    #[test]
    fn extracts_rar_archives() {
        let dir = tempdir("rar");
        let never = AtomicBool::new(false);
        let version = rar_copy("version.rar", &dir);
        assert_eq!(detect(&version).unwrap(), ArchiveKind::Rar);
        extract(&version, &dir.join("out"), 1 << 20, &never, &|_, _| {}).unwrap();
        assert_eq!(fs::read_to_string(dir.join("out/VERSION")).unwrap(), "unrar-0.4.0");

        let unicode = rar_copy("unicode-entry.rar", &dir);
        extract(&unicode, &dir.join("uni"), 1 << 20, &never, &|_, _| {}).unwrap();
        assert!(dir.join("uni/unicodefilename❤️.txt").is_file());

        // Plus gros que la limite : refusé avant d'écrire quoi que ce soit.
        let err = extract(&version, &dir.join("small"), 3, &never, &|_, _| {}).unwrap_err();
        assert!(matches!(err, AppError::BadArchive(_)), "{err}");
        assert!(!dir.join("small").exists());

        // Annulée : arrêt avant le premier fichier.
        let cancel = AtomicBool::new(true);
        assert!(matches!(extract(&version, &dir.join("stop"), 1 << 20, &cancel, &|_, _| {}), Err(AppError::Cancelled)));
    }

    #[test]
    fn refuses_protected_or_partial_rar_archives() {
        let dir = tempdir("rar-bad");
        let never = AtomicBool::new(false);
        let err = extract(&rar_copy("crypted.rar", &dir), &dir.join("a"), 1 << 20, &never, &|_, _| {}).unwrap_err();
        assert!(matches!(&err, AppError::UnsupportedArchive(m) if m.contains("mot de passe")), "{err}");
        let err = extract(&rar_copy("archive.part1.rar", &dir), &dir.join("b"), 1 << 24, &never, &|_, _| {}).unwrap_err();
        assert!(matches!(&err, AppError::UnsupportedArchive(m) if m.contains("plusieurs parties")), "{err}");
    }

    #[test]
    fn finds_archives_inside_a_mod() {
        let dir = tempdir("nested");
        fs::write(dir.join("Version PC.zip"), make_zip(&[("a.txt", b"a")])).unwrap();
        fs::write(dir.join("Version Quest.rar"), b"Rar!\x1A\x07\x01\x00").unwrap();
        fs::write(dir.join("Lisez-moi.txt"), b"choisir une version").unwrap();
        fs::write(dir.join("apercu.png"), b"png").unwrap();
        let found = nested_archives(&dir);
        let names: Vec<_> = found.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Version PC.zip", "Version Quest.rar"]);

        // Dans un dossier qui enveloppe tout.
        let wrapped = tempdir("nested-wrap");
        fs::create_dir(wrapped.join("Mod")).unwrap();
        fs::write(wrapped.join("Mod/Mod.7z"), [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0, 4]).unwrap();
        assert_eq!(nested_archives(&wrapped).len(), 1);

        // Un mod avec d'autres fichiers n'est pas fait que d'archives.
        fs::write(dir.join("plugin.dll"), b"MZ").unwrap();
        assert!(nested_archives(&dir).is_empty());
        let with_dir = tempdir("nested-dir");
        fs::write(with_dir.join("a.zip"), make_zip(&[("a.txt", b"a")])).unwrap();
        fs::create_dir(with_dir.join("textures")).unwrap();
        assert!(nested_archives(&with_dir).is_empty());
    }

    #[test]
    fn extracts_zips_with_other_compression_methods() {
        use std::io::Write;
        use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};
        let dir = tempdir("zip-methods");
        let never = AtomicBool::new(false);
        let text = "texte du mod ".repeat(200);
        for method in [CompressionMethod::Bzip2, CompressionMethod::Zstd, CompressionMethod::Xz] {
            let mut w = ZipWriter::new(io::Cursor::new(Vec::new()));
            w.start_file("Mod/lisez-moi.txt", SimpleFileOptions::default().compression_method(method)).unwrap();
            w.write_all(text.as_bytes()).unwrap();
            let path = dir.join(format!("{method:?}.zip"));
            fs::write(&path, w.finish().unwrap().into_inner()).unwrap();
            let out = dir.join(format!("{method:?}"));
            extract(&path, &out, 1 << 20, &never, &|_, _| {}).unwrap_or_else(|e| panic!("{method:?} : {e}"));
            assert_eq!(fs::read_to_string(out.join("Mod/lisez-moi.txt")).unwrap(), text, "{method:?}");
        }
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
