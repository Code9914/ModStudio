// Appels au backend Rust (voir src-tauri/src/commands.rs).
import { Channel, invoke } from "@tauri-apps/api/core";
import { backendLocale } from "./i18n.svelte";

// ------------------------------------------------------------------ Types

export interface AppError {
  kind:
    | "missing_api_key"
    | "unauthorized"
    | "forbidden"
    | "not_found"
    | "rate_limited"
    | "api"
    | "network"
    | "decode"
    | "invalid_api_path"
    | "storage"
    | "invalid_argument"
    | "not_logged_in"
    | "session_expired"
    | "email_login_unavailable"
    | "invalid_code"
    | "invalid_token"
    | "game_closed"
    | "no_game_folder"
    | "install_in_progress"
    | "no_modfile"
    | "virus_flagged"
    | "corrupted"
    | "cancelled"
    | "conflict"
    | "io"
    | "bad_archive"
    | "download_refused"
    | "login_required_for_download"
    | "unsupported_archive"
    | "unknown_layout"
    | "wrong_platform"
    | "need_game_dir"
    | "nexus_key_missing"
    | "nexus_key_invalid"
    | "nexus_premium_required"
    | "nexus_link_expired"
    | "nexus_not_found"
    | "nexus_api"
    | "tool_unavailable"
    | "tool_mismatch"
    | "tool_present"
    | "nexus_refused"
    | "thunderstore_api"
    | "thunderstore_unsupported"
    | "share_invalid"
    | "unknown";
  message: string;
  retryAfter: number | null;
}

export interface SettingsView {
  hasApiKey: boolean;
  apiKeyHint: string | null;
  apiPath: string;
  hideMature: boolean;
}

export interface Page<T> {
  data: T[];
  result_count: number;
  result_offset: number;
  result_limit: number;
  result_total: number;
}

export interface Logo {
  original: string | null;
  thumb_320x180: string | null;
  thumb_640x360: string | null;
  thumb_1280x720: string | null;
}

export interface Icon {
  original: string | null;
  thumb_64x64: string | null;
  thumb_128x128: string | null;
  thumb_256x256: string | null;
}

export interface Image {
  original: string | null;
  thumb_320x180: string | null;
  thumb_1280x720: string | null;
}

export interface TagOption {
  name: string | null;
  name_localized: string | null;
  type: string | null;
  tags: string[];
  hidden: boolean;
}

export interface Game {
  id: number;
  name: string | null;
  name_id: string | null;
  summary: string | null;
  profile_url: string | null;
  instructions_url: string | null;
  /** Bit 1 : ouvert aux applications tierces ; bit 2 : téléchargements directs. */
  api_access_options: number | null;
  ugc_name: string | null;
  date_updated: number | null;
  icon: Icon | null;
  logo: Logo | null;
  header: { original: string | null } | null;
  stats: {
    mods_count_total: number;
    mods_downloads_total: number;
    mods_subscribers_total: number;
  } | null;
  tag_options: TagOption[];
}

export interface User {
  id: number;
  username: string | null;
  display_name_portal: string | null;
  profile_url: string | null;
  avatar: { original: string | null; thumb_50x50: string | null; thumb_100x100: string | null } | null;
}

export interface Modfile {
  id: number;
  mod_id: number;
  date_added: number | null;
  virus_status: number;
  virus_positive: number;
  filesize: number;
  filesize_uncompressed: number;
  filename: string | null;
  version: string | null;
  changelog: string | null;
  filehash: { md5: string | null } | null;
}

export interface ModStats {
  popularity_rank_position: number;
  popularity_rank_total_mods: number;
  downloads_today: number;
  downloads_total: number;
  subscribers_total: number;
  ratings_total: number;
  ratings_positive: number;
  ratings_negative: number;
  ratings_percentage_positive: number;
  ratings_display_text: string | null;
}

export interface Mod {
  id: number;
  game_id: number;
  name: string | null;
  name_id: string | null;
  summary: string | null;
  description: string | null;
  description_plaintext: string | null;
  profile_url: string | null;
  homepage_url: string | null;
  date_added: number | null;
  date_updated: number | null;
  date_live: number | null;
  maturity_option: number;
  dependencies: boolean;
  logo: Logo | null;
  submitted_by: User | null;
  media: { images: Image[]; youtube: string[]; sketchfab: string[] } | null;
  modfile: Modfile | null;
  tags: { name: string | null; name_localized: string | null }[];
  stats: ModStats | null;
  platforms: { platform: string | null; modfile_live: number | null }[];
}

export interface Dependency {
  mod_id: number;
  name: string | null;
  name_id: string | null;
  dependency_depth: number | null;
  logo: Logo | null;
}

export type GameSort = "name" | "-date_updated";
export type ModSort =
  | "-downloads_today"
  | "-downloads_total"
  | "-subscribers_total"
  | "-ratings_weighted_aggregate"
  | "-date_live"
  | "-date_updated"
  | "name";

