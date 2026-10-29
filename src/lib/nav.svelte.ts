// Navigation de l'appli (pas de routeur : un seul écran avec des vues).
import { SvelteSet } from "svelte/reactivity";
import { api, type AuthView, type Game, type SettingsView } from "./api";

export type View =
  | { name: "games" }
  | { name: "mods" }
  | { name: "mod"; modId: number }
  | { name: "library" }
  | { name: "account" }
  | { name: "settings" };

class Nav {
  view = $state<View>({ name: "games" });
  /** Jeu ouvert (sa liste de mods reste montée pour garder le défilement). */
  game = $state<Game | null>(null);
  /** Mods ouverts depuis la liste, puis via les dépendances. */
  modStack = $state<number[]>([]);
  /** Vue où revenir en quittant la fiche d'un mod. */
  modOrigin = $state<"mods" | "library">("mods");
  /** Vue où revenir après la connexion. */
  afterLogin = $state<View | null>(null);
  settings = $state<SettingsView | null>(null);
  auth = $state<AuthView | null>(null);
  /** Change quand la clé ou les filtres changent : les listes se rechargent. */
  session = $state(0);

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

  /** Ouvre la fiche d'un mod depuis « Mes mods ». */
  openModFromLibrary(game: Game, modId: number) {
    if (this.game?.id !== game.id) this.game = game;
    this.modOrigin = "library";
    this.modStack = [modId];
    this.view = { name: "mod", modId };
  }

  back() {
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
