//! Parcourir Nexus Mods : jeux, mods, fiches et fichiers (API GraphQL publique).

use serde::Deserialize;
use serde_json::{json, Value};

use super::models::{NexusFile, NexusGame, NexusMod, NexusModDetail, RawModDetail};
use super::NexusState;
use crate::api::models::{lenient_vec, Page};
use crate::error::{AppError, AppResult};

const GAME_FIELDS: &str = "id name domainName modCount downloadCount genre";
const MOD_FIELDS: &str = "modId uid name summary author adultContent endorsements downloads updatedAt createdAt \
     version category thumbnailUrl pictureUrl status uploader { name avatar memberId } game { id domainName name }";
const FILE_FIELDS: &str = "fileId uid name version category date sizeInBytes size description primary uri";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "T: serde::de::DeserializeOwned"))]
struct Nodes<T> {
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    nodes: Vec<T>,
    #[serde(default)]
    total_count: Option<i64>,
}

/// Identifiant texte d'un jeu sur Nexus (`skyrimspecialedition`).
pub fn check_domain(domain: &str) -> AppResult<()> {
    let ok = !domain.is_empty()
        && domain.len() <= 64
        && domain.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(format!("jeu « {domain} »")))
    }
}

fn check_id(id: i64) -> AppResult<()> {
    if id > 0 { Ok(()) } else { Err(AppError::InvalidArgument(format!("identifiant {id}"))) }
}

fn clamp(limit: u32) -> u32 {
    limit.clamp(1, 100)
}

fn page<T>(nodes: Nodes<T>, offset: u32, limit: u32) -> Page<T> {
    let count = nodes.nodes.len() as u32;
    Page {
        result_total: nodes.total_count.unwrap_or(count as i64).max(0) as u32,
        data: nodes.nodes,
        result_count: count,
        result_offset: offset,
        result_limit: limit,
    }
}

fn search_term(query: Option<String>) -> Option<String> {
    query.map(|q| q.trim().to_string()).filter(|q| !q.is_empty()).map(|q| q.chars().take(100).collect())
}

/// Tri des jeux : `downloads` (défaut), `mods`, `name`.
fn game_sort(sort: Option<&str>) -> AppResult<Value> {
    Ok(match sort.unwrap_or("downloads") {
        "downloads" => json!([{ "downloads": { "direction": "DESC" } }]),
        "mods" => json!([{ "mods": { "direction": "DESC" } }]),
        "name" => json!([{ "name": { "direction": "ASC" } }]),
        other => return Err(AppError::InvalidArgument(format!("tri « {other} »"))),
    })
}

/// Tri des mods : `endorsements` (défaut), `downloads`, `updated`, `created`, `name`.
fn mod_sort(sort: Option<&str>) -> AppResult<Value> {
    Ok(match sort.unwrap_or("endorsements") {
        "endorsements" => json!([{ "endorsements": { "direction": "DESC" } }]),
        "downloads" => json!([{ "downloads": { "direction": "DESC" } }]),
        "updated" => json!([{ "updatedAt": { "direction": "DESC" } }]),
        "created" => json!([{ "createdAt": { "direction": "DESC" } }]),
        "name" => json!([{ "name": { "direction": "ASC" } }]),
        other => return Err(AppError::InvalidArgument(format!("tri « {other} »"))),
    })
}

impl NexusState {
    pub async fn list_games(
        &self,
        query: Option<String>,
        sort: Option<String>,
        offset: u32,
        limit: u32,
    ) -> AppResult<Page<NexusGame>> {
        let limit = clamp(limit);
        let filter = search_term(query).map(|q| json!({ "name": [{ "value": q, "op": "WILDCARD" }] }));
        let gql = format!(
            "query Games($count: Int, $offset: Int, $filter: GamesSearchFilter, $sort: [GamesSearchSort!]) \
             {{ games(count: $count, offset: $offset, filter: $filter, sort: $sort) {{ totalCount nodes {{ {GAME_FIELDS} }} }} }}"
        );
        #[derive(Deserialize)]
        struct Data {
            games: Nodes<NexusGame>,
        }
        let mut variables = json!({ "count": limit, "offset": offset, "sort": game_sort(sort.as_deref())? });
        // Un filtre `null` est refusé par l'API : on l'omet.
        if let Some(f) = filter {
            variables["filter"] = f;
        }
        let data: Data = self.client.graphql(&gql, variables).await?;
        Ok(page(data.games, offset, limit))
    }

