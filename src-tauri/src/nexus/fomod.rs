//! Installateurs FOMOD (`fomod/ModuleConfig.xml`) : des étapes où l'on choisit
//! des options, puis les fichiers correspondants sont installés.
//!
//! Le fichier est lu une fois : les conditions qui ne dépendent que du jeu
//! (fichiers présents, version, script extender) sont évaluées tout de suite ;
//! celles qui dépendent des choix (« flags ») le sont dans l'interface pendant
//! qu'on choisit, puis de nouveau ici au moment d'installer.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use roxmltree::{Document, Node};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::installer;

/// Condition sur les choix déjà faits.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Dep {
    All { items: Vec<Dep> },
    Any { items: Vec<Dep> },
    Flag { flag: String, value: String },
    Const { value: bool },
}

impl Dep {
    pub fn eval(&self, flags: &HashMap<String, String>) -> bool {
        match self {
            Dep::All { items } => items.iter().all(|d| d.eval(flags)),
            Dep::Any { items } => items.is_empty() || items.iter().any(|d| d.eval(flags)),
            Dep::Flag { flag, value } => flags.get(flag).map(String::as_str).unwrap_or("") == value,
            Dep::Const { value } => *value,
        }
    }
}

/// Fichier ou dossier à installer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileItem {
    pub source: String,
    /// `None` : même chemin que la source.
    pub destination: Option<String>,
    pub folder: bool,
    pub priority: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TypePattern {
    pub dep: Dep,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    pub name: String,
    pub description: String,
    /// Image en `data:` (affichable directement).
    pub image: Option<String>,
    pub flags: Vec<Flag>,
    /// Type par défaut : `Required`, `Recommended`, `Optional`, `NotUsable`, `CouldBeUsable`.
    pub type_default: String,
    /// Le type change selon les choix déjà faits.
    pub type_patterns: Vec<TypePattern>,
    #[serde(skip)]
    pub files: Vec<FileItem>,
}

impl Plugin {
    pub fn kind(&self, flags: &HashMap<String, String>) -> &str {
        self.type_patterns
            .iter()
            .find(|p| p.dep.eval(flags))
            .map(|p| p.kind.as_str())
            .unwrap_or(&self.type_default)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub name: String,
    /// `SelectExactlyOne`, `SelectAtMostOne`, `SelectAtLeastOne`, `SelectAll`, `SelectAny`.
    #[serde(rename = "type")]
    pub kind: String,
    pub plugins: Vec<Plugin>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub name: String,
    /// Étape affichée seulement si la condition est remplie.
    pub visible: Option<Dep>,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installer {
    pub module_name: String,
    pub image: Option<String>,
    pub steps: Vec<Step>,
    #[serde(skip)]
    pub required: Vec<FileItem>,
    #[serde(skip)]
    pub conditional: Vec<(Dep, Vec<FileItem>)>,
}

impl Installer {
    /// Il y a des choix à faire (sinon les fichiers s'installent directement).
    pub fn has_choices(&self) -> bool {
        !self.steps.is_empty()
    }
}

/// Ce que l'installateur peut vérifier sur le jeu.
#[derive(Default)]
pub struct GameState {
    /// Dossier où se trouvent les fichiers testés (`Data` des jeux Bethesda).
    pub data_dir: Option<PathBuf>,
    /// Plugins actifs (en minuscules).
    pub active_plugins: HashSet<String>,
    pub game_version: Option<String>,
    pub extender_present: bool,
}

// ---------------------------------------------------------------- Lecture

/// Dossier qui contient `fomod/ModuleConfig.xml` (racine de l'archive ou un
/// dossier d'emballage), avec le chemin du fichier.
pub fn find(extracted: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut candidates = vec![extracted.to_path_buf()];
    if let Ok(entries) = fs::read_dir(extracted) {
        let dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        candidates.extend(dirs);
    }
    candidates.into_iter().find_map(|root| {
        let config = resolve_ci(&root, "fomod/ModuleConfig.xml")?;
        config.is_file().then_some((root, config))
    })
}

/// Chemin de l'archive sans tenir compte des majuscules ni du séparateur.
pub fn resolve_ci(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." {
            return None;
        }
        let direct = path.join(part);
        if direct.exists() {
            path = direct;
            continue;
        }
        let found = fs::read_dir(&path)
            .ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        path = found.path();
    }
    Some(path)
}