/**
 * L'éditeur a fermé l'accès aux applications tierces : seul le site mod.io
 * (ou le jeu) peut lister et télécharger ses mods. Champ absent = inconnu.
 */
export function gameClosed(g: Game | null | undefined): boolean {
  return g?.api_access_options != null && (g.api_access_options & 1) === 0;
}

/**
 * Fichier à installer sur PC. Pour les jeux qui publient un fichier par
 * plateforme (PC, Quest…), `modfile` peut être vide : on prend alors le
 * fichier en ligne pour Windows (même règle que le backend).
 */
export function pcFileId(m: Mod | null | undefined): number | null {
  if (!m) return null;
  if (m.modfile?.id) return m.modfile.id;
  const pc = m.platforms.find((p) => p.platform?.toLowerCase() === "windows");
  return pc?.modfile_live || null;
}

// ------------------------------------------------------------------ Appels

export function asAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    return e as AppError;
  }
  return { kind: "unknown", message: String(e), retryAfter: null };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // Les messages du backend arrivent dans la langue de l'interface.
  await backendLocale;
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw asAppError(e);
  }
}

export const api = {
  getSettings: () => call<SettingsView>("get_settings"),

  saveSettings: (apiKey: string | null, apiPath: string) =>
    call<SettingsView>("save_settings", { apiKey, apiPath }),

  setHideMature: (value: boolean) => call<SettingsView>("set_hide_mature", { value }),

  listGames: (p: { query?: string; sort?: GameSort; offset: number; limit: number }) =>
    call<Page<Game>>("list_games", p),

  getGame: (gameId: number) => call<Game>("get_game", { gameId }),

  listMods: (p: {
    gameId: number;
    query?: string;
    tags?: string[];
    sort?: ModSort;
    offset: number;
    limit: number;
  }) => call<Page<Mod>>("list_mods", p),

  getMod: (gameId: number, modId: number) => call<Mod>("get_mod", { gameId, modId }),

  listModFiles: (gameId: number, modId: number) =>
    call<Modfile[]>("list_mod_files", { gameId, modId }),

  listDependencies: (gameId: number, modId: number) =>
    call<Dependency[]>("list_dependencies", { gameId, modId }),

  /** Ouvre une page du site officiel de mod.io dans une fenêtre de ModStudio. */
  openModioWindow: (url: string, title: string | null = null) => call<null>("open_modio_window", { url, title }),

  // ---------------------------------------------------------------- Compte

  authStatus: () => call<AuthView>("auth_status"),

  requestCode: (email: string) => call<null>("auth_request_code", { email }),

  exchangeCode: (code: string) => call<AuthView>("auth_exchange_code", { code }),

  useToken: (token: string) => call<AuthView>("auth_use_token", { token }),

  logout: () => call<AuthView>("auth_logout"),

  listMyMods: () => call<MyMods>("list_my_mods"),

  listSubscribedIds: (gameId: number) => call<number[]>("list_subscribed_ids", { gameId }),

  subscribe: (gameId: number, modId: number) => call<null>("subscribe_mod", { gameId, modId }),

  unsubscribe: (gameId: number, modId: number) => call<null>("unsubscribe_mod", { gameId, modId }),

  // ---------------------------------------------------------------- Installation

  installOverview: () => call<GameInstall[]>("install_overview"),

  setGameFolder: (
    gameId: number,
    gameName: string,
    icon: string | null,
    folder: string,
    layout: Layout,
    create = false,
  ) => call<GameInstall>("set_game_folder", { gameId, gameName, icon, folder, layout, create }),

  folderSuggestion: (gameId: number) => call<FolderSuggestion | null>("folder_suggestion", { gameId }),

  inspectFolder: (folder: string) => call<FolderInfo>("inspect_folder", { folder }),

  /** `requiredBy` : mod qui demande cette installation (mod nécessaire). */
  installMod: (gameId: number, modId: number, onProgress: (e: InstallEvent) => void, requiredBy: number | null = null) => {
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = onProgress;
    return call<InstalledMod>("install_mod", { gameId, modId, requiredBy, onEvent });
  },

  cancelInstall: (modId: number) => call<null>("cancel_install", { modId }),

  uninstallMod: (gameId: number, modId: number) => call<UninstallResult>("uninstall_mod", { gameId, modId }),

  latestInstalledFiles: (gameId: number) => call<LatestFile[]>("latest_installed_files", { gameId }),
  setModEnabled: (gameId: number, modId: number, enabled: boolean) =>
    call<GameInstall>("set_mod_enabled", { gameId, modId, enabled }),
  applyModProfile: (gameId: number, name: string) => call<ProfileApplied>("apply_mod_profile", { gameId, name }),
  editModProfiles: (gameId: number, edit: ProfileEdit) => call<GameInstall>("edit_mod_profiles", { gameId, edit }),

  revealInstall: (gameId: number, modId: number | null = null) => call<null>("reveal_install", { gameId, modId }),

  /** Le jeu installe-t-il ses mods lui-même via mod.io sur ce PC ? */
  detectModioInstall: (gameId: number) => call<ManagedByGame | null>("detect_modio_install", { gameId }),
};

