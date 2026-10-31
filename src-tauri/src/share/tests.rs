//! Codes et fichiers de profils (ModStudio et r2modman), contre un faux serveur.

use std::fs;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

use super::r2modman::{self, ConfigFile};
use super::*;
use crate::installer::tests::tempdir;
use crate::installer::InstalledMod;
use crate::testutil::{fake_server, fake_server_with, json_response};
use crate::thunderstore::api::TsClient;

fn sample() -> SharedProfile {
    SharedProfile {
        name: "Coop du soir".into(),
        source: Source::Ts,
        game: SharedGame { id: 0, key: "lethal-company".into(), name: "Lethal Company".into() },
        mods: vec![
            SharedMod { id: "BepInEx-BepInExPack".into(), name: "BepInExPack".into(), version: Some("5.4.2100".into()), file: None, dep: true },
            SharedMod { id: "notnotnotswipez-MoreCompany".into(), name: "MoreCompany".into(), version: Some("1.11.0".into()), file: None, dep: false },
        ],
    }
}

#[test]
fn a_code_carries_the_whole_profile_and_is_found_inside_a_message() {
    let p = sample();
    let code = encode(&p).unwrap();
    assert!(code.starts_with("MS1-"));
    assert!(code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{code}");
    assert_eq!(decode(&code).unwrap(), p);
    // Collé au milieu d'un message, avec un préfixe en minuscules.
    let message = format!("Salut, voilà mon profil : {} — dis-moi si ça marche !", code.replacen("MS1-", "ms1-", 1));
    assert_eq!(decode(&message).unwrap(), p);
    assert!(matches!(classify(&message), Ok(Pasted::ModStudio)));
}

#[test]
fn a_damaged_or_foreign_code_is_refused() {
    let code = encode(&sample()).unwrap();
    let cut = &code[..code.len() - 6];
    assert!(matches!(decode(cut), Err(AppError::ShareInvalid(_))));
    assert!(matches!(decode("MS1-pas_un_vrai_code"), Err(AppError::ShareInvalid(_))));
    assert!(matches!(classify("bonjour"), Err(AppError::ShareInvalid(_))));

    // Un profil d'une version plus récente demande de mettre ModStudio à jour.
    let future = br#"{"format":"modstudio-profile","v":2,"name":"x","source":"ts","game":{"key":"a","name":"A"},"mods":[]}"#;
    let err = from_json(future).unwrap_err();
    assert!(err.to_string().contains("plus récente"), "{err}");
    let other = br#"{"format":"autre","v":1}"#;
    assert!(from_json(other).is_err());
}

#[test]
fn profiles_are_cleaned_and_impossible_mods_refused() {
    let mut p = sample();
    p.name = "  \u{7}Coop\n  ".into();
    p.mods.push(p.mods[1].clone());
    p.mods[1].name = String::new();
    let v = validate(p).unwrap();
    assert_eq!(v.name, "Coop");
    assert_eq!(v.mods.len(), 2, "doublon retiré");
    assert_eq!(v.mods[1].name, "notnotnotswipez-MoreCompany");

    let bad_ts = SharedProfile { mods: vec![SharedMod { id: "../evil".into(), ..sample().mods[0].clone() }], ..sample() };
    assert!(validate(bad_ts).is_err());
    let bad_modio = SharedProfile {
        source: Source::Modio,
        game: SharedGame { id: 34, key: String::new(), name: "x".into() },
        mods: vec![SharedMod { id: "abc".into(), ..sample().mods[0].clone() }],
        ..sample()
    };
    assert!(validate(bad_modio).is_err());
    let no_game = SharedProfile { source: Source::Nexus, game: SharedGame { id: 0, key: "bonelab".into(), name: "B".into() }, ..sample() };
    assert!(validate(no_game).is_err());
    let empty = SharedProfile { mods: Vec::new(), ..sample() };
    assert!(validate(empty).unwrap_err().to_string().contains("aucun mod"));
}

fn record(id: i64, name: &str, file_name: Option<&str>, version: &str, auto: bool, disabled: bool) -> InstalledMod {
    InstalledMod {
        mod_id: id,
        name: name.into(),
        version: Some(version.into()),
        modfile_id: id * 10,
        file_name: file_name.map(str::to_string),
        auto,
        disabled,
        ..Default::default()
    }
}

#[test]
fn export_shares_enabled_mods_with_their_dependencies_first() {
    let mut g = GameInstall { game_id: 5, game_name: "Lethal Company".into(), domain: Some("lethal-company".into()), ..Default::default() };
    g.mods.insert(1, record(1, "MoreCompany", Some("notnotnotswipez-MoreCompany"), "1.11.0", false, false));
    g.mods.insert(2, record(2, "BepInExPack", Some("BepInEx-BepInExPack"), "5.4.2100", true, false));
    g.mods.insert(3, record(3, "Off", Some("Someone-Off"), "1.0.0", false, true));
    g.mods.insert(4, record(4, "Rml", Some("rml:Some.Mod"), "2.0.0", false, false));
    let (p, left_out) = export(Source::Ts, &g);
    assert_eq!(left_out, 1);
    assert_eq!(p.game.key, "lethal-company");
    assert_eq!(p.name, "Lethal Company");
    let ids: Vec<&str> = p.mods.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["BepInEx-BepInExPack", "notnotnotswipez-MoreCompany", "rml:Some.Mod"]);
    assert!(p.mods.iter().all(|m| m.file.is_none()));

    // mod.io et Nexus : le numéro du mod et le fichier installé ; le profil actif donne son nom.
    let mut m = GameInstall { game_id: 3809, game_name: "BONELAB".into(), active_profile: Some("Coop".into()), ..Default::default() };
    m.mods.insert(77, record(77, "Fusion", None, "1.0", false, false));
    let (p, _) = export(Source::Modio, &m);
    assert_eq!((p.name.as_str(), p.game.id), ("Coop", 3809));
    assert_eq!((p.mods[0].id.as_str(), p.mods[0].file), ("77", Some(770)));
    assert_eq!(validate(p.clone()).unwrap(), p);

    // Communautés : l'identifiant du mod dans la liste du jeu.
    let mut c = GameInstall { game_id: crate::lists::game_id("raft"), game_name: "Raft".into(), ..Default::default() };
    c.mods.insert(9, record(9, "BiofuelCounter", Some("raft:biofuelcounter"), "1.3", false, false));
    let (p, _) = export(Source::Cl, &c);
    assert_eq!((p.game.key.as_str(), p.mods[0].id.as_str()), ("raft", "biofuelcounter"));
}

