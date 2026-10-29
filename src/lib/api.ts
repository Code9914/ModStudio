// Appels au backend Rust (voir src-tauri/src/commands.rs).
import { Channel, invoke } from "@tauri-apps/api/core";

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
}

export type InstallEvent =
  | { phase: "preparing" }
  | { phase: "downloading"; received: number; total: number }
  | { phase: "verifying" }
  | { phase: "extracting"; done: number; total: number }
  | { phase: "finishing" };

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