export interface ManagedByGame {
  folder: string;
  mods: number;
}

export type Layout = "subfolder" | "flat" | "archive";

export interface InstalledMod {
  modId: number;
  name: string;
  logo: string | null;
  version: string | null;
  modfileId: number;
  size: number;
  installedAt: number;
  layout: Layout;
  root: string | null;
  files: string[];
  dirs: string[];
  /** Installé automatiquement comme mod nécessaire. */
  auto: boolean;
  requiredBy: number[];
  /** Fichiers rangés dans un autre dossier (dossier du jeu pour un mod de code). */
  parts?: { key: string; files: string[]; dirs: string[] }[];
  /** Nom du fichier choisi (Nexus Mods). */
  fileName?: string;
  fileUid?: string;
  /** Plugins activés dans `plugins.txt` (jeux Bethesda). */
  plugins?: string[];
  /** Mod désactivé : ses fichiers sont mis de côté. */
  disabled?: boolean;
}

/** Ensemble de mods activés d'un jeu. */
export interface ModProfile {
  name: string;
  mods: number[];
}

export type ProfileEdit =
  | { action: "create"; name: string }
  /** Profil importé : ces mods-là ; les autres sont désactivés en y passant. */
  | { action: "createWith"; name: string; mods: number[] }
  | { action: "rename"; name: string; newName: string }
  | { action: "delete"; name: string };

export interface ProfileApplied {
  game: GameInstall;
  /** Mods qui n'ont pas pu changer d'état (conflit, fichier ouvert…). */
  issues: { modId: number; name: string; message: string }[];
}

/** Outil de mods (UE4SS, BepInEx, MelonLoader) installé par ModStudio. */
export interface InstalledTool {
  id: string;
  name: string;
  key?: string;
  version: string | null;
  installedAt: number;
  files: string[];
  dirs: string[];
}

export interface FolderSuggestion {
  path: string;
  layout: Layout;
  exists: boolean;
}

export interface FolderInfo {
  exists: boolean;
  looksLikeGameRoot: boolean;
}

export interface GameInstall {
  gameId: number;
  gameName: string;
  icon: string | null;
  folder: string;
  layout: Layout;
  mods: Record<string, InstalledMod>;
  /** Règles d'installation propres au jeu (`bonelab`), à la place du rangement. */
  rules?: string;
  /** Dossiers supplémentaires (`game` : dossier d'installation du jeu). */
  extraFolders?: Record<string, string>;
  /** Identifiant texte du jeu sur Nexus Mods. */
  domain?: string;
  /** Outils de mods installés par ModStudio. */
  tools?: Record<string, InstalledTool>;
  profiles?: ModProfile[];
  activeProfile?: string;
}

// ------------------------------------------------------------------ Nexus Mods

export interface NexusGame {
  id: number;
  name: string;
  domainName: string;
  modCount: number;
  downloadCount: number;
  genre: string | null;
}

export interface NexusMod {
  /** Copie de `modId`, pour les listes paginées. */
  id: number;
  modId: number;
  uid: string | null;
  name: string;
  summary: string | null;
  author: string | null;
  adultContent: boolean;
  endorsements: number;
  downloads: number;
  updatedAt: string | null;
  createdAt: string | null;
  version: string | null;
  category: string | null;
  thumbnailUrl: string | null;
  pictureUrl: string | null;
  status: string | null;
  uploader: { name: string | null; avatar: string | null; memberId: number } | null;
  game: { id: number; domainName: string; name: string | null } | null;
}

export interface NexusRequirement {
  modId: number;
  gameId: number;
  modName: string;
  url: string | null;
  externalRequirement: boolean;
  notes: string | null;
}

export interface NexusModDetail extends NexusMod {
  description: string | null;
  requirements: NexusRequirement[];
}

export type NexusFileCategory =
  | "MAIN"
  | "OPTIONAL"
  | "UPDATE"
  | "MISCELLANEOUS"
  | "OLD_VERSION"
  | "ARCHIVED"
  | "REMOVED";

export interface NexusFile {
  fileId: number;
  uid: string | null;
  name: string;
  version: string | null;
  category: NexusFileCategory | string;
  /** Secondes Unix. */
  date: number;
  sizeInBytes: number;
  size: number;
  description: string | null;
  primary: boolean;
  uri: string | null;
}

export interface NexusUser {
  memberId: number;
  name: string;
  isPremium: boolean;
  isSupporter: boolean;
  profileUrl: string | null;
}

export interface NexusAuthView {
  loggedIn: boolean;
  user: NexusUser | null;
  persisted: boolean;
  storageWarning: string | null;
  rejected: boolean;
}

export type Launcher = "steam" | "epic";

/** Jeu trouvé dans un launcher (Steam, Epic). */
export interface InstalledGame {
  launcher: Launcher;
  appId: string;
  name: string;
  path: string;
}

