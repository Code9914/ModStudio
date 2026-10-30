// Navigation de l'appli (pas de routeur : un seul écran avec des vues).
import { SvelteSet } from "svelte/reactivity";
import { api, type AuthView, type Game, type NexusGame, type SettingsView, type TsCommunity } from "./api";

export type View =
  | { name: "games" }
  | { name: "mods" }
  | { name: "mod"; modId: number }
  | { name: "library" }
  | { name: "installed" }
  | { name: "account" }
  | { name: "settings" }
  | { name: "nexusGames" }
  | { name: "nexusMods" }
  | { name: "nexusMod"; modId: number }
  | { name: "nexusAccount" }
  | { name: "nexusTracked" }
  | { name: "tsGames" }
  | { name: "tsMods" }
  | { name: "tsMod"; namespace: string; modName: string }
  | { name: "rmlMod"; id: string };

export type ModOrigin = "mods" | "library" | "installed";
export type NexusOrigin = "nexusMods" | "installed" | "nexusTracked";
export type TsOrigin = "tsMods" | "installed";
export type TsRef = { namespace: string; name: string };

class Nav {
  view = $state<View>({ name: "games" });
  /** Jeu ouvert (sa liste de mods reste montée pour garder le défilement). */
  game = $state<Game | null>(null);
  /** Mods ouverts depuis la liste, puis via les dépendances. */
  modStack = $state<number[]>([]);
  /** Vue où revenir en quittant la fiche d'un mod. */
  modOrigin = $state<ModOrigin>("mods");
  /** Vue où revenir après la connexion. */
  afterLogin = $state<View | null>(null);
  settings = $state<SettingsView | null>(null);
  auth = $state<AuthView | null>(null);
  /** Change quand la clé ou les filtres changent : les listes se rechargent. */
  session = $state(0);

  /** Jeu Nexus ouvert, et mods ouverts depuis sa liste puis via les mods nécessaires. */
  nexusGame = $state<NexusGame | null>(null);
  nexusModStack = $state<number[]>([]);
  nexusOrigin = $state<NexusOrigin>("nexusMods");
  /** Vue où revenir après avoir ajouté la clé Nexus. */
  afterNexusKey = $state<View | null>(null);
  /** Page du compte Nexus affichée comme accueil du premier lancement. */
  nexusWelcome = $state(false);

  /** Jeu Thunderstore ouvert, et mods ouverts depuis sa liste puis via leurs dépendances. */
  tsGame = $state<TsCommunity | null>(null);
  tsModStack = $state<TsRef[]>([]);
  tsOrigin = $state<TsOrigin>("tsMods");
  /** Onglet de la page de Resonite : paquets Thunderstore ou mods de ResoniteModLoader. */
  tsTab = $state<"thunderstore" | "rml">("thunderstore");
  /** Mods de ResoniteModLoader ouverts (puis via leurs dépendances). */
  rmlStack = $state<string[]>([]);
  rmlOrigin = $state<TsOrigin>("tsMods");

  /** Mods suivis dans le jeu ouvert. */
  subs = new SvelteSet<number>();
  subsGameId = $state<number | null>(null);
  #subsSeq = 0;

  get loggedIn() {
    return this.auth?.loggedIn === true;
  }

  openGames() {
    this.view = { name: "games" };
  }

  openGame(game: Game) {
    if (this.game?.id !== game.id) this.game = game;
    this.modStack = [];
    this.view = { name: "mods" };
  }

  openMod(modId: number) {
    if (this.view.name !== "mod") this.modOrigin = "mods";
    this.modStack = [...this.modStack, modId];
    this.view = { name: "mod", modId };
  }

  /** Ouvre la fiche d'un mod depuis « Mes mods » ou « Installés ». */
  openModFromLibrary(game: Game, modId: number, origin: ModOrigin = "library") {
    if (this.game?.id !== game.id) this.game = game;
    this.modOrigin = origin;
    this.modStack = [modId];
    this.view = { name: "mod", modId };
  }

  openInstalled() {
    this.view = { name: "installed" };
  }

  // ------------------------------------------------------------ Nexus Mods

  openNexusGames() {
    this.view = { name: "nexusGames" };
  }

  openNexusGame(game: NexusGame) {
    if (this.nexusGame?.id !== game.id) this.nexusGame = game;
    this.nexusModStack = [];
    this.view = { name: "nexusMods" };
  }

  openNexusMod(modId: number) {
    if (this.view.name !== "nexusMod") this.nexusOrigin = "nexusMods";
    this.nexusModStack = [...this.nexusModStack, modId];
    this.view = { name: "nexusMod", modId };
  }

  /** Fiche d'un mod Nexus ouverte depuis « Installés ». */
  openNexusModFromInstalled(game: NexusGame, modId: number) {
    this.openNexusModFrom(game, modId, "installed");
  }

  /** Fiche d'un mod Nexus ouverte depuis une autre liste que celle de son jeu. */
  openNexusModFrom(game: NexusGame, modId: number, origin: NexusOrigin) {
    if (this.nexusGame?.id !== game.id) this.nexusGame = game;
    this.nexusOrigin = origin;
    this.nexusModStack = [modId];
    this.view = { name: "nexusMod", modId };
  }

  /** Mods suivis sur Nexus Mods. */
  openNexusTracked() {
    this.view = { name: "nexusTracked" };
  }

  // ------------------------------------------------------------ Thunderstore

  openTsGames() {
    this.view = { name: "tsGames" };
  }

