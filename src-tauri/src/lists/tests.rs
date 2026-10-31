//! Les listes de chaque jeu et l'installation de leurs mods, contre un faux serveur.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use md5::{Digest, Md5};
use sha2::Sha256;

use super::games::{self, GameDef};
use super::install::RULES;
use super::{game_id, mod_id, ListsState, Urls};
use crate::error::AppError;
use crate::installer::tests::{make_zip, tempdir};
use crate::installer::{InstallEvent, STAGING_DIR};
use crate::testutil::{fake_server_with, json_response};

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha_b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(bytes))
}

fn md5(bytes: &[u8]) -> String {
    Md5::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn raw(status: &str, headers: &str, body: &[u8]) -> String {
    let mut s = format!("HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    s.extend(body.iter().map(|&b| b as char));
    s
}

fn file(body: &[u8]) -> String {
    raw("200 OK", "Content-Type: application/octet-stream\r\n", body)
}

fn text(body: &str) -> String {
    raw("200 OK", "Content-Type: text/plain\r\n", body.as_bytes())
}

fn quiet() -> Arc<dyn Fn(InstallEvent) + Send + Sync> {
    Arc::new(|_| {})
}

fn state(base: &str, tag: &str) -> ListsState {
    let urls = Urls {
        modlinks: format!("{base}/ModLinks.xml"),
        apilinks: format!("{base}/ApiLinks.xml"),
        owdb: format!("{base}/database.json"),
        beatmods: base.to_string(),
        cvrmg: format!("{base}/cvrmg"),
        raft: base.to_string(),
        github_api: base.to_string(),
    };
    ListsState::with_urls(tempdir(tag).join("installed-lists.json"), urls)
}

fn game(id: &str) -> &'static GameDef {
    games::game(id).unwrap()
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

// ---------------------------------------------------------------- Hollow Knight

fn hk_folder(tag: &str, version: &str) -> PathBuf {
    let dir = tempdir(tag);
    fs::write(dir.join("hollow_knight.exe"), b"exe").unwrap();
    fs::create_dir_all(dir.join("hollow_knight_Data/Managed")).unwrap();
    let mut asm = b"vanilla assembly ".to_vec();
    asm.extend(utf16(version));
    fs::write(dir.join("hollow_knight_Data/Managed/Assembly-CSharp.dll"), asm).unwrap();
    dir
}

fn hk_lists(base: &str, api: &[u8], satchel: &[u8], charm: &[u8]) -> (String, String) {
    let modlinks = format!(
        r#"<?xml version="1.0"?>
<ModLinks xmlns="https://github.com/HollowKnight-Modding/HollowKnight.ModLinks/HollowKnight.ModManager">
  <Manifest>
    <Name>Satchel</Name><Description>A library</Description><Version>0.8.12.0</Version>
    <Link SHA256="{}"><![CDATA[
      {base}/Satchel.dll
    ]]></Link>
    <Dependencies/>
    <Repository><![CDATA[https://github.com/PrashantMohta/Satchel]]></Repository>
    <Tags><Tag>Library</Tag></Tags><Authors><Author>Dandy</Author></Authors>
  </Manifest>
  <Manifest>
    <Name>Charm Mod</Name><DisplayName>Charm Mod!</DisplayName><Description>More charms</Description><Version>1.0.2.0</Version>
    <Link SHA256="{}"><![CDATA[{base}/CharmMod.zip]]></Link>
    <Dependencies><Dependency>Satchel</Dependency></Dependencies>
    <Repository><![CDATA[https://github.com/someone/CharmMod]]></Repository>
  </Manifest>
</ModLinks>"#,
        sha_hex(satchel).to_uppercase(),
        sha_hex(charm)
    );
    let apilinks = format!(
        r#"<?xml version="1.0"?>
<ApiLinks xmlns="https://github.com/HollowKnight-Modding/HollowKnight.ModLinks/HollowKnight.ModManager">
  <Manifest>
    <Version>77</Version>
    <Links>
      <Linux SHA256="{h}"><![CDATA[{base}/api.zip]]></Linux>
      <Mac SHA256="{h}"><![CDATA[{base}/api.zip]]></Mac>
      <Windows SHA256="{h}"><![CDATA[{base}/api.zip]]></Windows>
    </Links>
    <Files><File>Assembly-CSharp.dll</File></Files>
  </Manifest>
</ApiLinks>"#,
        h = sha_hex(api)
    );
    (modlinks, apilinks)
}

#[tokio::test]
async fn hollow_knight_gets_the_api_then_mods_in_their_folders_and_the_game_back_on_removal() {
    let folder = hk_folder("lists-hk", "1.5.78.11833");
    let mut modded = b"modded ModHooks ".to_vec();
    modded.extend(utf16("1.5.78.11833"));
    let api = make_zip(&[("Assembly-CSharp.dll", &modded), ("MMHOOK_Assembly-CSharp.dll", b"hooks")]);
    let satchel = b"satchel dll".to_vec();
    let charm = make_zip(&[("CharmMod.dll", b"charm"), ("README.md", b"readme")]);
    let (a, s, c) = (api.clone(), satchel.clone(), charm.clone());
    let server = fake_server_with(move |base| {
        let base = base.trim_end_matches("/v1");
        let (modlinks, apilinks) = hk_lists(base, &a, &s, &c);
        vec![text(&modlinks), text(&apilinks), file(&a), file(&s), file(&c)]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-hk-state");
    let hk = game("hollow-knight");
    let config = st.configure(hk, folder.to_str().unwrap()).unwrap();
    assert_eq!(config.rules.as_deref(), Some(RULES));
    let loader = st.loader_state(hk).await.unwrap();
    assert!(!loader.present && loader.installable && loader.note.is_none());
    assert_eq!(loader.game_version.as_deref(), Some("1.5.78.11833"));

    let out = st.install(hk, "Charm Mod", None, &[], quiet()).await.unwrap();
    assert_eq!(out.installed, ["Modding API", "Satchel", "Charm Mod!"]);
    let managed = folder.join("hollow_knight_Data/Managed");
    assert_eq!(fs::read(managed.join("Assembly-CSharp.dll")).unwrap(), modded);
    assert!(fs::read(managed.join("Assembly-CSharp.dll.modstudio-orig")).unwrap().starts_with(b"vanilla"));
    assert_eq!(fs::read(managed.join("Mods/Satchel/Satchel.dll")).unwrap(), satchel);
    assert_eq!(fs::read(managed.join("Mods/Charm Mod/CharmMod.dll")).unwrap(), b"charm");
    assert!(!folder.join(STAGING_DIR).exists());
    assert!(st.loader_state(hk).await.unwrap().present);

    let gid = game_id("hollow-knight");
    let api_id = mod_id("hollow-knight", "Modding API");
    // Désactivé : le jeu retrouve ses fichiers ; réactivé : le Modding API revient.
    st.set_enabled(gid, api_id, false, &[]).unwrap();
    assert!(fs::read(managed.join("Assembly-CSharp.dll")).unwrap().starts_with(b"vanilla"));
    assert!(!managed.join("Assembly-CSharp.dll.modstudio-orig").exists());
    st.set_enabled(gid, api_id, true, &[]).unwrap();
    assert_eq!(fs::read(managed.join("Assembly-CSharp.dll")).unwrap(), modded);
    assert!(fs::read(managed.join("Assembly-CSharp.dll.modstudio-orig")).unwrap().starts_with(b"vanilla"));
    let rec = &out.game.mods[&api_id];
    assert_eq!(rec.replaced, ["hollow_knight_Data/Managed/Assembly-CSharp.dll"]);
    assert!(rec.auto && rec.required_by.contains(&mod_id("hollow-knight", "Charm Mod")) && rec.required_by.contains(&mod_id("hollow-knight", "Satchel")));
    let removed = st.remove(gid, mod_id("hollow-knight", "Charm Mod")).unwrap();
    assert_eq!(removed.orphans.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["Satchel"]);
    let removed = st.remove(gid, mod_id("hollow-knight", "Satchel")).unwrap();
    assert_eq!(removed.orphans.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["Modding API"]);
    st.remove(gid, api_id).unwrap();
    assert!(fs::read(managed.join("Assembly-CSharp.dll")).unwrap().starts_with(b"vanilla"), "le fichier du jeu est remis");
    assert!(!managed.join("Assembly-CSharp.dll.modstudio-orig").exists());
}

#[tokio::test]
async fn hollow_knight_refuses_the_api_for_another_game_version() {
    let folder = hk_folder("lists-hk-version", "1.5.12620.0");
    let server = fake_server_with(|base| {
        let base = base.trim_end_matches("/v1");
        let (modlinks, apilinks) = hk_lists(base, b"api", b"satchel", b"charm");
        vec![text(&modlinks), text(&apilinks)]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-hk-version-state");
    let hk = game("hollow-knight");
    st.configure(hk, folder.to_str().unwrap()).unwrap();
    let err = st.install(hk, "Satchel", None, &[], quiet()).await.unwrap_err();
    assert!(err.to_string().contains("1.5.78.11833") && err.to_string().contains("1.5.12620.0"), "{err}");
    assert_eq!(server.count(), 2, "rien n'est téléchargé");
    assert!(st.loader_state(hk).await.unwrap().note.is_some());
}

// ---------------------------------------------------------------- ChilloutVR

#[tokio::test]
async fn chilloutvr_gets_melonloader_and_mods_named_by_the_server_after_checking_their_hash() {
    let folder = tempdir("lists-cvr");
    fs::write(folder.join("ChilloutVR.exe"), b"exe").unwrap();
    let melon = make_zip(&[("version.dll", b"proxy"), ("MelonLoader/net35/MelonLoader.dll", b"ml")]);
    let (lib, video) = (b"btkui".to_vec(), b"video remote".to_vec());
    let (m, l, v) = (melon.clone(), lib.clone(), video.clone());
    let server = fake_server_with(move |base| {
        let base = base.trim_end_matches("/v1");
        let list = serde_json::json!([
            { "_id": 5, "aliases": ["Video Remote Mod"], "category": "Utilities & Tweaks", "name": "Video Remote", "versions": [{
                "approvalStatus": 1, "name": "Video Remote", "modVersion": "1.7.25", "gameVersion": "2026r182", "loaderVersion": "v0.7.2",
                "modType": "Mod", "author": "Shin", "description": "Remote", "searchTags": ["video"],
                "requirements": ["[Requires - BTKUILib](https://api.cvrmg.com/v1/mods/download/113)", "None"],
                "downloadLink": format!("{base}/v1/mods/download/5"), "sourceLink": "https://github.com/Nirv-git/CVRMods", "hash": sha_b64(&v)
            }]},
            { "_id": 113, "aliases": [], "category": "Core Mods & Libraries", "name": "BTKUILib", "versions": [{
                "approvalStatus": 1, "name": "BTKUILib", "modVersion": "2.3.0", "modType": "Mod", "author": "BTK",
                "requirements": [""], "downloadLink": format!("{base}/v1/mods/download/113"), "hash": sha_b64(&l)
            }]},
            { "_id": 7, "name": "Old", "versions": [{ "approvalStatus": 3, "name": "Old", "modVersion": "1.0.0", "downloadLink": format!("{base}/x"), "hash": sha_b64(b"o") }]}
        ]);
        let release = serde_json::json!({ "tag_name": "v0.7.3", "assets": [
            { "name": "MelonLoader.x64.zip", "browser_download_url": format!("{base}/MelonLoader.x64.zip"), "digest": format!("sha256:{}", sha_hex(&m)) },
            { "name": "MelonLoader.x86.zip", "browser_download_url": format!("{base}/x86.zip"), "digest": null }
        ]});
        vec![
            json_response(200, &list.to_string()),
            json_response(200, &release.to_string()),
            file(&m),
            raw("302 Found", &format!("Location: {base}/v1/mods/files/113/6/BTKUILib.dll\r\n"), b""),
            file(&l),
            raw("200 OK", "Content-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"VideoRemote.dll\"\r\n", &v),
        ]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-cvr-state");
    let cvr = game("chilloutvr");
    st.configure(cvr, folder.to_str().unwrap()).unwrap();
    let catalog = st.catalog(cvr, false).await.unwrap();
    let old = catalog.get("7").unwrap();
    assert!(old.deprecated && old.note.is_some());
    let video_mod = catalog.get("5").unwrap();
    assert_eq!(video_mod.versions[0].dependencies[0].name, "BTKUILib");
    assert!(video_mod.versions[0].game_versions.contains(&"MelonLoader 0.7.2".to_string()));
    assert_eq!(catalog.loader().unwrap().latest, "0.7.3");

    let out = st.install(cvr, "5", None, &[], quiet()).await.unwrap();
    assert_eq!(out.installed, ["MelonLoader", "BTKUILib", "Video Remote"]);
    assert_eq!(fs::read(folder.join("version.dll")).unwrap(), b"proxy");
    assert!(folder.join("MelonLoader/net35/MelonLoader.dll").is_file());
    assert_eq!(fs::read(folder.join("Mods/BTKUILib.dll")).unwrap(), lib);
    assert_eq!(fs::read(folder.join("Mods/VideoRemote.dll")).unwrap(), video);
    let lib_rec = &out.game.mods[&mod_id("chilloutvr", "113")];
    assert!(lib_rec.auto && lib_rec.required_by == [mod_id("chilloutvr", "5")]);
    assert!(st.loader_state(cvr).await.unwrap().present);
}

// ---------------------------------------------------------------- Raft

fn raft_list() -> serde_json::Value {
    serde_json::json!([
        { "id": "biofuelcounter", "title": "BiofuelCounter", "description": "Counts biofuel", "category": "utility", "author": "vsevolorod",
          "iconImageUrl": "https://example.org/icon.png", "bannerImageUrl": null, "mod-versions": [
            { "id": 10, "modId": "biofuelcounter", "version": "1.2", "changelog": "", "downloadUrl": "/mods/biofuelcounter/1.2/BiofuelCounter v1.2.rmod", "downloadCount": 5, "maxRaftVersionId": 31, "definiteMaxRaftVersion": false },
            { "id": 12, "modId": "biofuelcounter", "version": "1.3", "changelog": "Fix", "downloadUrl": "/mods/biofuelcounter/1.3/BiofuelCounter v1.3.rmod", "downloadCount": 7, "maxRaftVersionId": 31, "definiteMaxRaftVersion": false }
        ]},
        { "id": "nofog", "title": "NoFog", "description": "", "category": "fun", "author": "x", "mod-versions": [
            { "id": 86, "modId": "nofog", "version": "1.0", "downloadUrl": "/mods/nofog/1.0/NoFog.rmod", "maxRaftVersionId": 25, "definiteMaxRaftVersion": true }
        ]}
    ])
}

#[tokio::test]
async fn raft_mods_go_to_the_mods_folder_and_a_warning_page_is_refused() {
    let folder = tempdir("lists-raft");
    fs::write(folder.join("Raft.exe"), b"exe").unwrap();
    let rmod = make_zip(&[("modinfo.json", b"{}")]);
    let r = rmod.clone();
    let server = fake_server_with(move |_| {
        vec![
            json_response(200, &raft_list().to_string()),
            raw("300 Multiple Choices", "Content-Type: text/html; charset=utf-8\r\n", b"<!DOCTYPE html>"),
            file(&r),
        ]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-raft-state");
    let raft = game("raft");
    st.configure(raft, folder.to_str().unwrap()).unwrap();
    let catalog = st.catalog(raft, false).await.unwrap();
    let bio = catalog.get("biofuelcounter").unwrap();
    assert_eq!((bio.latest.as_str(), bio.downloads), ("1.3", Some(12)));
    assert_eq!(bio.versions[0].files, ["mods/BiofuelCounter v1.3.rmod"]);
    let nofog = catalog.get("nofog").unwrap();
    assert!(nofog.deprecated && nofog.note.as_deref().is_some_and(|n| n.contains("ancienne version")));
    let state_ = st.loader_state(raft).await.unwrap();
    assert!(!state_.installable && state_.note.is_some_and(|n| n.contains("RMLLauncher")));

    let err = st.install(raft, "biofuelcounter", None, &[], quiet()).await.unwrap_err();
    assert!(matches!(err, AppError::ListDownload { ref detail, .. } if detail.contains("page web")), "{err:?}");
    assert!(!folder.join("mods").exists());
    st.install(raft, "biofuelcounter", None, &[], quiet()).await.unwrap();
    assert_eq!(fs::read(folder.join("mods/BiofuelCounter v1.3.rmod")).unwrap(), rmod);
}

// ---------------------------------------------------------------- Beat Saber

fn bs_folder(tag: &str) -> PathBuf {
    let dir = tempdir(tag);
    fs::write(dir.join("Beat Saber.exe"), b"exe").unwrap();
    fs::create_dir_all(dir.join("Beat Saber_Data/Managed")).unwrap();
    fs::write(dir.join("Beat Saber_Data/globalgamemanagers"), b"\x00\x002021.3.16f1\x00 junk 1.40.8_7379 \x00").unwrap();
    dir
}

fn bs_mods(bsipa: &[(&str, &[u8])], songcore: &[u8], tamper: bool) -> serde_json::Value {
    let hashes = |files: &[(&str, &[u8])]| -> Vec<serde_json::Value> {
        files.iter().map(|(p, b)| serde_json::json!({ "path": p, "hash": md5(b) })).collect()
    };
    let song_hash = if tamper { md5(b"something else") } else { md5(songcore) };
    serde_json::json!({ "mods": [
        { "mod": { "id": 1, "name": "BSIPA", "summary": "A mod loader", "description": "", "category": "core", "authors": [{ "displayName": "Meivyn" }], "gitUrl": "https://github.com/nike4613/BeatSaber-IPA-Reloaded" },
          "latest": { "id": 1158, "modId": 1, "modVersion": "4.3.6", "platform": "universalpc", "zipHash": "22ddb9499bf8b308ea30d2b555172baf", "contentHashes": hashes(bsipa), "dependencies": [], "downloadCount": 10 } },
        { "mod": { "id": 2, "name": "SongCore", "summary": "Custom songs", "category": "core", "authors": [{ "username": "Kyle1413" }], "gitUrl": "https://github.com/Kylemc1413/SongCore" },
          "latest": { "id": 2001, "modId": 2, "modVersion": "3.15.4", "platform": "universalpc", "zipHash": "efdb620e232d3193fbdff97cbe357804",
                      "contentHashes": [{ "path": "Plugins/SongCore.dll", "hash": song_hash }], "dependencies": [1158, 999], "downloadCount": 3 } }
    ]})
}

#[tokio::test]
async fn beat_saber_reads_its_version_installs_bsipa_like_ipa_and_checks_every_file() {
    let folder = bs_folder("lists-bs");
    let files: [(&str, &[u8]); 4] = [
        ("IPA.exe", b"ipa"),
        ("IPA/Data/Managed/IPA.Injector.dll", b"injector"),
        ("IPA/Libs/0Harmony.dll", b"harmony"),
        ("IPA/winhttp.dll", b"doorstop"),
    ];
    let bsipa = make_zip(&files);
    let songcore = make_zip(&[("Plugins/SongCore.dll", b"songcore")]);
    let (b, s) = (bsipa.clone(), songcore.clone());
    let server = fake_server_with(move |_| {
        vec![
            json_response(200, r#"["1.45.3","1.40.10","1.40.8","1.40.1"]"#),
            json_response(200, &bs_mods(&files, b"songcore", false).to_string()),
            file(&b),
            file(&s),
        ]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-bs-state");
    let bs = game("beat-saber");
    st.configure(bs, folder.to_str().unwrap()).unwrap();
    let catalog = st.catalog(bs, false).await.unwrap();
    assert_eq!(catalog.game_version.as_deref(), Some("1.40.8"));
    let song = catalog.get("2").unwrap();
    assert_eq!(song.versions[0].dependencies.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["BSIPA"]);
    assert!(catalog.get("1").unwrap().loader);

    let out = st.install(bs, "2", None, &[], quiet()).await.unwrap();
    assert_eq!(out.installed, ["BSIPA", "SongCore"]);
    for (rel, body) in [
        ("IPA.exe", &b"ipa"[..]),
        ("IPA/Data/Managed/IPA.Injector.dll", b"injector"),
        ("Beat Saber_Data/Managed/IPA.Injector.dll", b"injector"),
        ("Libs/0Harmony.dll", b"harmony"),
        ("winhttp.dll", b"doorstop"),
        ("Plugins/SongCore.dll", b"songcore"),
    ] {
        assert_eq!(fs::read(folder.join(rel)).unwrap(), body, "{rel}");
    }
    assert!(games::beat_saber::bsipa_present(&folder));
    let state_ = st.loader_state(bs).await.unwrap();
    assert!(state_.present && state_.managed && state_.game_version.as_deref() == Some("1.40.8"));
}

#[tokio::test]
async fn beat_saber_refuses_a_file_that_does_not_match_beatmods() {
    let folder = bs_folder("lists-bs-tamper");
    // BSIPA déjà là (installé à la main) : seul le mod est téléchargé.
    fs::write(folder.join("winhttp.dll"), b"doorstop").unwrap();
    fs::write(folder.join("Beat Saber_Data/Managed/IPA.Injector.dll"), b"injector").unwrap();
    let songcore = make_zip(&[("Plugins/SongCore.dll", b"songcore")]);
    let s = songcore.clone();
    let server = fake_server_with(move |_| {
        vec![json_response(200, r#"["1.40.8"]"#), json_response(200, &bs_mods(&[], b"songcore", true).to_string()), file(&s)]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-bs-tamper-state");
    let bs = game("beat-saber");
    st.configure(bs, folder.to_str().unwrap()).unwrap();
    let err = st.install(bs, "2", None, &[], quiet()).await.unwrap_err();
    assert!(matches!(err, AppError::Corrupted), "{err:?}");
    assert!(!folder.join("Plugins").exists());
    assert!(!folder.join(STAGING_DIR).exists());
}

// ---------------------------------------------------------------- Outer Wilds

#[tokio::test]
async fn outer_wilds_mods_go_to_owml_with_the_dependencies_their_manifest_asks_for() {
    let folder = tempdir("lists-ow");
    fs::write(folder.join("OuterWilds.exe"), b"exe").unwrap();
    let owml_dir = tempdir("lists-ow-owml");
    let owml = make_zip(&[
        ("OWML.Launcher.exe", b"launcher"),
        ("OWML.Manifest.json", br#"{"version":"2.15.1"}"#),
        ("OWML.DefaultConfig.json", br#"{"gamePath":"","debugMode":false}"#),
    ]);
    let nh = make_zip(&[
        ("NewHorizons/manifest.json", br#"{"uniqueName":"xen.NewHorizons","dependencies":["Dep.Lib"]}"#),
        ("NewHorizons/NewHorizons.dll", b"nh"),
        ("NewHorizons/config.json", br#"{"enabled":true}"#),
    ]);
    let dep = make_zip(&[("manifest.json", br#"{"uniqueName":"Dep.Lib"}"#), ("Lib.dll", b"lib")]);
    let (o, n, d) = (owml.clone(), nh.clone(), dep.clone());
    let server = fake_server_with(move |base| {
        let base = base.trim_end_matches("/v1");
        let entry = |id: &str, name: &str, file: &str| {
            serde_json::json!({ "name": name, "uniqueName": id, "description": name, "author": "someone", "repo": "https://github.com/someone/repo",
                "version": "v1.2.0", "downloadUrl": format!("{base}/{file}"), "downloadCount": 3, "tags": ["library"], "thumbnail": { "main": "thumb.webp" } })
        };
        let db = serde_json::json!({ "releases": [entry("Alek.OWML", "OWML", "OWML.zip"), entry("xen.NewHorizons", "New Horizons", "nh.zip"), entry("Dep.Lib", "Dep Lib", "dep.zip")] });
        vec![json_response(200, &db.to_string()), file(&o), file(&n), file(&d)]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-ow-state");
    let ow = game("outer-wilds");
    st.configure(ow, folder.to_str().unwrap()).unwrap();
    // Le dossier d'OWML (celui d'Outer Wilds Mod Manager hors des tests).
    st.library.lock().unwrap().games.get_mut(&game_id("outer-wilds")).unwrap().extra_folders.insert("owml".into(), owml_dir.clone());

    let out = st.install(ow, "xen.NewHorizons", None, &[], quiet()).await.unwrap();
    assert_eq!(out.installed, ["OWML", "New Horizons", "Dep Lib"]);
    assert!(owml_dir.join("OWML.Launcher.exe").is_file());
    let config: serde_json::Value = serde_json::from_slice(&fs::read(owml_dir.join("OWML.Config.json")).unwrap()).unwrap();
    assert_eq!(config["gamePath"].as_str(), Some(folder.to_str().unwrap()));
    assert_eq!(config["debugMode"].as_bool(), Some(false));
    assert_eq!(fs::read(owml_dir.join("Mods/xen.NewHorizons/NewHorizons.dll")).unwrap(), b"nh");
    assert!(!owml_dir.join("Mods/xen.NewHorizons/config.json").exists(), "les réglages restent à l'utilisateur");
    assert_eq!(fs::read(owml_dir.join("Mods/Dep.Lib/Lib.dll")).unwrap(), b"lib");
    let dep_rec = &out.game.mods[&mod_id("outer-wilds", "Dep.Lib")];
    assert!(dep_rec.auto && dep_rec.required_by == [mod_id("outer-wilds", "xen.NewHorizons")]);
    let nh_rec = &out.game.mods[&mod_id("outer-wilds", "xen.NewHorizons")];
    assert_eq!(nh_rec.parts[0].key, "owml");
    assert_eq!(nh_rec.version.as_deref(), Some("1.2.0"));
    let state_ = st.loader_state(ow).await.unwrap();
    assert!(state_.present && state_.note.is_some_and(|n| n.contains("Lancer avec OWML")));

    // Désactivé puis désinstallé : rien ne reste.
    let gid = game_id("outer-wilds");
    st.set_enabled(gid, nh_rec.mod_id, false, &[]).unwrap();
    assert!(!owml_dir.join("Mods/xen.NewHorizons/NewHorizons.dll").exists());
    st.set_enabled(gid, nh_rec.mod_id, true, &[]).unwrap();
    assert!(owml_dir.join("Mods/xen.NewHorizons/NewHorizons.dll").is_file());
    st.remove(gid, nh_rec.mod_id).unwrap();
    assert!(!owml_dir.join("Mods/xen.NewHorizons").exists());
}

#[test]
fn versions_compare_without_semver() {
    use std::cmp::Ordering::*;
    assert_eq!(games::version_cmp("1.10.0", "1.9.2"), Greater);
    assert_eq!(games::version_cmp("v0.7.3", "0.7.3"), Equal);
    assert_eq!(games::version_cmp("1.0.2.0", "1.0.10.0"), Less);
    assert!(games::safe_file_name("BiofuelCounter v1.3.rmod") && !games::safe_file_name("../x") && !games::safe_file_name("a\\b"));
}


#[tokio::test]
async fn nothing_is_installed_without_the_loader_when_its_site_does_not_answer() {
    let folder = tempdir("lists-cvr-noloader");
    fs::write(folder.join("ChilloutVR.exe"), b"exe").unwrap();
    let server = fake_server_with(|base| {
        let base = base.trim_end_matches("/v1");
        let list = serde_json::json!([{ "_id": 5, "name": "Video Remote", "versions": [{
            "approvalStatus": 1, "name": "Video Remote", "modVersion": "1.0.0", "downloadLink": format!("{base}/v1/mods/download/5"), "hash": sha_b64(b"v")
        }]}]);
        vec![json_response(200, &list.to_string()), json_response(403, r#"{"message":"API rate limit exceeded"}"#)]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-cvr-noloader-state");
    let cvr = game("chilloutvr");
    st.configure(cvr, folder.to_str().unwrap()).unwrap();
    let err = st.install(cvr, "5", None, &[], quiet()).await.unwrap_err();
    assert!(matches!(err, AppError::ListUnavailable { ref list, .. } if list == "MelonLoader"), "{err:?}");
    assert!(!folder.join("Mods").exists());
}

#[tokio::test]
async fn beat_saber_refuses_files_beatmods_does_not_list() {
    let folder = bs_folder("lists-bs-extra");
    fs::write(folder.join("winhttp.dll"), b"doorstop").unwrap();
    fs::write(folder.join("Beat Saber_Data/Managed/IPA.Injector.dll"), b"injector").unwrap();
    let songcore = make_zip(&[("Plugins/SongCore.dll", b"songcore"), ("Plugins/Extra.dll", b"not listed")]);
    let s = songcore.clone();
    let server = fake_server_with(move |_| {
        vec![json_response(200, r#"["1.40.8"]"#), json_response(200, &bs_mods(&[], b"songcore", false).to_string()), file(&s)]
    });
    let st = state(server.base.trim_end_matches("/v1"), "lists-bs-extra-state");
    let bs = game("beat-saber");
    st.configure(bs, folder.to_str().unwrap()).unwrap();
    let err = st.install(bs, "2", None, &[], quiet()).await.unwrap_err();
    assert!(matches!(err, AppError::Corrupted), "{err:?}");
    assert!(!folder.join("Plugins").exists());
}