/// Texte d'un fichier XML en UTF-8 ou UTF-16 (avec ou sans BOM).
pub fn decode(raw: &[u8]) -> String {
    let utf16 = |bytes: &[u8], le: bool| {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match raw {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [b'<', 0, ..] => utf16(raw, true),
        [0, b'<', ..] => utf16(raw, false),
        _ => String::from_utf8_lossy(raw).into_owned(),
    }
}

fn is(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name().eq_ignore_ascii_case(name)
}

fn child<'a, 'i>(node: &Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| is(c, name))
}

fn children<'a, 'i>(node: &Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |c| is(c, name))
}

fn attr(node: &Node, name: &str) -> Option<String> {
    node.attributes()
        .find(|a| a.name().eq_ignore_ascii_case(name))
        .map(|a| a.value().to_string())
}

fn text(node: &Node) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Ordre d'une liste : `Explicit` (celui du fichier), `Ascending` (par défaut) ou `Descending`.
fn ordered<T>(mut items: Vec<T>, order: Option<String>, name: impl Fn(&T) -> String) -> Vec<T> {
    match order.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("explicit") => {}
        Some("descending") => items.sort_by_key(|i| std::cmp::Reverse(name(i).to_lowercase())),
        _ => items.sort_by_key(|i| name(i).to_lowercase()),
    }
    items
}

fn version_at_least(have: Option<&str>, want: &str) -> bool {
    let parse = |v: &str| v.split(['.', ',', ' ']).filter_map(|p| p.trim().parse::<u64>().ok()).collect::<Vec<_>>();
    let Some(have) = have else { return true };
    let (h, w) = (parse(have), parse(want));
    for i in 0..h.len().max(w.len()) {
        let (a, b) = (h.get(i).copied().unwrap_or(0), w.get(i).copied().unwrap_or(0));
        if a != b {
            return a > b;
        }
    }
    true
}

fn file_state(game: &GameState, file: &str, state: &str) -> bool {
    let Some(dir) = &game.data_dir else { return true };
    let path = resolve_ci(dir, file);
    let exists = path.as_ref().is_some_and(|p| p.exists());
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file).to_ascii_lowercase();
    let is_plugin = name.ends_with(".esp") || name.ends_with(".esm") || name.ends_with(".esl");
    // Les fichiers maîtres du jeu et des DLC ne sont pas listés dans plugins.txt.
    let active = exists && (!is_plugin || game.active_plugins.contains(&name) || name.ends_with(".esm"));
    match state.to_ascii_lowercase().as_str() {
        "active" => active,
        "inactive" => exists && !active,
        "missing" => !exists,
        _ => true,
    }
}

fn parse_deps(node: &Node, game: &GameState) -> Dep {
    let items: Vec<Dep> = node
        .children()
        .filter(|c| c.is_element())
        .map(|c| {
            let tag = c.tag_name().name().to_ascii_lowercase();
            match tag.as_str() {
                "flagdependency" => Dep::Flag {
                    flag: attr(&c, "flag").unwrap_or_default(),
                    value: attr(&c, "value").unwrap_or_default(),
                },
                "filedependency" => Dep::Const {
                    value: file_state(game, &attr(&c, "file").unwrap_or_default(), &attr(&c, "state").unwrap_or_default()),
                },
                "gamedependency" => Dep::Const {
                    value: version_at_least(game.game_version.as_deref(), &attr(&c, "version").unwrap_or_default()),
                },
                // Script extender (FOSE, NVSE, SKSE…) : présent ou non.
                "fosedependency" => Dep::Const { value: game.extender_present },
                "dependencies" => parse_deps(&c, game),
                // fommDependency, fomodDependency… : toujours satisfaits.
                _ => Dep::Const { value: true },
            }
        })
        .collect();
    if attr(node, "operator").is_some_and(|o| o.eq_ignore_ascii_case("or")) {
        Dep::Any { items }
    } else {
        Dep::All { items }
    }
}

fn parse_files(node: &Node) -> Vec<FileItem> {
    node.children()
        .filter(|c| is(c, "file") || is(c, "folder"))
        .map(|c| FileItem {
            source: attr(&c, "source").unwrap_or_default(),
            destination: attr(&c, "destination"),
            folder: is(&c, "folder"),
            priority: attr(&c, "priority").and_then(|p| p.trim().parse().ok()).unwrap_or(0),
        })
        .collect()
}

/// Images en `data:` (taille limitée, pour ne pas alourdir l'interface).
struct Images<'a> {
    root: &'a Path,
    budget: u64,
}