/// `export.r2x` tel que l'écrit r2modman (js-yaml).
const R2X: &str = "profileName: 'Coop: le retour'\nmods:\n  - name: BepInEx-BepInExPack\n    version:\n      major: 5\n      minor: 4\n      patch: 2100\n    enabled: true\n  - name: notnotnotswipez-MoreCompany\n    version:\n      major: 1\n      minor: 11\n      patch: 0\n    enabled: false\n  - name: pas un mod\n    version:\n      major: 1\n";

fn r2z(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn r2modman_profiles_are_read_with_only_safe_config_files() {
    let bytes = r2z(&[
        ("export.r2x", R2X.as_bytes()),
        ("config/BepInEx.cfg", b"[Logging]"),
        ("config/MoreCompany/settings.json", b"{}"),
        ("config/../evil.cfg", b"x"),
        ("config/payload.dll", b"MZ"),
        ("BepInEx/plugins/x/readme.txt", b"hors config"),
    ]);
    let p = r2modman::parse_r2z(&bytes).unwrap();
    assert_eq!(p.name, "Coop: le retour");
    assert_eq!(p.mods.len(), 2, "l'entrée sans version est ignorée");
    assert_eq!((p.mods[0].full_name.as_str(), p.mods[0].version.as_str(), p.mods[0].enabled), ("BepInEx-BepInExPack", "5.4.2100", true));
    assert!(!p.mods[1].enabled);
    let paths: Vec<&str> = p.configs.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, ["BepInEx.cfg", "MoreCompany/settings.json"]);

    assert!(r2modman::is_code("0192f0a1-3b4c-7d8e-9f00-112233445566"));
    assert!(!r2modman::is_code("0192f0a1-3b4c-7d8e-9f00-11223344556"));
    assert!(!r2modman::is_code("MS1-abc"));
}

#[test]
fn a_built_r2modman_profile_reads_back_the_same() {
    let dir = tempdir("share-r2-config");
    fs::create_dir_all(dir.join("MoreCompany")).unwrap();
    fs::write(dir.join("BepInEx.cfg"), b"[Logging]").unwrap();
    fs::write(dir.join("MoreCompany/settings.json"), b"{\"a\":1}").unwrap();
    fs::write(dir.join("notes.exe"), b"MZ").unwrap();
    let mods = vec![("BepInEx-BepInExPack".to_string(), "5.4.2100".to_string()), ("A-B".to_string(), "1.0.0".to_string())];
    let zip = r2modman::build_r2z("Profil \"spécial\"", &mods, Some(&dir)).unwrap();
    let back = r2modman::parse_r2z(&zip).unwrap();
    assert_eq!(back.name, "Profil \"spécial\"");
    assert_eq!(back.mods.iter().map(|m| (m.full_name.clone(), m.version.clone())).collect::<Vec<_>>(), mods);
    assert_eq!(back.configs.len(), 2, "le .exe n'est pas joint");

    let payload = format!("{}\n{}", r2modman::PREFIX, base64::engine::general_purpose::STANDARD.encode(&zip));
    assert_eq!(r2modman::decode_payload(&payload).unwrap(), zip);
    assert!(r2modman::decode_payload("pas un profil").is_err());
}

