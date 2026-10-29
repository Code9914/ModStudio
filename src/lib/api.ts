// Appels au backend Rust (voir src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";

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
};