impl Images<'_> {
    fn load(&mut self, rel: Option<String>) -> Option<String> {
        let rel = rel.filter(|r| !r.trim().is_empty())?;
        let path = resolve_ci(self.root, &rel)?;
        let size = fs::metadata(&path).ok()?.len();
        if size > 4 * 1024 * 1024 || size > self.budget {
            return None;
        }
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "webp" => "image/webp",
            _ => return None,
        };
        let bytes = fs::read(&path).ok()?;
        self.budget -= size;
        Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }
}

/// Lit `ModuleConfig.xml`. `root` : dossier qui contient le dossier `fomod`.
pub fn parse(root: &Path, config: &Path, game: &GameState) -> AppResult<Installer> {
    let raw = fs::read(config).map_err(installer::io_err)?;
    let xml = decode(&raw);
    let doc = Document::parse_with_options(
        &xml,
        roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
    )
    .map_err(|e| AppError::UnknownLayout(format!("L'installateur de ce mod (FOMOD) est illisible : {e}")))?;
    let cfg = doc.root_element();
    let mut images = Images { root, budget: 32 * 1024 * 1024 };

    if let Some(deps) = child(&cfg, "moduleDependencies") {
        if !parse_deps(&deps, game).eval(&HashMap::new()) {
            return Err(AppError::UnknownLayout(
                "Ce mod demande d'autres mods ou une autre version du jeu (conditions de son installateur FOMOD non remplies).".into(),
            ));
        }
    }

    let required = child(&cfg, "requiredInstallFiles").map(|n| parse_files(&n)).unwrap_or_default();
    let mut steps = Vec::new();
    if let Some(install_steps) = child(&cfg, "installSteps") {
        let list: Vec<Step> = children(&install_steps, "installStep")
            .map(|s| {
                let groups_node = child(&s, "optionalFileGroups");
                let groups: Vec<Group> = groups_node
                    .map(|gn| {
                        let groups: Vec<Group> = children(&gn, "group")
                            .map(|g| {
                                let plugins_node = child(&g, "plugins");
                                let plugins: Vec<Plugin> = plugins_node
                                    .map(|pn| {
                                        let plugins: Vec<Plugin> = children(&pn, "plugin")
                                            .map(|p| {
                                                let (type_default, type_patterns) = child(&p, "typeDescriptor")
                                                    .map(|td| {
                                                        if let Some(t) = child(&td, "type") {
                                                            (attr(&t, "name").unwrap_or_else(|| "Optional".into()), Vec::new())
                                                        } else if let Some(dt) = child(&td, "dependencyType") {
                                                            let default = child(&dt, "defaultType")
                                                                .and_then(|d| attr(&d, "name"))
                                                                .unwrap_or_else(|| "Optional".into());
                                                            let patterns = child(&dt, "patterns")
                                                                .map(|ps| {
                                                                    children(&ps, "pattern")
                                                                        .filter_map(|pat| {
                                                                            Some(TypePattern {
                                                                                dep: parse_deps(&child(&pat, "dependencies")?, game),
                                                                                kind: attr(&child(&pat, "type")?, "name")?,
                                                                            })
                                                                        })
                                                                        .collect()
                                                                })
                                                                .unwrap_or_default();
                                                            (default, patterns)
                                                        } else {
                                                            ("Optional".into(), Vec::new())
                                                        }
                                                    })
                                                    .unwrap_or_else(|| ("Optional".into(), Vec::new()));
                                                Plugin {
                                                    name: attr(&p, "name").unwrap_or_default(),
                                                    description: child(&p, "description").map(|d| text(&d)).unwrap_or_default(),
                                                    image: images.load(child(&p, "image").and_then(|i| attr(&i, "path"))),
                                                    flags: child(&p, "conditionFlags")
                                                        .map(|cf| {
                                                            children(&cf, "flag")
                                                                .map(|f| Flag { name: attr(&f, "name").unwrap_or_default(), value: text(&f) })
                                                                .collect()
                                                        })
                                                        .unwrap_or_default(),
                                                    type_default,
                                                    type_patterns,
                                                    files: child(&p, "files").map(|f| parse_files(&f)).unwrap_or_default(),
                                                }
                                            })
                                            .collect();
                                        ordered(plugins, attr(&pn, "order"), |p| p.name.clone())
                                    })
                                    .unwrap_or_default();
                                Group {
                                    name: attr(&g, "name").unwrap_or_default(),
                                    kind: attr(&g, "type").unwrap_or_else(|| "SelectAny".into()),
                                    plugins,
                                }
                            })
                            .collect();
                        ordered(groups, attr(&gn, "order"), |g| g.name.clone())
                    })
                    .unwrap_or_default();
                Step {
                    name: attr(&s, "name").unwrap_or_default(),
                    visible: child(&s, "visible").map(|v| parse_deps(&v, game)),
                    groups,
                }
            })
            .collect();
        steps = ordered(list, attr(&install_steps, "order"), |s| s.name.clone());
    }

    let conditional = child(&cfg, "conditionalFileInstalls")
        .and_then(|c| child(&c, "patterns"))
        .map(|ps| {
            children(&ps, "pattern")
                .filter_map(|p| Some((parse_deps(&child(&p, "dependencies")?, game), parse_files(&child(&p, "files")?))))
                .collect()
        })
        .unwrap_or_default();

    Ok(Installer {
        module_name: child(&cfg, "moduleName").map(|n| text(&n)).unwrap_or_default(),
        image: images.load(child(&cfg, "moduleImage").and_then(|i| attr(&i, "path"))),
        steps,
        required,
        conditional,
    })
}