#[test]
fn config_files_replace_old_ones_after_a_backup() {
    let folder = tempdir("share-configs-game");
    let backup = tempdir("share-configs-backup").join("b");
    let game = GameInstall { game_id: 5, game_name: "Lethal Company".into(), folder: folder.clone(), ..Default::default() };
    let files = vec![
        ConfigFile { path: "BepInEx.cfg".into(), data: b"nouveau".to_vec() },
        ConfigFile { path: "Same.cfg".into(), data: b"pareil".to_vec() },
        ConfigFile { path: "Mod/new.json".into(), data: b"{}".to_vec() },
    ];
    // Pas de BepInEx : rien n'est écrit.
    assert!(write_configs(&game, &files, &backup).is_err());

    fs::create_dir_all(folder.join("BepInEx/config")).unwrap();
    fs::write(folder.join("BepInEx/config/BepInEx.cfg"), b"ancien").unwrap();
    fs::write(folder.join("BepInEx/config/Same.cfg"), b"pareil").unwrap();
    let r = write_configs(&game, &files, &backup).unwrap();
    assert_eq!(r.written, 2, "le fichier identique n'est pas réécrit");
    assert_eq!(fs::read(folder.join("BepInEx/config/BepInEx.cfg")).unwrap(), b"nouveau");
    assert_eq!(fs::read(folder.join("BepInEx/config/Mod/new.json")).unwrap(), b"{}");
    assert_eq!(fs::read(backup.join("BepInEx.cfg")).unwrap(), b"ancien");
    assert!(r.backup.is_some());

    // Un dossier lié ailleurs n'est pas suivi.
    #[cfg(unix)]
    {
        let outside = tempdir("share-configs-outside");
        std::os::unix::fs::symlink(&outside, folder.join("BepInEx/config/Lien")).unwrap();
        let r = write_configs(&game, &[ConfigFile { path: "Lien/x.cfg".into(), data: b"x".to_vec() }], &backup).unwrap();
        assert_eq!(r.written, 0);
        assert!(!outside.join("x.cfg").exists());
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

#[test]
fn r2modman_codes_are_fetched_through_the_thunderstore_redirect() {
    let zip = r2z(&[("export.r2x", R2X.as_bytes())]);
    let payload = format!("{}\n{}", r2modman::PREFIX, base64::engine::general_purpose::STANDARD.encode(&zip));
    let server = fake_server_with(|base| {
        let host = base.trim_end_matches("/v1");
        vec![
            format!("HTTP/1.1 302 Found\r\nLocation: {host}/cdn/profile\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len()),
            json_response(404, "{}"),
        ]
    });
    let client = TsClient::new(server.base.trim_end_matches("/v1"));
    let code = "0192F0A1-3b4c-7d8e-9f00-112233445566";
    let text = runtime().block_on(client.legacy_profile(code)).unwrap();
    assert_eq!(server.request(0).line, "GET /api/experimental/legacyprofile/get/0192f0a1-3b4c-7d8e-9f00-112233445566/ HTTP/1.1");
    let p = r2modman::parse_r2z(&r2modman::decode_payload(&text).unwrap()).unwrap();
    assert_eq!(p.mods.len(), 2);
    let err = runtime().block_on(client.legacy_profile(code)).unwrap_err();
    assert!(matches!(err, AppError::ShareCodeUnknown), "{err:?}");
}

#[test]
fn publishing_a_profile_returns_its_code() {
    let server = fake_server(vec![json_response(200, r#"{"key":"0192f0a1-3b4c-7d8e-9f00-112233445566"}"#)]);
    let client = TsClient::new(server.base.trim_end_matches("/v1"));
    let key = runtime().block_on(client.create_legacy_profile("#r2modman\nAAAA".into())).unwrap();
    assert_eq!(key, "0192f0a1-3b4c-7d8e-9f00-112233445566");
    let seen = server.request(0);
    assert_eq!(seen.line, "POST /api/experimental/legacyprofile/create/ HTTP/1.1");
    assert_eq!(seen.header("content-type").as_deref(), Some("application/octet-stream"));
    assert_eq!(seen.body, "#r2modman\nAAAA");
}

#[test]
fn communities_of_a_package_come_from_its_listings() {
    let server = fake_server(vec![json_response(
        200,
        r#"{"namespace":"A","name":"B","community_listings":[{"community":"lethal-company","categories":[]},{"community":"content-warning"}]}"#,
    )]);
    let client = TsClient::new(server.base.trim_end_matches("/v1"));
    let list = runtime().block_on(client.communities_of("A", "B")).unwrap();
    assert_eq!(list, ["lethal-company", "content-warning"]);
}
