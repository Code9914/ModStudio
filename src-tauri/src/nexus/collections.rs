//! Collections Nexus Mods : parcourir celles d'un jeu, lire leurs mods, et
//! l'ordre dans lequel la collection les range (fichier `collection.json` de
//! son archive) : sur un fichier que deux de ses mods installent, celui qui
//! vient après gagne (voir `layers.rs`).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::browse::check_domain;
use super::models::{flag, number, NexusGameRef, NexusUploader};
use super::NexusState;
use crate::api::models::{lenient, lenient_vec, Page};
use crate::archive;
use crate::error::{AppError, AppResult};
use crate::installer::{self, CollectionMark, DownloadRequest, GameInstall};

/// Archive d'une collection lue au plus (elle contient surtout `collection.json`).
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;

/// Identifiant texte d'une collection (`tsdxpq`).
pub fn check_slug(slug: &str) -> AppResult<()> {
    let ok = !slug.is_empty() && slug.len() <= 64 && slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(crate::tr!("collection « {slug} »", "collection \"{slug}\"")))
    }
}

// ---------------------------------------------------------------- Réponses de l'API

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawImage {
    #[serde(default, deserialize_with = "lenient")]
    url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Named {
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFileMod {
    #[serde(default, deserialize_with = "number")]
    mod_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    name: String,
    #[serde(default, deserialize_with = "lenient")]
    thumbnail_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    picture_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    author: Option<String>,
    #[serde(default, deserialize_with = "flag")]
    adult_content: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawModFile {
    #[serde(default, deserialize_with = "lenient")]
    name: String,
    #[serde(default, deserialize_with = "lenient")]
    version: Option<String>,
    #[serde(default, deserialize_with = "number")]
    size_in_bytes: i64,
    /// En Ko.
    #[serde(default, deserialize_with = "number")]
    size: i64,
    #[serde(default, deserialize_with = "lenient")]
    r#mod: Option<RawFileMod>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRevisionMod {
    #[serde(default, deserialize_with = "number")]
    file_id: i64,
    #[serde(default, deserialize_with = "flag")]
    optional: bool,
    #[serde(default, deserialize_with = "lenient")]
    file: Option<RawModFile>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExternal {
    #[serde(default, deserialize_with = "lenient")]
    name: String,
    #[serde(default, deserialize_with = "lenient")]
    resource_type: String,
    #[serde(default, deserialize_with = "lenient")]
    resource_url: Option<String>,
    #[serde(default, deserialize_with = "flag")]
    optional: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRevision {
    #[serde(default, deserialize_with = "number")]
    revision_number: i64,
    #[serde(default, deserialize_with = "number")]
    mod_count: i64,
    #[serde(default, deserialize_with = "number")]
    total_size: i64,
    #[serde(default, deserialize_with = "number")]
    file_size: i64,
    #[serde(default, deserialize_with = "lenient")]
    installation_info: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    download_link: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    mod_files: Vec<RawRevisionMod>,
    #[serde(default, deserialize_with = "lenient_vec")]
    external_resources: Vec<RawExternal>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCollection {
    #[serde(default, deserialize_with = "lenient")]
    slug: String,
    #[serde(default, deserialize_with = "lenient")]
    name: String,
    #[serde(default, deserialize_with = "lenient")]
    summary: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "number")]
    endorsements: i64,
    #[serde(default, deserialize_with = "number")]
    total_downloads: i64,
    #[serde(default, deserialize_with = "flag")]
    adult_content: bool,
    #[serde(default, deserialize_with = "lenient")]
    category: Option<Named>,
    #[serde(default, deserialize_with = "lenient")]
    user: Option<NexusUploader>,
    #[serde(default, deserialize_with = "lenient")]
    game: Option<NexusGameRef>,
    #[serde(default, deserialize_with = "lenient")]
    tile_image: Option<RawImage>,
    #[serde(default, deserialize_with = "lenient")]
    header_image: Option<RawImage>,
    #[serde(default, deserialize_with = "lenient")]
    latest_published_revision: Option<RawRevision>,
}

// ---------------------------------------------------------------- Pour l'interface

/// Collection telle qu'affichée dans la liste d'un jeu.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionCard {
    pub slug: String,
    pub name: String,
    pub summary: Option<String>,
    pub endorsements: i64,
    pub downloads: i64,
    pub image: Option<String>,
    pub author: Option<String>,
    pub author_avatar: Option<String>,
    pub category: Option<String>,
    pub adult: bool,
    pub revision: i64,
    pub mod_count: i64,
    /// Taille de tous ses mods, en octets.
    pub size: i64,
}

/// Mod d'une collection (un fichier précis de ce mod).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionMod {
    pub mod_id: i64,
    pub file_id: i64,
    pub name: String,
    pub file_name: String,
    pub version: Option<String>,
    pub size: i64,
    pub optional: bool,
    pub image: Option<String>,
    pub author: Option<String>,
    pub adult: bool,
}

/// Ressource à récupérer hors de Nexus Mods (site d'un outil…).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalResource {
    pub name: String,
    pub url: Option<String>,
    pub kind: String,
    pub optional: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDetail {
    #[serde(flatten)]
    pub card: CollectionCard,
    pub description: Option<String>,
    pub header: Option<String>,
    pub game: NexusGameRef,
    pub install_info: Option<String>,
    pub mods: Vec<CollectionMod>,
    pub external: Vec<ExternalResource>,
}

impl RawCollection {
    fn card(&self) -> CollectionCard {
        let rev = self.latest_published_revision.clone().unwrap_or_default();
        let image = self.tile_image.as_ref().and_then(|i| i.thumbnail_url.clone().or(i.url.clone()));
        CollectionCard {
            slug: self.slug.clone(),
            name: self.name.clone(),
            summary: self.summary.clone().filter(|s| !s.trim().is_empty()),
            endorsements: self.endorsements,
            downloads: self.total_downloads,
            image,
            author: self.user.as_ref().and_then(|u| u.name.clone()),
            author_avatar: self.user.as_ref().and_then(|u| u.avatar.clone()),
            category: self.category.as_ref().and_then(|c| c.name.clone()),
            adult: self.adult_content,
            revision: rev.revision_number,
            mod_count: rev.mod_count,
            size: rev.total_size,
        }
    }
}

fn collection_mod(raw: &RawRevisionMod) -> Option<CollectionMod> {
    let file = raw.file.as_ref()?;
    let m = file.r#mod.as_ref()?;
    (m.mod_id > 0 && raw.file_id > 0).then(|| CollectionMod {
        mod_id: m.mod_id,
        file_id: raw.file_id,
        name: m.name.clone(),
        file_name: file.name.clone(),
        version: file.version.clone().filter(|v| !v.trim().is_empty()),
        size: if file.size_in_bytes > 0 { file.size_in_bytes } else { file.size.saturating_mul(1024) },
        optional: raw.optional,
        image: m.thumbnail_url.clone().or(m.picture_url.clone()),
        author: m.author.clone(),
        adult: m.adult_content,
    })
}

const CARD_FIELDS: &str = "slug name summary endorsements totalDownloads adultContent category { name } \
     user { name avatar memberId } tileImage { url thumbnailUrl(size: med) } \
     latestPublishedRevision { revisionNumber modCount totalSize }";

/// Tri des collections : `endorsements` (défaut), `downloads`, `updated`, `rating`.
fn collection_sort(sort: Option<&str>) -> AppResult<Value> {
    Ok(match sort.unwrap_or("endorsements") {
        "endorsements" => json!([{ "endorsements": { "direction": "DESC" } }]),
        "downloads" => json!([{ "downloads": { "direction": "DESC" } }]),
        "updated" => json!([{ "updatedAt": { "direction": "DESC" } }]),
        "rating" => json!([{ "rating": { "direction": "DESC" } }]),
        other => return Err(AppError::InvalidArgument(crate::tr!("tri « {other} »", "sort \"{other}\""))),
    })
}

// ---------------------------------------------------------------- Ordre d'installation

/// Ordre de la collection, et ce qu'elle demande en dehors de Nexus Mods.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionPlan {
    /// Mods Nexus du jeu, dans l'ordre où les installer (rang croissant).
    pub order: Vec<PlanEntry>,
    /// À faire à la main : fichiers fournis par la collection, sites externes, autres jeux.
    pub extras: Vec<PlanExtra>,
    /// `collection.json` lu : l'ordre vient de ses règles (sinon, celui de la liste).
    pub from_manifest: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub mod_id: i64,
    pub file_id: i64,
    pub rank: u32,
    /// Conseils du créateur pour ce mod.
    pub instructions: Option<String>,
    /// Options d'installation choisies par le créateur (installateur FOMOD), lisibles.
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanExtra {
    pub name: String,
    /// `bundle`, `browse`, `manual`, `direct` ou `other_game`.
    pub kind: String,
    pub url: Option<String>,
    pub instructions: Option<String>,
    pub optional: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSource {
    #[serde(default, deserialize_with = "lenient")]
    r#type: String,
    #[serde(default, deserialize_with = "number")]
    mod_id: i64,
    #[serde(default, deserialize_with = "number")]
    file_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    md5: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    logical_filename: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    file_expression: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    tag: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    instructions: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestMod {
    #[serde(default, deserialize_with = "lenient")]
    name: String,
    #[serde(default, deserialize_with = "flag")]
    optional: bool,
    #[serde(default, deserialize_with = "lenient")]
    domain_name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    source: ManifestSource,
    #[serde(default, deserialize_with = "lenient")]
    instructions: Option<String>,
    #[serde(default)]
    choices: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoRef {
    #[serde(default, deserialize_with = "lenient")]
    mod_id: Option<Value>,
    #[serde(default, deserialize_with = "lenient")]
    file_id: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModRef {
    #[serde(default, deserialize_with = "lenient", rename = "fileMD5")]
    file_md5: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    logical_file_name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    file_expression: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    tag: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    repo: Option<RepoRef>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ManifestRule {
    #[serde(default, deserialize_with = "lenient")]
    source: ModRef,
    #[serde(default, deserialize_with = "lenient")]
    r#type: String,
    #[serde(default, deserialize_with = "lenient")]
    reference: ModRef,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(default, deserialize_with = "lenient_vec")]
    mods: Vec<ManifestMod>,
    #[serde(default, deserialize_with = "lenient_vec")]
    mod_rules: Vec<ManifestRule>,
}

fn id_of(v: &Option<Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// Mod de la collection que désigne une règle.
fn find_ref(mods: &[ManifestMod], r: &ModRef) -> Option<usize> {
    let same = |a: &Option<String>, b: &Option<String>| match (a, b) {
        (Some(x), Some(y)) => !x.is_empty() && x.eq_ignore_ascii_case(y),
        _ => false,
    };
    let repo_mod = r.repo.as_ref().map(|x| id_of(&x.mod_id)).unwrap_or(0);
    let repo_file = r.repo.as_ref().map(|x| id_of(&x.file_id)).unwrap_or(0);
    mods.iter()
        .position(|m| repo_file > 0 && m.source.file_id == repo_file)
        .or_else(|| mods.iter().position(|m| same(&m.source.md5, &r.file_md5)))
        .or_else(|| mods.iter().position(|m| same(&m.source.tag, &r.tag)))
        .or_else(|| mods.iter().position(|m| same(&m.source.logical_filename, &r.logical_file_name)))
        .or_else(|| mods.iter().position(|m| same(&m.source.file_expression, &r.file_expression)))
        .or_else(|| mods.iter().position(|m| repo_mod > 0 && repo_file == 0 && m.source.mod_id == repo_mod))
}

/// Ordre des mods d'après les règles « avant / après » (tri topologique ;
/// à égalité, l'ordre de la liste). Une boucle de règles est rompue dans
/// l'ordre de la liste.
fn ranked(manifest: &Manifest) -> Vec<usize> {
    let n = manifest.mods.len();
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming = vec![0usize; n];
    for rule in &manifest.mod_rules {
        let (Some(a), Some(b)) = (find_ref(&manifest.mods, &rule.source), find_ref(&manifest.mods, &rule.reference)) else { continue };
        if a == b {
            continue;
        }
        let (first, then) = match rule.r#type.as_str() {
            "before" => (a, b),
            "after" => (b, a),
            _ => continue,
        };
        if !next[first].contains(&then) {
            next[first].push(then);
            incoming[then] += 1;
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..n).filter(|&i| incoming[i] == 0).map(Reverse).collect();
    let mut order = Vec::with_capacity(n);
    let mut placed = vec![false; n];
    while order.len() < n {
        let i = match ready.pop() {
            Some(Reverse(i)) => i,
            // Boucle : le premier mod restant de la liste passe.
            None => (0..n).find(|&i| !placed[i]).expect("il reste des mods"),
        };
        if placed[i] {
            continue;
        }
        placed[i] = true;
        order.push(i);
        for &j in &next[i] {
            incoming[j] = incoming[j].saturating_sub(1);
            if incoming[j] == 0 && !placed[j] {
                ready.push(Reverse(j));
            }
        }
    }
    order
}

/// Options FOMOD choisies par le créateur, en lignes « Étape › Groupe : option ».
fn choice_lines(choices: Option<&Value>) -> Vec<String> {
    let Some(c) = choices else { return Vec::new() };
    if c.get("type").and_then(Value::as_str) != Some("fomod") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for step in c.get("options").and_then(Value::as_array).into_iter().flatten() {
        let step_name = step.get("name").and_then(Value::as_str).unwrap_or_default();
        for group in step.get("groups").and_then(Value::as_array).into_iter().flatten() {
            let group_name = group.get("name").and_then(Value::as_str).unwrap_or_default();
            let picked: Vec<&str> =
                group.get("choices").and_then(Value::as_array).into_iter().flatten().filter_map(|x| x.get("name").and_then(Value::as_str)).collect();
            if !picked.is_empty() {
                out.push(format!("{step_name} › {group_name} : {}", picked.join(", ")));
            }
        }
    }
    out.truncate(40);
    out
}

/// Ordre d'installation d'après `collection.json` (mods Nexus du jeu `domain`).
fn plan_from_manifest(manifest: &Manifest, domain: &str) -> CollectionPlan {
    let mut plan = CollectionPlan { from_manifest: true, ..Default::default() };
    let mut rank = 0u32;
    for i in ranked(manifest) {
        let m = &manifest.mods[i];
        let src = &m.source;
        let other_game = m.domain_name.as_deref().is_some_and(|d| !d.is_empty() && d != domain);
        let instructions = m.instructions.clone().or(src.instructions.clone()).filter(|s| !s.trim().is_empty());
        if src.r#type == "nexus" && !other_game && src.mod_id > 0 && src.file_id > 0 {
            rank += 1;
            plan.order.push(PlanEntry {
                mod_id: src.mod_id,
                file_id: src.file_id,
                rank,
                instructions,
                choices: choice_lines(m.choices.as_ref()),
            });
        } else {
            let kind = if other_game && src.r#type == "nexus" { "other_game".to_string() } else { src.r#type.clone() };
            plan.extras.push(PlanExtra {
                name: m.name.clone(),
                kind,
                url: src.url.clone().filter(|u| u.starts_with("https://") || u.starts_with("http://")),
                instructions,
                optional: m.optional,
            });
        }
    }
    plan
}

/// Sans `collection.json` : l'ordre de la liste de la collection.
fn plan_from_list(mods: &[CollectionMod]) -> CollectionPlan {
    CollectionPlan {
        order: mods
            .iter()
            .enumerate()
            .map(|(i, m)| PlanEntry { mod_id: m.mod_id, file_id: m.file_id, rank: i as u32 + 1, instructions: None, choices: Vec::new() })
            .collect(),
        extras: Vec::new(),
        from_manifest: false,
    }
}

/// Chemin REST du lien de téléchargement d'une archive de collection.
fn download_path(link: &str) -> Option<String> {
    let path = link.trim();
    let path = path.strip_prefix("https://api.nexusmods.com").unwrap_or(path);
    let path = path.strip_prefix("/v1").unwrap_or(path);
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let ok = parts.len() == 5
        && parts[0] == "collections"
        && check_slug(parts[1]).is_ok()
        && parts[2] == "revisions"
        && !parts[3].is_empty()
        && parts[3].chars().all(|c| c.is_ascii_digit())
        && parts[4] == "download_link";
    ok.then(|| format!("/{}", parts.join("/")))
}

/// Lit `collection.json` dans une archive extraite (à la racine, sinon un niveau plus bas).
fn read_manifest(dir: &Path) -> Option<Manifest> {
    let direct = dir.join("collection.json");
    let path = if direct.is_file() {
        direct
    } else {
        fs::read_dir(dir).ok()?.flatten().map(|e| e.path().join("collection.json")).find(|p| p.is_file())?
    };
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

impl NexusState {
    /// Collections d'un jeu, publiques et listées.
    pub async fn list_collections(
        &self,
        domain: &str,
        query: Option<String>,
        sort: Option<String>,
        hide_adult: bool,
        offset: u32,
        limit: u32,
    ) -> AppResult<Page<CollectionCard>> {
        check_domain(domain)?;
        let limit = limit.clamp(1, 60);
        let mut filter = json!({
            "op": "AND",
            "gameDomain": [{ "value": domain, "op": "EQUALS" }],
            "collectionStatus": [{ "value": "listed", "op": "EQUALS" }],
        });
        if let Some(q) = query.map(|q| q.trim().chars().take(100).collect::<String>()).filter(|q| !q.is_empty()) {
            filter["generalSearch"] = json!([{ "value": q, "op": "WILDCARD" }]);
        }
        if hide_adult {
            filter["adultContent"] = json!([{ "value": false, "op": "EQUALS" }]);
        }
        let gql = format!(
            "query Collections($count: Int, $offset: Int, $filter: CollectionsSearchFilter, $sort: [CollectionsSearchSort!]) \
             {{ collectionsV2(count: $count, offset: $offset, filter: $filter, sort: $sort) {{ totalCount nodes {{ {CARD_FIELDS} }} }} }}"
        );
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Nodes {
            #[serde(default, deserialize_with = "lenient_vec")]
            nodes: Vec<RawCollection>,
            #[serde(default, deserialize_with = "number")]
            total_count: i64,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            collections_v2: Nodes,
        }
        let variables = json!({ "count": limit, "offset": offset, "filter": filter, "sort": collection_sort(sort.as_deref())? });
        let data: Data = self.client.graphql(&gql, variables).await?;
        let cards: Vec<CollectionCard> = data.collections_v2.nodes.iter().filter(|c| check_slug(&c.slug).is_ok()).map(RawCollection::card).collect();
        Ok(Page {
            result_count: cards.len() as u32,
            result_total: data.collections_v2.total_count.max(0) as u32,
            data: cards,
            result_offset: offset,
            result_limit: limit,
        })
    }

    async fn raw_collection(&self, slug: &str, domain: &str) -> AppResult<RawCollection> {
        check_slug(slug)?;
        check_domain(domain)?;
        let gql = format!(
            "query Collection($slug: String, $domain: String) {{ collection(slug: $slug, domainName: $domain, viewAdultContent: true) {{ \
             {CARD_FIELDS} description game {{ id domainName name }} headerImage {{ url }} \
             latestPublishedRevision {{ revisionNumber modCount totalSize fileSize installationInfo downloadLink \
               modFiles {{ fileId optional file {{ name version sizeInBytes size mod {{ modId name thumbnailUrl pictureUrl author adultContent }} }} }} \
               externalResources {{ name resourceType resourceUrl optional }} }} }} }}"
        );
        #[derive(Deserialize)]
        struct Data {
            #[serde(default, deserialize_with = "lenient")]
            collection: Option<RawCollection>,
        }
        let data: Data = self.client.graphql(&gql, json!({ "slug": slug, "domain": domain })).await?;
        data.collection.filter(|c| c.slug == slug && c.latest_published_revision.is_some()).ok_or(AppError::NexusNotFound)
    }

    /// Fiche d'une collection et liste de ses mods (dernière version publiée).
    pub async fn get_collection(&self, slug: &str, domain: &str) -> AppResult<CollectionDetail> {
        let raw = self.raw_collection(slug, domain).await?;
        let rev = raw.latest_published_revision.clone().unwrap_or_default();
        Ok(CollectionDetail {
            card: raw.card(),
            description: raw.description.clone().filter(|d| !d.trim().is_empty()),
            header: raw.header_image.as_ref().and_then(|i| i.url.clone()),
            game: raw.game.clone().unwrap_or_default(),
            install_info: rev.installation_info.clone().filter(|s| !s.trim().is_empty()),
            mods: rev.mod_files.iter().filter_map(collection_mod).collect(),
            external: rev
                .external_resources
                .iter()
                .map(|e| ExternalResource {
                    name: e.name.clone(),
                    url: e.resource_url.clone().filter(|u| u.starts_with("https://") || u.starts_with("http://")),
                    kind: e.resource_type.clone(),
                    optional: e.optional,
                })
                .collect(),
        })
    }

    /// Ordre d'installation d'une collection : celui de ses règles (archive de
    /// la collection), sinon celui de sa liste.
    pub async fn collection_plan(&self, slug: &str, domain: &str, revision: i64) -> AppResult<CollectionPlan> {
        let raw = self.raw_collection(slug, domain).await?;
        let rev = raw.latest_published_revision.clone().unwrap_or_default();
        if revision > 0 && rev.revision_number != revision {
            return Err(AppError::InvalidArgument(crate::tr!(
                "la collection a changé depuis l'ouverture de sa page : recharge-la.",
                "the collection changed since its page was opened: reload it."
            )));
        }
        let mods: Vec<CollectionMod> = rev.mod_files.iter().filter_map(collection_mod).collect();
        let manifest = match rev.download_link.as_deref().and_then(download_path) {
            Some(path) if rev.file_size <= MAX_ARCHIVE as i64 => self.download_manifest(&path, slug, rev.revision_number).await.ok(),
            _ => None,
        };
        let mut plan = match manifest {
            Some(m) => plan_from_manifest(&m, domain),
            None => plan_from_list(&mods),
        };
        // Mods de la liste absents de `collection.json` : après les autres, dans l'ordre de la liste.
        let mut rank = plan.order.iter().map(|e| e.rank).max().unwrap_or(0);
        for m in &mods {
            if !plan.order.iter().any(|e| e.file_id == m.file_id) {
                rank += 1;
                plan.order.push(PlanEntry { mod_id: m.mod_id, file_id: m.file_id, rank, instructions: None, choices: Vec::new() });
            }
        }
        // Seuls les fichiers de la liste publiée s'installent, et un seul par mod
        // (ModStudio garde un fichier par mod) : le premier de la liste.
        let mut first: HashMap<i64, i64> = HashMap::new();
        for m in &mods {
            first.entry(m.mod_id).or_insert(m.file_id);
        }
        plan.order.retain(|e| first.get(&e.mod_id) == Some(&e.file_id));
        for m in mods.iter().filter(|m| first.get(&m.mod_id) != Some(&m.file_id)) {
            plan.extras.push(PlanExtra {
                name: format!("{} — {}", m.name, m.file_name),
                kind: "second_file".into(),
                url: Some(format!("https://www.nexusmods.com/{domain}/mods/{}?tab=files&file_id={}", m.mod_id, m.file_id)),
                instructions: None,
                optional: m.optional,
            });
        }
        Ok(plan)
    }

    async fn download_manifest(&self, path: &str, slug: &str, revision: i64) -> AppResult<Manifest> {
        let key = self.key()?;
        let links: Value = self.client.rest(path, &key, &[]).await?;
        let uri = links
            .get("download_links")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(links.get("download_link"))
            .filter_map(|l| l.get("URI").and_then(Value::as_str))
            .find(|u| super::client::download_url_allowed(u, self.client.base()))
            .map(str::to_string)
            .ok_or_else(|| AppError::DownloadRefused(crate::tr!("aucun lien de téléchargement Nexus Mods valable", "no valid Nexus Mods download link")))?;
        let dir = self.library_path.with_file_name("collection-cache").join(format!("{slug}-{revision}"));
        let _ = fs::remove_dir_all(&dir);
        let archive_path = dir.join("collection.part");
        let cancel = AtomicBool::new(false);
        let result = async {
            installer::download(
                self.client.downloads(),
                DownloadRequest { url: &uri, token: None, dest: &archive_path, expected_size: 0, expected_md5: None },
                &cancel,
                &|_, _| {},
            )
            .await?;
            let extracted = dir.join("extracted");
            let (a, e) = (archive_path.clone(), extracted.clone());
            tokio::task::spawn_blocking(move || {
                let cancel = AtomicBool::new(false);
                archive::extract(&a, &e, MAX_ARCHIVE, &cancel, &|_, _| {})?;
                read_manifest(&e).ok_or_else(|| AppError::BadArchive(crate::tr!("collection.json introuvable", "collection.json not found")))
            })
            .await
            .map_err(|e| AppError::Io(e.to_string()))?
        }
        .await;
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir(dir.parent().unwrap_or(&dir));
        result
    }

    /// Mods déjà installés que reprend une collection : rangés à leur place dans
    /// son ordre, sans toucher à leurs fichiers.
    pub fn mark_collection(&self, game_id: i64, slug: &str, marks: &[(i64, u32)]) -> AppResult<GameInstall> {
        check_slug(slug)?;
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        let wanted: HashMap<i64, u32> = marks.iter().copied().collect();
        for m in game.mods.values_mut() {
            if let Some(&rank) = wanted.get(&m.mod_id) {
                m.collection = Some(CollectionMark { slug: slug.to_string(), rank });
            }
        }
        let out = game.clone();
        lib.save(&self.library_path)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::installer::tests::{make_zip, tempdir};
    use crate::testutil::{fake_server_with, json_response};

    fn zip_response(body: &[u8]) -> String {
        let mut s = format!("HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        s.extend(body.iter().map(|&b| b as char));
        s
    }

    const COLLECTION: &str = r#"{"data":{"collection":{"slug":"abc123","name":"Belle Skyrim","summary":"Tout beau","endorsements":"12","totalDownloads":340,
        "adultContent":false,"user":{"name":"Curateur"},"game":{"id":1704,"domainName":"skyrimspecialedition","name":"Skyrim SE"},
        "tileImage":{"url":"https://images.nexusmods.com/c.png","thumbnailUrl":"https://images.nexusmods.com/c-med.png"},
        "latestPublishedRevision":{"revisionNumber":4,"modCount":2,"totalSize":"2048","fileSize":"300",
          "downloadLink":"/v1/collections/abc123/revisions/4/download_link",
          "modFiles":[
            {"fileId":22,"optional":false,"file":{"name":"B Main","version":"2.0","sizeInBytes":"1024","mod":{"modId":2,"name":"Mod B"}}},
            {"fileId":11,"optional":true,"file":{"name":"A Main","version":"1.0","size":1,"mod":{"modId":1,"name":"Mod A"}}},
            {"fileId":12,"optional":false,"file":{"name":"A Patch","version":"1.0","size":1,"mod":{"modId":1,"name":"Mod A"}}}
          ],
          "externalResources":[{"name":"SKSE","resourceType":"browse","resourceUrl":"https://skse.silverlock.org","optional":false}]}}}}"#;

    #[tokio::test]
    async fn reads_a_collection_and_orders_it_from_its_archive() {
        // A doit passer avant B (règle), alors que la liste les donne dans l'autre sens.
        let manifest = br#"{"mods":[
            {"name":"B","source":{"type":"nexus","modId":2,"fileId":22,"md5":"bb"}},
            {"name":"A","source":{"type":"nexus","modId":1,"fileId":11,"md5":"aa"}}],
          "modRules":[{"source":{"fileMD5":"aa"},"type":"before","reference":{"fileMD5":"bb"}}]}"#;
        let archive = make_zip(&[("collection.json", manifest)]);
        let server = fake_server_with(|base| {
            let root = base.trim_end_matches("/v1").to_string();
            vec![
                json_response(200, COLLECTION),
                json_response(200, r#"{"user_id":7,"name":"Code","is_premium":false,"is_supporter":false}"#),
                json_response(200, COLLECTION),
                json_response(200, &format!(r#"{{"download_links":[{{"name":"CDN","URI":"{root}/dl/collection.zip"}}]}}"#)),
                zip_response(&archive),
            ]
        });
        let s = NexusState::new(server.base.trim_end_matches("/v1"), Box::new(MemoryStore::default()), tempdir("col-plan").join("n.json"));

        let detail = s.get_collection("abc123", "skyrimspecialedition").await.unwrap();
        assert_eq!(detail.card.name, "Belle Skyrim");
        assert_eq!(detail.card.endorsements, 12);
        assert_eq!(detail.card.image.as_deref(), Some("https://images.nexusmods.com/c-med.png"));
        assert_eq!(detail.mods.len(), 3);
        assert_eq!(detail.mods[1].size, 1024, "taille en Ko convertie");
        assert!(detail.mods[1].optional);
        assert_eq!(detail.external[0].url.as_deref(), Some("https://skse.silverlock.org"));

        s.set_key("cle").await.unwrap();
        let plan = s.collection_plan("abc123", "skyrimspecialedition", 4).await.unwrap();
        assert!(plan.from_manifest);
        assert_eq!(plan.order.iter().map(|e| e.mod_id).collect::<Vec<_>>(), vec![1, 2]);
        // Second fichier d'un même mod : à faire à la main.
        assert_eq!(plan.extras.len(), 1);
        assert_eq!(plan.extras[0].kind, "second_file");
        assert!(plan.extras[0].url.as_deref().unwrap().ends_with("/skyrimspecialedition/mods/1?tab=files&file_id=12"));
        let req = server.request(3);
        assert!(req.line.starts_with("GET /v1/collections/abc123/revisions/4/download_link"), "{}", req.line);
        assert_eq!(req.header("apikey").as_deref(), Some("cle"));
        assert!(!s.library_path.with_file_name("collection-cache").exists(), "archive supprimée");
    }

    fn manifest(json: &str) -> Manifest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn slugs_are_checked() {
        assert!(check_slug("tsdxpq").is_ok());
        assert!(check_slug("").is_err());
        assert!(check_slug("a/b").is_err());
        assert!(check_slug("../x").is_err());
    }

    #[test]
    fn download_links_must_point_to_the_collection_api() {
        assert_eq!(download_path("/v1/collections/abc123/revisions/7/download_link").as_deref(), Some("/collections/abc123/revisions/7/download_link"));
        assert_eq!(
            download_path("https://api.nexusmods.com/v1/collections/abc123/revisions/7/download_link").as_deref(),
            Some("/collections/abc123/revisions/7/download_link")
        );
        assert!(download_path("/v1/users/validate.json").is_none());
        assert!(download_path("https://evil.example/v1/collections/a/revisions/1/download_link").is_none());
        assert!(download_path("/v1/collections/a/revisions/1x/download_link").is_none());
    }

    #[test]
    fn rules_order_the_mods_and_ties_keep_the_list_order() {
        // B doit passer après C, A avant tout le monde ; D sans règle.
        let m = manifest(
            r#"{
              "mods": [
                {"name":"A","source":{"type":"nexus","modId":1,"fileId":11,"md5":"aa"}},
                {"name":"B","source":{"type":"nexus","modId":2,"fileId":22,"md5":"bb"}},
                {"name":"C","source":{"type":"nexus","modId":3,"fileId":33,"logicalFilename":"C-Main"}},
                {"name":"D","source":{"type":"nexus","modId":4,"fileId":44}},
                {"name":"Config","source":{"type":"bundle","fileExpression":"cfg"}},
                {"name":"Tool","source":{"type":"browse","url":"https://example.org/tool","instructions":"Télécharge l'outil"}}
              ],
              "modRules": [
                {"source":{"fileMD5":"bb"},"type":"after","reference":{"logicalFileName":"c-main"}},
                {"source":{"fileMD5":"aa"},"type":"before","reference":{"fileMD5":"bb"}},
                {"source":{"fileMD5":"aa"},"type":"requires","reference":{"fileMD5":"zz"}}
              ]
            }"#,
        );
        let plan = plan_from_manifest(&m, "skyrimspecialedition");
        let order: Vec<i64> = plan.order.iter().map(|e| e.mod_id).collect();
        assert_eq!(order, vec![1, 3, 2, 4]);
        assert_eq!(plan.order.iter().map(|e| e.rank).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
        assert_eq!(plan.extras.len(), 2);
        assert_eq!(plan.extras[1].url.as_deref(), Some("https://example.org/tool"));
    }

    #[test]
    fn rule_loops_are_broken_in_list_order() {
        let m = manifest(
            r#"{
              "mods": [
                {"name":"A","source":{"type":"nexus","modId":1,"fileId":11,"md5":"aa"}},
                {"name":"B","source":{"type":"nexus","modId":2,"fileId":22,"md5":"bb"}}
              ],
              "modRules": [
                {"source":{"fileMD5":"aa"},"type":"after","reference":{"fileMD5":"bb"}},
                {"source":{"fileMD5":"bb"},"type":"after","reference":{"fileMD5":"aa"}}
              ]
            }"#,
        );
        let order: Vec<i64> = plan_from_manifest(&m, "x").order.iter().map(|e| e.mod_id).collect();
        assert_eq!(order, vec![1, 2]);
    }

    #[test]
    fn other_games_and_creator_choices() {
        let m = manifest(
            r#"{"mods":[
              {"name":"A","domainName":"skyrim","source":{"type":"nexus","modId":1,"fileId":11}},
              {"name":"B","domainName":"skyrimspecialedition","source":{"type":"nexus","modId":2,"fileId":22},
               "choices":{"type":"fomod","options":[{"name":"Main","groups":[{"name":"Textures","choices":[{"name":"2K","idx":1}]}]}]}}
            ]}"#,
        );
        let plan = plan_from_manifest(&m, "skyrimspecialedition");
        assert_eq!(plan.order.len(), 1);
        assert_eq!(plan.order[0].choices, vec!["Main › Textures : 2K".to_string()]);
        assert_eq!(plan.extras[0].kind, "other_game");
    }
}