    pub async fn get_game(&self, domain: &str) -> AppResult<NexusGame> {
        check_domain(domain)?;
        let gql = format!("query Game($domain: String) {{ game(domainName: $domain) {{ {GAME_FIELDS} }} }}");
        #[derive(Deserialize)]
        struct Data {
            game: Option<NexusGame>,
        }
        let data: Data = self.client.graphql(&gql, json!({ "domain": domain })).await?;
        data.game.filter(|g| g.id > 0).ok_or(AppError::NexusNotFound)
    }

    pub async fn list_mods(
        &self,
        game_domain: &str,
        query: Option<String>,
        sort: Option<String>,
        hide_adult: bool,
        offset: u32,
        limit: u32,
    ) -> AppResult<Page<NexusMod>> {
        check_domain(game_domain)?;
        let limit = clamp(limit);
        let mut filter = json!({ "gameDomainName": [{ "value": game_domain, "op": "EQUALS" }] });
        if let Some(q) = search_term(query) {
            filter["name"] = json!([{ "value": q, "op": "WILDCARD" }]);
        }
        if hide_adult {
            filter["adultContent"] = json!([{ "value": false, "op": "EQUALS" }]);
        }
        let gql = format!(
            "query Mods($count: Int, $offset: Int, $filter: ModsFilter, $sort: [ModsSort!]) \
             {{ mods(count: $count, offset: $offset, filter: $filter, sort: $sort) {{ totalCount nodes {{ {MOD_FIELDS} }} }} }}"
        );
        #[derive(Deserialize)]
        struct Data {
            mods: Nodes<NexusMod>,
        }
        let data: Data = self
            .client
            .graphql(
                &gql,
                json!({ "count": limit, "offset": offset, "filter": filter, "sort": mod_sort(sort.as_deref())? }),
            )
            .await?;
        Ok(page(data.mods, offset, limit))
    }