// ---------------------------------------------------------------- Choix

/// Fichiers à installer d'après les choix : `selections[étape][groupe]` =
/// options cochées. Les règles des groupes sont vérifiées de nouveau ici.
pub fn resolve(inst: &Installer, selections: &[Vec<Vec<usize>>]) -> AppResult<Vec<FileItem>> {
    let mut flags: HashMap<String, String> = HashMap::new();
    let mut files = inst.required.clone();
    for (si, step) in inst.steps.iter().enumerate() {
        if step.visible.as_ref().is_some_and(|v| !v.eval(&flags)) {
            continue;
        }
        let mut new_flags = Vec::new();
        for (gi, group) in step.groups.iter().enumerate() {
            let mut chosen: Vec<usize> = selections
                .get(si)
                .and_then(|s| s.get(gi))
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|i| *i < group.plugins.len())
                .collect();
            chosen.sort_unstable();
            chosen.dedup();
            for (pi, p) in group.plugins.iter().enumerate() {
                match p.kind(&flags).to_ascii_lowercase().as_str() {
                    "required" if !chosen.contains(&pi) => chosen.push(pi),
                    "notusable" => chosen.retain(|i| *i != pi),
                    _ => {}
                }
            }
            let kind = group.kind.to_ascii_lowercase();
            if kind == "selectall" {
                chosen = (0..group.plugins.len()).collect();
            }
            let invalid = match kind.as_str() {
                "selectexactlyone" => chosen.len() != 1,
                "selectatleastone" => chosen.is_empty(),
                "selectatmostone" => chosen.len() > 1,
                _ => false,
            };
            if invalid && !group.plugins.is_empty() {
                return Err(AppError::InvalidArgument(format!(
                    "choix incomplet dans « {} » ({}).",
                    if group.name.is_empty() { &step.name } else { &group.name },
                    match kind.as_str() {
                        "selectexactlyone" => "une option exactement",
                        "selectatleastone" => "au moins une option",
                        _ => "une option au plus",
                    }
                )));
            }
            chosen.sort_unstable();
            for pi in chosen {
                let p = &group.plugins[pi];
                files.extend(p.files.iter().cloned());
                new_flags.extend(p.flags.iter().cloned());
            }
        }
        // Les choix d'une étape comptent pour les étapes suivantes.
        for f in new_flags {
            flags.insert(f.name, f.value);
        }
    }
    for (dep, items) in &inst.conditional {
        if dep.eval(&flags) {
            files.extend(items.iter().cloned());
        }
    }
    Ok(files)
}

fn clean(path: &str) -> String {
    path.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect::<Vec<_>>().join("/")
}