/** Jeu de la liste publiée par Nexus Mods. */
export interface CatalogGame {
  id: number;
  name: string;
  domain_name: string;
  mods: number;
}

export interface DetectedGame extends InstalledGame {
  nexus: CatalogGame | null;
}

export type GameEngine = "unreal" | "unity-mono" | "unity-il2cpp" | "bethesda" | "unknown";

export interface EngineInfo {
  engine: GameEngine;
  project: string | null;
  arch: "x64" | "x86";
  exeVersion: string | null;
  /** Outils présents dans le dossier du jeu (`ue4ss`, `bepinex`, `melonloader`, `skse64`…). */
  tools: string[];
}

/** Outil de mods conseillé pour un jeu. */
/** Un mod Nexus désigné par son jeu (`skyrimspecialedition`) et son numéro. */
export interface ModRef {
  domain: string;
  modId: number;
}

/** Mods suivis et avis donnés sur Nexus Mods. */
export interface NexusSocial {
  tracked: ModRef[];
  endorsed: ModRef[];
  /** « Ne pas recommander ». */
  abstained: ModRef[];
}

/** Chargeur de mods trouvé dans le dossier d'un jeu, installé sans ModStudio. */
export interface FoundLoader {
  id: string;
  name: string;
}

export interface ToolOffer {
  id: string;
  name: string;
  description: string;
  present: boolean;
  /** Installé par ModStudio : peut être retiré depuis ModStudio. */
  managed: boolean;
  /** Script extender : sa page Nexus (il s'installe comme un mod). */
  nexusMod: number | null;
  gameVersion: string | null;
  /** Mods populaires du jeu qui en ont besoin, sur `sampled` examinés. */
  popular: number;
  sampled: number;
  /** Version installée par ModStudio. */
  installedVersion: string | null;
  /** Autre version disponible (ou demandée par les mods du jeu). */
  update: string | null;
  /** Version précise demandée par les mods du jeu (`6.0.0-be.791`). */
  requiredVersion: string | null;
  /** Sous Linux (Proton) : options de lancement pour que le chargeur soit chargé. */
  launchOption?: string | null;
}

export interface NexusGameSetup {
  config: GameInstall | null;
  rules: string | null;
  suggestedFolder: string | null;
  suggestedFolderExists: boolean;
  gameDir: string | null;
  gameDirDetected: boolean;
  melonloader: boolean;
  /** Le jeu trouvé dans Steam ou Epic. */
  installed: InstalledGame | null;
  engine: EngineInfo | null;
  tool: ToolOffer | null;
  /** Rangement automatique qu'aurait le jeu dans son dossier d'installation trouvé (sinon celui du jeu configuré). */
  autoRules: string | null;
}

export interface NexusInstalled {
  record: InstalledMod;
  notice: string | null;
  /** Outil absent sans lequel ce mod ne se chargera pas. */
  needsTool: ToolOffer | null;
}

// ------------------------------------------------------------------ FOMOD

/** Condition d'un installateur FOMOD sur les choix déjà faits. */
export type FomodDep =
  | { kind: "all"; items: FomodDep[] }
  | { kind: "any"; items: FomodDep[] }
  | { kind: "flag"; flag: string; value: string }
  | { kind: "const"; value: boolean };

export type FomodPluginType = "Required" | "Recommended" | "Optional" | "NotUsable" | "CouldBeUsable" | string;

export interface FomodPlugin {
  name: string;
  description: string;
  /** Image en `data:`. */
  image: string | null;
  flags: { name: string; value: string }[];
  typeDefault: FomodPluginType;
  typePatterns: { dep: FomodDep; type: FomodPluginType }[];
}

export type FomodGroupType = "SelectExactlyOne" | "SelectAtMostOne" | "SelectAtLeastOne" | "SelectAll" | "SelectAny" | string;

export interface FomodGroup {
  name: string;
  type: FomodGroupType;
  plugins: FomodPlugin[];
}

export interface FomodStep {
  name: string;
  visible: FomodDep | null;
  groups: FomodGroup[];
}

export interface FomodInstaller {
  moduleName: string;
  image: string | null;
  steps: FomodStep[];
}

/** Installation d'un fichier Nexus : installé, ou options FOMOD à choisir. */
export type NexusInstallOutcome =
  | ({ status: "installed" } & NexusInstalled)
  | { status: "choices"; token: string; installer: FomodInstaller };

export interface NexusUpdate {
  modId: number;
  superseded: boolean;
  category: string;
}

/** Lien `nxm://` capté dans la fenêtre de téléchargement. */
export interface NxmLink {
  domain: string;
  modId: number;
  fileId: number;
  key: string;
  expires: number;
}

/** Zone de la fenêtre principale où afficher une page Nexus (pixels CSS). */
export interface SiteBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** `embedded` : page intégrée à ModStudio ; `window` : fenêtre séparée (repli). */
export type SitePlacement = "embedded" | "window";

export type NexusGameSort = "downloads" | "mods" | "name";
export type NexusModSort = "endorsements" | "downloads" | "updated" | "created" | "name";