    /// Fiches de plusieurs mods, d'après (jeu, mod), par lots de 50.
    pub async fn mods_by_ids(&self, ids: &[(i64, i64)]) -> AppResult<Vec<NexusMod>> {
        let gql = format!("query Mods($ids: [CompositeIdInput!]!) {{ legacyMods(ids: $ids) {{ nodes {{ {MOD_FIELDS} }} }} }}");
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            legacy_mods: Nodes<NexusMod>,
        }
        let mut out = Vec::new();
        for chunk in ids.chunks(50) {
            let list: Vec<Value> = chunk.iter().map(|(g, m)| json!({ "gameId": g, "modId": m })).collect();
            let data: Data = self.client.graphql(&gql, json!({ "ids": list })).await?;
            out.extend(data.legacy_mods.nodes);
        }
        Ok(out)
    }

    pub async fn get_mod(&self, game_id: i64, mod_id: i64) -> AppResult<NexusModDetail> {
        check_id(game_id)?;
        check_id(mod_id)?;
        let gql = format!(
            "query Mod($ids: [CompositeIdInput!]!) {{ legacyMods(ids: $ids) {{ nodes {{ {MOD_FIELDS} description \
             modRequirements {{ nexusRequirements {{ nodes {{ modId gameId modName url externalRequirement notes }} }} }} }} }} }}"
        );
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            legacy_mods: Nodes<RawModDetail>,
        }
        let data: Data = self
            .client
            .graphql(&gql, json!({ "ids": [{ "gameId": game_id, "modId": mod_id }] }))
            .await?;
        data.legacy_mods
            .nodes
            .into_iter()
            .find(|m| m.summary.mod_id == mod_id)
            .map(NexusModDetail::from)
            .ok_or(AppError::NexusNotFound)
    }

    pub async fn list_files(&self, game_id: i64, mod_id: i64) -> AppResult<Vec<NexusFile>> {
        check_id(game_id)?;
        check_id(mod_id)?;
        let gql = format!("query Files($modId: ID!, $gameId: ID!) {{ modFiles(modId: $modId, gameId: $gameId) {{ {FILE_FIELDS} }} }}");
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
            mod_files: Vec<NexusFile>,
        }
        let data: Data = self
            .client
            .graphql(&gql, json!({ "modId": mod_id.to_string(), "gameId": game_id.to_string() }))
            .await?;
        let mut files = data.mod_files;
        files.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(files)
    }

    /// Catégorie actuelle de fichiers déjà installés (repérer les versions remplacées).
    pub async fn files_by_uid(&self, uids: &[String]) -> AppResult<Vec<NexusFile>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let gql = format!(
            "query FilesByUid($uids: [ID!]!, $count: Int) {{ modFilesByUid(uids: $uids, count: $count) {{ nodes {{ {FILE_FIELDS} }} }} }}"
        );
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            mod_files_by_uid: Nodes<NexusFile>,
        }
        let mut out = Vec::new();
        for chunk in uids.chunks(100) {
            let data: Data = self
                .client
                .graphql(&gql, json!({ "uids": chunk, "count": chunk.len() }))
                .await?;
            out.extend(data.mod_files_by_uid.nodes);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::installer::tests::tempdir;
    use crate::testutil::{fake_server, json_response};

    fn state(base: &str) -> NexusState {
        NexusState::new(base.trim_end_matches("/v1"), Box::new(MemoryStore::default()), tempdir("nx-br").join("n.json"))
    }

    #[test]
    fn rejects_odd_domains_and_sorts() {
        assert!(check_domain("skyrimspecialedition").is_ok());
        assert!(check_domain("bonelab").is_ok());
        assert!(check_domain("Bone Lab").is_err());
        assert!(check_domain("../x").is_err());
        assert!(mod_sort(Some("x;drop")).is_err());
        assert!(game_sort(None).is_ok());
    }

    #[tokio::test]
    async fn lists_mods_with_filters() {
        let server = fake_server(vec![json_response(
            200,
            r#"{"data":{"mods":{"totalCount":138,"nodes":[{"modId":148,"name":"7-11 SANDBOX","downloads":16884,"adultContent":false,"game":{"id":4832,"domainName":"bonelab"}}]}}}"#,
        )]);
        let s = state(&server.base);
        let p = s
            .list_mods("bonelab", Some("  halo ".into()), Some("downloads".into()), true, 30, 30)
            .await
            .unwrap();
        assert_eq!((p.result_total, p.result_offset, p.data[0].mod_id), (138, 30, 148));
        let body: Value = serde_json::from_str(&server.request(0).body).unwrap();
        let v = &body["variables"];
        assert_eq!(v["filter"]["gameDomainName"][0]["value"], "bonelab");
        assert_eq!(v["filter"]["name"][0]["value"], "halo");
        assert_eq!(v["filter"]["adultContent"][0]["value"], false);
        assert_eq!(v["sort"][0]["downloads"]["direction"], "DESC");
        assert_eq!(v["offset"], 30);
    }

    #[tokio::test]
    async fn game_list_omits_an_empty_filter() {
        let body = r#"{"data":{"games":{"totalCount":5695,"nodes":[{"id":1704,"name":"Skyrim Special Edition","domainName":"skyrimspecialedition","downloadCount":"13624857243"}]}}}"#;
        let server = fake_server(vec![json_response(200, body), json_response(200, body)]);
        let s = state(&server.base);
        let p = s.list_games(None, None, 0, 48).await.unwrap();
        assert_eq!((p.result_total, p.data[0].download_count), (5695, 13624857243));
        let sent: Value = serde_json::from_str(&server.request(0).body).unwrap();
        assert!(sent["variables"].get("filter").is_none(), "{sent}");
        s.list_games(Some("bone".into()), Some("name".into()), 0, 48).await.unwrap();
        let sent: Value = serde_json::from_str(&server.request(1).body).unwrap();
        assert_eq!(sent["variables"]["filter"]["name"][0]["value"], "bone");
        assert_eq!(sent["variables"]["sort"][0]["name"]["direction"], "ASC");
    }

    #[tokio::test]
    async fn reads_mod_details_and_files() {
        let server = fake_server(vec![
            json_response(
                200,
                r#"{"data":{"legacyMods":{"nodes":[{"modId":148,"name":"7-11","description":"x","modRequirements":{"nexusRequirements":{"nodes":[]}}}]}}}"#,
            ),
            json_response(
                200,
                r#"{"data":{"modFiles":[{"fileId":1,"name":"old","category":"OLD_VERSION","date":10},{"fileId":2,"name":"new","category":"MAIN","date":20}]}}"#,
            ),
            json_response(200, r#"{"data":{"legacyMods":{"nodes":[]}}}"#),
        ]);
        let s = state(&server.base);
        let m = s.get_mod(4832, 148).await.unwrap();
        assert_eq!(m.summary.name, "7-11");
        let files = s.list_files(4832, 148).await.unwrap();
        assert_eq!(files[0].file_id, 2, "le plus récent d'abord");
        let body: Value = serde_json::from_str(&server.request(1).body).unwrap();
        assert_eq!(body["variables"]["modId"], "148");
        assert!(matches!(s.get_mod(4832, 9).await, Err(AppError::NexusNotFound)));
    }
}