  openTsGame(game: TsCommunity) {
    if (this.tsGame?.id !== game.id) {
      this.tsGame = game;
      this.tsTab = "thunderstore";
    }
    this.tsModStack = [];
    this.view = { name: "tsMods" };
  }

  openTsMod(ref: TsRef) {
    if (this.view.name !== "tsMod") this.tsOrigin = "tsMods";
    this.tsModStack = [...this.tsModStack, ref];
    this.view = { name: "tsMod", namespace: ref.namespace, modName: ref.name };
  }

  /** Fiche d'un mod Thunderstore ouverte depuis « Installés ». */
  openTsModFromInstalled(game: TsCommunity, ref: TsRef) {
    if (this.tsGame?.id !== game.id) this.tsGame = game;
    this.tsOrigin = "installed";
    this.tsModStack = [ref];
    this.view = { name: "tsMod", namespace: ref.namespace, modName: ref.name };
  }

  /** Fiche d'un mod de ResoniteModLoader (onglet de la page de Resonite). */
  openRmlMod(id: string) {
    if (this.view.name !== "rmlMod") this.rmlOrigin = "tsMods";
    this.rmlStack = [...this.rmlStack, id];
    this.view = { name: "rmlMod", id };
  }

  /** Fiche d'un mod de ResoniteModLoader ouverte depuis « Installés ». */
  openRmlModFromInstalled(game: TsCommunity, id: string) {
    if (this.tsGame?.id !== game.id) this.tsGame = game;
    this.tsTab = "rml";
    this.rmlOrigin = "installed";
    this.rmlStack = [id];
    this.view = { name: "rmlMod", id };
  }

  openNexusAccount(returnTo: View | null = null) {
    this.afterNexusKey = returnTo;
    this.nexusWelcome = false;
    this.view = { name: "nexusAccount" };
  }

  /** Accueil du premier lancement : connexion à Nexus Mods. */
  openNexusWelcome() {
    this.afterNexusKey = null;
    this.nexusWelcome = true;
    this.view = { name: "nexusAccount" };
  }

  back() {
    if (this.view.name === "rmlMod" && this.rmlStack.length > 1) {
      this.rmlStack = this.rmlStack.slice(0, -1);
      this.view = { name: "rmlMod", id: this.rmlStack[this.rmlStack.length - 1] };
      return;
    }
    if (this.view.name === "rmlMod") {
      this.rmlStack = [];
      this.view = { name: this.rmlOrigin };
      return;
    }
    if (this.view.name === "tsMod" && this.tsModStack.length > 1) {
      this.tsModStack = this.tsModStack.slice(0, -1);
      const top = this.tsModStack[this.tsModStack.length - 1];
      this.view = { name: "tsMod", namespace: top.namespace, modName: top.name };
      return;
    }
    if (this.view.name === "tsMod") {
      this.tsModStack = [];
      this.view = { name: this.tsOrigin };
      return;
    }
    if (this.view.name === "tsMods") {
      this.view = { name: "tsGames" };
      return;
    }
    if (this.view.name === "nexusMod" && this.nexusModStack.length > 1) {
      this.nexusModStack = this.nexusModStack.slice(0, -1);
      this.view = { name: "nexusMod", modId: this.nexusModStack[this.nexusModStack.length - 1] };
      return;
    }
    if (this.view.name === "nexusMod") {
      this.nexusModStack = [];
      this.view = { name: this.nexusOrigin };
      return;
    }
    if (this.view.name === "nexusMods") {
      this.view = { name: "nexusGames" };
      return;
    }
    if (this.view.name === "mod" && this.modStack.length > 1) {
      this.modStack = this.modStack.slice(0, -1);
      this.view = { name: "mod", modId: this.modStack[this.modStack.length - 1] };
    } else if (this.view.name === "mod") {
      this.modStack = [];
      this.view = { name: this.modOrigin };
    } else {
      this.view = { name: "games" };
    }
  }

  openLibrary() {
    this.view = { name: "library" };
  }

  /** Ouvre la connexion, puis revient à `returnTo` une fois connecté. */
  openAccount(returnTo: View | null = null) {
    this.afterLogin = returnTo;
    this.view = { name: "account" };
  }

  openSettings() {
    this.view = { name: "settings" };
  }

  /** Après un changement de clé ou de filtre adulte. */
  reset(keepGame: boolean) {
    this.session += 1;
    if (!keepGame) this.game = null;
    this.modStack = [];
  }

  setAuth(auth: AuthView) {
    const changed = auth.loggedIn !== this.auth?.loggedIn || auth.user?.id !== this.auth?.user?.id;
    this.auth = auth;
    if (changed) this.reloadSubs();
  }

  /** Charge les mods suivis du jeu ouvert. */
  async loadSubs(gameId: number) {
    const seq = ++this.#subsSeq;
    this.subsGameId = gameId;
    this.subs.clear();
    if (!this.loggedIn) return;
    try {
      const ids = await api.listSubscribedIds(gameId);
      if (seq !== this.#subsSeq) return;
      for (const id of ids) this.subs.add(id);
    } catch (e) {
      // Session expirée : l'état de connexion est rafraîchi pour l'interface.
      if ((e as { kind?: string }).kind === "session_expired") this.refreshAuth();
    }
  }

  reloadSubs() {
    if (this.subsGameId !== null) this.loadSubs(this.subsGameId);
    else this.subs.clear();
  }

  async refreshAuth() {
    try {
      this.setAuth(await api.authStatus());
    } catch {
      /* hors ligne : on garde l'état connu */
    }
  }
}

export const nav = new Nav();