/// Fichiers réels à déplacer : (source dans l'archive, destination relative).
/// Deux fichiers pour la même destination : la priorité la plus haute gagne,
/// à égalité le dernier cité.
pub fn expand(root: &Path, items: &[FileItem]) -> AppResult<Vec<(PathBuf, String)>> {
    let mut chosen: HashMap<String, (i64, usize, PathBuf, String)> = HashMap::new();
    let mut order = 0usize;
    for item in items {
        let source = clean(&item.source);
        let src = if source.is_empty() { Some(root.to_path_buf()) } else { resolve_ci(root, &source) };
        let Some(src) = src.filter(|p| p.exists()) else {
            return Err(AppError::UnknownLayout(format!(
                "L'installateur de ce mod demande « {} », absent de l'archive.",
                item.source
            )));
        };
        let mut push = |from: PathBuf, dest: String| {
            order += 1;
            let key = dest.to_ascii_lowercase();
            let keep = chosen.get(&key).is_some_and(|(p, _, _, _)| *p > item.priority);
            if !keep {
                chosen.insert(key, (item.priority, order, from, dest));
            }
        };
        if item.folder || src.is_dir() {
            let base = match &item.destination {
                Some(d) => clean(d),
                None => source.clone(),
            };
            for rel in installer::walk_files(&src)? {
                let rel = installer::rel_string(&rel);
                if rel.split('/').next().is_some_and(|c| c.eq_ignore_ascii_case("fomod")) && source.is_empty() {
                    continue;
                }
                let dest = if base.is_empty() { rel.clone() } else { format!("{base}/{rel}") };
                push(src.join(&rel), dest);
            }
        } else {
            let file_name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dest = match &item.destination {
                None => source.clone(),
                Some(d) if clean(d).is_empty() => file_name,
                // Destination qui finit par un séparateur : un dossier.
                Some(d) if d.ends_with('/') || d.ends_with('\\') => format!("{}/{file_name}", clean(d)),
                Some(d) => clean(d),
            };
            push(src, dest);
        }
    }
    let mut out: Vec<(usize, PathBuf, String)> = chosen.into_values().map(|(_, o, s, d)| (o, s, d)).collect();
    out.sort_by_key(|(o, _, _)| *o);
    if out.is_empty() {
        return Err(AppError::UnknownLayout("Les options choisies n'installent aucun fichier.".into()));
    }
    Ok(out.into_iter().map(|(_, s, d)| (s, d)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;

    const CONFIG: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://qconsulting.ca/fo3/ModConfig5.0.xsd">
  <moduleName>Better Lanterns</moduleName>
  <moduleImage path="fomod\images\main.png"/>
  <requiredInstallFiles>
    <folder source="00 Core" destination=""/>
  </requiredInstallFiles>
  <installSteps order="Explicit">
    <installStep name="Style">
      <optionalFileGroups order="Explicit">
        <group name="Couleur" type="SelectExactlyOne">
          <plugins order="Explicit">
            <plugin name="Chaude">
              <description>Lumière orangée.</description>
              <image path="fomod/images/warm.png"/>
              <conditionFlags><flag name="color">warm</flag></conditionFlags>
              <files><file source="10 Warm\Lanterns.esp" destination="Lanterns.esp"/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Froide">
              <description>Lumière bleutée.</description>
              <conditionFlags><flag name="color">cold</flag></conditionFlags>
              <files><file source="11 Cold/Lanterns.esp" destination="Lanterns.esp"/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
        <group name="Extras" type="SelectAny">
          <plugins>
            <plugin name="Zeta patch">
              <description>Patch pour Zeta.</description>
              <files><folder source="20 Patch" destination="textures/patch"/></files>
              <typeDescriptor>
                <dependencyType>
                  <defaultType name="NotUsable"/>
                  <patterns><pattern>
                    <dependencies operator="And"><fileDependency file="Zeta.esp" state="Active"/></dependencies>
                    <type name="Recommended"/>
                  </pattern></patterns>
                </dependencyType>
              </typeDescriptor>
            </plugin>
            <plugin name="Alpha extra">
              <description>Plus de lanternes.</description>
              <files><file source="21 Extra\extra.esp" destination="" priority="1"/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
    <installStep name="Réglages bleus">
      <visible><flagDependency flag="color" value="cold"/></visible>
      <optionalFileGroups>
        <group name="Intensité" type="SelectExactlyOne">
          <plugins>
            <plugin name="Forte"><description/><files><file source="12 Strong/strong.ini" destination="strong.ini"/></files><typeDescriptor><type name="Optional"/></typeDescriptor></plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
  <conditionalFileInstalls>
    <patterns>
      <pattern>
        <dependencies operator="And"><flagDependency flag="color" value="warm"/></dependencies>
        <files><file source="30 Warm meshes\lantern.nif" destination="meshes\lantern.nif"/></files>
      </pattern>
    </patterns>
  </conditionalFileInstalls>
</config>"#;

    fn archive() -> PathBuf {
        let dir = tempdir("fomod");
        let files = [
            "Wrapper/fomod/ModuleConfig.xml",
            "Wrapper/fomod/images/main.png",
            "Wrapper/fomod/images/warm.png",
            "Wrapper/00 Core/scripts/lantern.pex",
            "Wrapper/10 Warm/Lanterns.esp",
            "Wrapper/11 Cold/Lanterns.esp",
            "Wrapper/12 Strong/strong.ini",
            "Wrapper/20 Patch/a.dds",
            "Wrapper/21 Extra/extra.esp",
            "Wrapper/30 Warm meshes/lantern.nif",
        ];
        for f in files {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, f.as_bytes()).unwrap();
        }
        // ModuleConfig.xml en UTF-16 avec BOM, comme souvent.
        let mut raw = vec![0xFF, 0xFE];
        for u in CONFIG.encode_utf16() {
            raw.extend(u.to_le_bytes());
        }
        fs::write(dir.join("Wrapper/fomod/ModuleConfig.xml"), raw).unwrap();
        dir
    }

    #[test]
    fn reads_steps_options_and_conditions() {
        let dir = archive();
        let (root, config) = find(&dir).unwrap();
        assert!(root.ends_with("Wrapper"));
        let inst = parse(&root, &config, &GameState::default()).unwrap();
        assert_eq!(inst.module_name, "Better Lanterns");
        assert!(inst.image.as_deref().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(inst.steps.len(), 2);
        let style = &inst.steps[0];
        assert_eq!(style.groups.len(), 2);
        assert_eq!(style.groups[0].plugins[0].name, "Chaude");
        assert_eq!(style.groups[0].plugins[0].description, "Lumière orangée.");
        assert!(style.groups[0].plugins[0].image.is_some());
        // Ordre alphabétique par défaut.
        assert_eq!(style.groups[1].plugins[0].name, "Alpha extra");
        // Pas de dossier de jeu connu : le fichier est supposé présent.
        assert_eq!(style.groups[1].plugins[1].kind(&HashMap::new()), "Recommended");
        assert!(inst.steps[1].visible.is_some());
    }

    #[test]
    fn installs_the_files_of_the_chosen_options() {
        let dir = archive();
        let (root, config) = find(&dir).unwrap();
        let data = tempdir("fomod-data");
        let game = GameState { data_dir: Some(data.clone()), ..Default::default() };
        let inst = parse(&root, &config, &game).unwrap();
        // Zeta.esp absent : le patch n'est pas utilisable.
        assert_eq!(inst.steps[0].groups[1].plugins[1].kind(&HashMap::new()), "NotUsable");

        // Chaude + extra (+ patch demandé mais refusé) ; l'étape bleue est cachée.
        let files = resolve(&inst, &[vec![vec![0], vec![0, 1]]]).unwrap();
        let moves = expand(&root, &files).unwrap();
        let mut dests: Vec<String> = moves.iter().map(|(_, d)| d.clone()).collect();
        dests.sort();
        assert_eq!(dests, vec!["Lanterns.esp", "extra.esp", "meshes/lantern.nif", "scripts/lantern.pex"]);
        let esp = moves.iter().find(|(_, d)| d == "Lanterns.esp").unwrap();
        assert!(esp.0.ends_with("10 Warm/Lanterns.esp"));

        // Froide : l'étape suivante apparaît, ses choix comptent.
        let files = resolve(&inst, &[vec![vec![1], vec![]], vec![vec![0]]]).unwrap();
        let dests: HashSet<String> = expand(&root, &files).unwrap().into_iter().map(|(_, d)| d).collect();
        assert!(dests.contains("strong.ini") && !dests.contains("meshes/lantern.nif"));

        // « Une option exactement » : rien de coché, refusé.
        assert!(matches!(resolve(&inst, &[vec![vec![], vec![]]]), Err(AppError::InvalidArgument(_))));
    }

    #[test]
    fn decodes_utf16_and_compares_versions() {
        let mut raw = vec![0xFE, 0xFF];
        for u in "<a/>".encode_utf16() {
            raw.extend(u.to_be_bytes());
        }
        assert_eq!(decode(&raw), "<a/>");
        assert_eq!(decode(b"\xEF\xBB\xBF<a/>"), "<a/>");
        assert!(version_at_least(Some("1.6.1170.0"), "1.5.97"));
        assert!(!version_at_least(Some("1.5.39"), "1.5.97"));
    }
}