/** Images d'un jeu sur Nexus : `tile` (portrait), `hero` (bannière), `thumbnail` (icône). */
export function nexusGameImage(gameId: number, kind: "tile" | "hero" | "thumbnail"): string {
  return `https://images.nexusmods.com/images/games/v2/${gameId}/${kind}.jpg`;
}

export function nexusModUrl(domain: string, modId: number): string {
  return `https://www.nexusmods.com/${domain}/mods/${modId}`;
}

function withId<T extends { modId: number }>(m: T): T & { id: number } {
  return { ...m, id: m.modId };
}

export const nexus = {
  status: () => call<NexusAuthView>("nexus_status"),
  setKey: (key: string) => call<NexusAuthView>("nexus_set_key", { key }),
  forgetKey: () => call<NexusAuthView>("nexus_forget_key"),

  listGames: (p: { query?: string; sort?: NexusGameSort; offset: number; limit: number }) =>
    call<Page<NexusGame>>("nexus_list_games", p),
  getGame: (domain: string) => call<NexusGame>("nexus_get_game", { domain }),
  listMods: async (p: { gameDomain: string; query?: string; sort?: NexusModSort; offset: number; limit: number }) => {
    const page = await call<Page<NexusMod>>("nexus_list_mods", p);
    return { ...page, data: page.data.map(withId) };
  },
  getMod: async (gameId: number, modId: number) => withId(await call<NexusModDetail>("nexus_get_mod", { gameId, modId })),
  listFiles: (gameId: number, modId: number) => call<NexusFile[]>("nexus_list_files", { gameId, modId }),
  /** Page d'un fichier dans ModStudio (`bounds` : zone où l'intégrer). */
  openDownload: (domain: string, modId: number, fileId: number, bounds: SiteBounds | null = null) =>
    call<SitePlacement>("nexus_open_download", { domain, modId, fileId, bounds }),
  /** Page de connexion / clés API dans ModStudio. */
  openLogin: (bounds: SiteBounds | null = null) => call<SitePlacement>("nexus_open_login", { bounds }),
  siteBounds: (bounds: SiteBounds) => call<null>("nexus_site_bounds", { bounds }),
  siteClose: () => call<null>("nexus_site_close"),

  overview: () => call<GameInstall[]>("nexus_overview"),
  social: () => call<NexusSocial>("nexus_social"),
  setTracked: (domain: string, modId: number, tracked: boolean) =>
    call<null>("nexus_set_tracked", { domain, modId, tracked }),
  /** Recommande (`endorse`) ou « ne pas recommander » ; renvoie `Endorsed` / `Abstained`. */
  endorse: (domain: string, modId: number, version: string | null, endorse: boolean) =>
    call<string>("nexus_endorse", { domain, modId, version, endorse }),
  trackedMods: () => call<NexusMod[]>("nexus_tracked_mods"),
  foundLoaders: () => call<Record<number, FoundLoader[]>>("nexus_found_loaders"),
  gameSetup: (gameId: number, domain: string) => call<NexusGameSetup>("nexus_game_setup", { gameId, domain }),
  detectGames: (refresh = false) => call<DetectedGame[]>("nexus_detect_games", { refresh }),
  installTool: (gameId: number, tool: string, onProgress: (e: InstallEvent) => void) => {
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = onProgress;
    return call<InstalledTool>("nexus_install_tool", { gameId, tool, onEvent });
  },
  cancelTool: (gameId: number) => call<null>("nexus_cancel_tool", { gameId }),
  uninstallTool: (gameId: number, tool: string) => call<string | null>("nexus_uninstall_tool", { gameId, tool }),
  configureGame: (p: {
    gameId: number;
    domain: string;
    gameName: string;
    icon: string | null;
    folder: string;
    layout: Layout;
    create?: boolean;
    gameDir?: string | null;
  }) => call<GameInstall>("nexus_configure_game", p),
  install: (
    p: {
      gameId: number;
      gameDomain: string;
      modId: number;
      fileId: number;
      nxmKey?: string;
      nxmExpires?: number;
      /** Mod qui a besoin de celui-ci (installation d'un mod nécessaire). */
      requiredBy?: number;
    },
    onProgress: (e: InstallEvent) => void,
  ) => {
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = onProgress;
    return call<NexusInstallOutcome>("nexus_install", { ...p, onEvent });
  },
  /** Termine une installation FOMOD : `selections[étape][groupe]` = options cochées. */
  fomodFinish: (token: string, selections: number[][][], onProgress: (e: InstallEvent) => void) => {
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = onProgress;
    // Une archive choisie peut à son tour proposer des options : nouveau choix.
    return call<NexusInstallOutcome>("nexus_fomod_finish", { token, selections, onEvent });
  },
  fomodCancel: (token: string) => call<null>("nexus_fomod_cancel", { token }),
  cancelInstall: (modId: number) => call<null>("nexus_cancel_install", { modId }),
  uninstall: (gameId: number, modId: number) => call<UninstallResult>("nexus_uninstall", { gameId, modId }),
  updates: (gameId: number) => call<NexusUpdate[]>("nexus_updates", { gameId }),
  setEnabled: (gameId: number, modId: number, enabled: boolean) =>
    call<GameInstall>("nexus_set_enabled", { gameId, modId, enabled }),
  applyProfile: (gameId: number, name: string) => call<ProfileApplied>("nexus_apply_profile", { gameId, name }),
  editProfiles: (gameId: number, edit: ProfileEdit) => call<GameInstall>("nexus_edit_profiles", { gameId, edit }),
  reveal: (gameId: number, modId: number | null = null) => call<null>("nexus_reveal", { gameId, modId }),
};

export type InstallEvent =
  | { phase: "preparing" }
  | { phase: "downloading"; received: number; total: number }
  | { phase: "verifying" }
  | { phase: "extracting"; done: number; total: number }
  | { phase: "finishing" }
  | { phase: "step"; name: string; index: number; total: number };

export interface LatestFile {
  modId: number;
  modfileId: number;
  version: string | null;
}

export interface UninstallResult {
  leftFolder: string | null;
  /** Mods nécessaires installés avec celui-ci et devenus inutiles. */
  orphans: { modId: number; name: string }[];
}

export interface AuthView {
  loggedIn: boolean;
  user: User | null;
  source: "email" | "manual" | null;
  persisted: boolean;
  storageWarning: string | null;
  expired: boolean;
}

export interface MyMods {
  games: Game[];
  mods: Mod[];
  truncated: boolean;
}

// ------------------------------------------------------------------ Thunderstore

export interface TsCommunity {
  /** Identifiant du jeu dans ModStudio (bibliothèque des mods installés). */
  id: number;
  identifier: string;
  name: string;
  coverImageUrl: string | null;
  heroImageUrl: string | null;
  communityIconUrl: string | null;
  totalDownloadCount: number;
  totalPackageCount: number;
  /** Chargeur de mods du jeu (« BepInEx », « MelonLoader », « Shimloader (UE4SS) »…). */
  loader: string | null;
  /** ModStudio sait installer ses mods. */
  supported: boolean;
}

export interface TsCategory {
  name: string;
  slug: string;
}

export interface TsPackage {
  /** Identifiant du mod dans ModStudio (`Équipe-Nom`). */
  id: number;
  namespace: string;
  name: string;
  description: string;
  iconUrl: string | null;
  downloadCount: number;
  ratingCount: number;
  categories: TsCategory[];
  isDeprecated: boolean;
  isNsfw: boolean;
  isPinned: boolean;
  lastUpdated: string | null;
  size: number;
}

export interface TsDependency {
  id: number;
  namespace: string;
  name: string;
  versionNumber: string;
  description: string;
  iconUrl: string | null;
  isRemoved: boolean;
  isUnavailable: boolean;
}

export interface TsPackageDetail {
  id: number;
  namespace: string;
  name: string;
  description: string;
  iconUrl: string | null;
  downloadCount: number;
  ratingCount: number;
  categories: TsCategory[];
  communityIdentifier: string;
  communityName: string;
  dependencies: TsDependency[];
  latestVersionNumber: string;
  websiteUrl: string | null;
  isDeprecated: boolean;
  isNsfw: boolean;
  versionCount: number;
  versionCreated: string | null;
  packageCreated: string | null;
  size: number;
  team: { name: string } | null;
  dependantCount: number;
}

export interface TsVersion {
  versionNumber: string;
  datetimeCreated: string | null;
  downloadCount: number;
}

export interface TsPackageView {
  detail: TsPackageDetail;
  versions: TsVersion[];
  readme: string | null;
}

export type TsOrdering = "most-downloaded" | "top-rated" | "newest" | "last-updated";

export interface TsInstalled {
  game: GameInstall;
  /** Mods installés, dépendances d'abord. */
  installed: string[];
  /** BepInEx était déjà là : le paquet BepInExPack demandé n'a pas été installé. */
  keptLoader: boolean;
}

export interface TsUpdate {
  modId: number;
  version: string;
}

export interface TsLoader {
  /** Chargeur que le jeu demande (« bepinex », « melonloader », « shimloader »…), null : inconnu. */
  required: string | null;
  /** Son nom affiché (« BepInEx 6 (IL2CPP) », « MelonLoader »…). */
  requiredLabel: string | null;
  /** ModStudio sait installer les mods de ce jeu. */
  supported: boolean;
  /** Chargeur présent : « BepInEx 5 », « BepInEx 6 (IL2CPP) », « MelonLoader »… */
  name: string | null;
  /** Le chargeur présent peut charger les mods de ce jeu. */
  suitable: boolean;
  engine: "il2cpp" | "mono" | null;
  /** Installé par ModStudio : remplaçable sans risque. */
  managed: boolean;
  /** Paquet à installer : chargeur absent, ou qui ne convient pas. */
  needed: { id: number; namespace: string; name: string } | null;
  launchOption: string | null;
  /** Option de lancement que le chargeur demande sur tous les systèmes (jeux Godot). */
  launchArgs: string | null;
}

export const ts = {
  communities: (refresh = false) => call<TsCommunity[]>("ts_communities", { refresh }),
  packages: (p: { community: string; query: string; ordering: TsOrdering; offset: number; limit: number }) =>
    call<Page<TsPackage>>("ts_packages", p),
  package: (community: string, namespace: string, name: string) =>
    call<TsPackageView>("ts_package", { community, namespace, name }),
  overview: () => call<GameInstall[]>("ts_overview"),
  configure: (community: string, name: string, icon: string | null, folder: string) =>
    call<GameInstall>("ts_configure", { community, name, icon, folder }),
  install: (
    p: { community: string; namespace: string; name: string; version?: string | null },
    onProgress: (e: InstallEvent) => void,
  ) => {
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = onProgress;
    return call<TsInstalled>("ts_install", { ...p, onEvent });
  },
  cancel: (community: string) => call<null>("ts_cancel", { community }),
  uninstall: (gameId: number, modId: number) => call<UninstallResult>("ts_uninstall", { gameId, modId }),
  setEnabled: (gameId: number, modId: number, enabled: boolean) =>
    call<GameInstall>("ts_set_enabled", { gameId, modId, enabled }),
  applyProfile: (gameId: number, name: string) => call<ProfileApplied>("ts_apply_profile", { gameId, name }),
  editProfiles: (gameId: number, edit: ProfileEdit) => call<GameInstall>("ts_edit_profiles", { gameId, edit }),
  updates: (gameId: number) => call<TsUpdate[]>("ts_updates", { gameId }),
  loader: (gameId: number) => call<TsLoader>("ts_loader", { gameId }),
  reveal: (gameId: number, modId: number | null = null) => call<null>("ts_reveal", { gameId, modId }),
};

// ---------------------------------------------------------------- ResoniteModLoader

/** Mod demandé ou en conflit. */
export interface RmlDep {
  id: string;
  name: string;
  /** Version demandée (`^2.4.3`, `*`…). */
  version: string;
  optional: boolean;
}

export interface RmlVersion {
  version: string;
  changelog: string | null;
  releaseUrl: string | null;
  dependencies: RmlDep[];
  conflicts: RmlDep[];
  /** Où vont ses fichiers, relatifs au dossier du jeu. */
  files: string[];
}

/** Mod de la liste vérifiée de ResoniteModLoader. */
export interface RmlMod {
  id: string;
  /** Identifiant dans la bibliothèque Thunderstore de Resonite. */
  modId: number;
  name: string;
  description: string;
  category: string;
  author: string;
  authorUrl: string | null;
  /** Portrait de l'auteur (celui de la liste, sinon de son compte GitHub ou Codeberg). */
  authorIcon: string | null;
  /** Portrait de son compte, si celui de la liste ne s'affiche pas. */
  authorAvatar: string | null;
  source: string | null;
  website: string | null;
  tags: string[];
  deprecated: boolean;
  broken: boolean;
  platforms: string[];
  /** Peut se charger dans le jeu sur ce PC. */
  compatible: boolean;
  latest: string;
  /** La plus récente d'abord. */
  versions: RmlVersion[];
}

/** ResoniteModLoader dans le dossier de Resonite. */
export interface RmlStatus {
  present: boolean;
  /** Installé par ModStudio. */
  managed: boolean;
  version: string | null;
  /** Chargé par BepisLoader (Resonite Mod Loader Loader). */
  viaBepis: boolean;
  launchOption: string;
}

export const rml = {
  mods: (refresh = false) => call<RmlMod[]>("rml_mods", { refresh }),
  status: () => call<RmlStatus | null>("rml_status"),
  install: (id: string, version: string | null, onProgress: (e: InstallEvent) => void) => {
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = onProgress;
    return call<TsInstalled>("rml_install", { id, version, onEvent });
  },
};

/** `rml:<id>` : mod de ResoniteModLoader dans la bibliothèque Thunderstore. */
export function rmlIdOf(fileName: string | null | undefined): string | null {
  return fileName?.startsWith("rml:") ? fileName.slice(4) : null;
}

// ---------------------------------------------------------------- Listes des communautés

/** Jeu dont la communauté tient sa propre liste de mods (Hollow Knight, Beat Saber…). */
export interface ClGame {
  id: string;
  gameId: number;
  name: string;
  listName: string;
  listUrl: string;
  /** Chargeur de mods du jeu. */
  loader: string;
  steamAppId: number;
  cover: string;
  hero: string;
  about: string;
}

export interface ClDep {
  id: string;
  name: string;
  version: string;
  optional: boolean;
}

export interface ClVersion {
  version: string;
  changelog: string | null;
  releaseUrl: string | null;
  dependencies: ClDep[];
  conflicts: ClDep[];
  gameVersions: string[];
  files: string[];
}

export interface ClMod {
  id: string;
  modId: number;
  name: string;
  description: string;
  category: string | null;
  author: string;
  authorUrl: string | null;
  icon: string | null;
  iconFallback: string | null;
  page: string | null;
  tags: string[];
  deprecated: boolean;
  broken: boolean;
  compatible: boolean;
  note: string | null;
  /** C'est le chargeur de mods du jeu. */
  loader: boolean;
  downloads: number | null;
  updated: string | null;
  latest: string;
  versions: ClVersion[];
}

export interface ClLoader {
  name: string;
  present: boolean;
  managed: boolean;
  version: string | null;
  installable: boolean;
  id: string | null;
  latest: string | null;
  note: string | null;
  launchOption: string | null;
  gameVersion: string | null;
}

export interface ClInstalled {
  game: GameInstall;
  installed: string[];
}

export const cl = {
  games: () => call<ClGame[]>("cl_games"),
  overview: () => call<GameInstall[]>("cl_overview"),
  mods: (game: string, refresh = false) => call<ClMod[]>("cl_mods", { game, refresh }),
  configure: (game: string, folder: string) => call<GameInstall>("cl_configure", { game, folder }),
  loader: (game: string) => call<ClLoader>("cl_loader", { game }),
  install: (game: string, id: string, version: string | null, onProgress: (e: InstallEvent) => void) => {
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = onProgress;
    return call<ClInstalled>("cl_install", { game, id, version, onEvent });
  },
  cancel: (game: string) => call<null>("cl_cancel", { game }),
  uninstall: (gameId: number, modId: number) => call<UninstallResult>("cl_uninstall", { gameId, modId }),
  setEnabled: (gameId: number, modId: number, enabled: boolean) =>
    call<GameInstall>("cl_set_enabled", { gameId, modId, enabled }),
  applyProfile: (gameId: number, name: string) => call<ProfileApplied>("cl_apply_profile", { gameId, name }),
  editProfiles: (gameId: number, edit: ProfileEdit) => call<GameInstall>("cl_edit_profiles", { gameId, edit }),
  updates: (gameId: number) => call<TsUpdate[]>("cl_updates", { gameId }),
  launch: (game: string) => call<null>("cl_launch", { game }),
  reveal: (gameId: number, modId: number | null = null) => call<null>("cl_reveal", { gameId, modId }),
};

/** `<jeu>:<identifiant>` : mod d'une liste de communauté. */
export function clIdOf(fileName: string | null | undefined): string | null {
  const i = fileName?.indexOf(":") ?? -1;
  return fileName && i > 0 ? fileName.slice(i + 1) : null;
}

/** Page d'un mod sur le site de Thunderstore. */
export function tsPackageUrl(community: string, namespace: string, name: string) {
  return `https://thunderstore.io/c/${community}/p/${namespace}/${name}/`;
}

// ---------------------------------------------------------------- Profils partagés

/** Source des mods d'un profil partagé. */
export type ShareSource = "modio" | "nexus" | "ts" | "cl";

export interface SharedMod {
  /** mod.io et Nexus : numéro du mod ; Thunderstore : `Équipe-Nom` ou `rml:<id>` ; communautés : identifiant. */
  id: string;
  name: string;
  version?: string | null;
  /** Fichier installé (mod.io : modfile ; Nexus : fichier). */
  file?: number | null;
  /** Installé comme mod nécessaire d'un autre. */
  dep?: boolean;
}

export interface SharedProfile {
  name: string;
  source: ShareSource;
  /** `id` : jeu mod.io ou Nexus ; `key` : domaine Nexus, jeu Thunderstore (vide : inconnu) ou jeu des communautés. */
  game: { id: number; key: string; name: string };
  mods: SharedMod[];
}

export interface ShareImport {
  profile: SharedProfile;
  origin: "modstudio" | "r2modman";
  /** Fichiers de réglages joints (profil r2modman). */
  configs: number;
  token: string | null;
  /** Mods désactivés dans le profil r2modman (pas installés). */
  skipped: string[];
}

export interface ShareExport {
  profile: SharedProfile;
  code: string;
  /** Mods laissés de côté (désactivés). */
  leftOut: number;
}

export interface ConfigsApplied {
  written: number;
  /** Dossier qui garde les réglages remplacés. */
  backup: string | null;
}

export const share = {
  export: (source: ShareSource, gameId: number) => call<ShareExport>("share_export", { source, gameId }),
  decode: (text: string) => call<ShareImport>("share_decode", { text }),
  openFile: (path: string) => call<ShareImport>("share_open_file", { path }),
  saveFile: (path: string, profile: SharedProfile) => call<null>("share_save_file", { path, profile }),
  /** Publie un profil r2modman (jeu Thunderstore) : renvoie son code. */
  r2modman: (gameId: number, name: string, includeConfigs: boolean) =>
    call<string>("share_r2modman_create", { gameId, name, includeConfigs }),
  applyConfigs: (token: string, gameId: number) => call<ConfigsApplied>("share_apply_configs", { token, gameId }),
};
