// Navigation de l'appli (pas de routeur : un seul écran avec des vues).
import type { Game, SettingsView } from "./api";

export type View =
  | { name: "games" }
  | { name: "mods" }
  | { name: "mod"; modId: number }
  | { name: "settings" };

class Nav {
  view = $state<View>({ name: "games" });
  /** Jeu ouvert (sa liste de mods reste montée pour garder le défilement). */
  game = $state<Game | null>(null);
  /** Mods ouverts depuis la liste, puis via les dépendances. */
  modStack = $state<number[]>([]);
  settings = $state<SettingsView | null>(null);
  /** Change quand la clé ou les filtres changent : les listes se rechargent. */
  session = $state(0);

  openGames() {
    this.view = { name: "games" };
  }

  openGame(game: Game) {
    if (this.game?.id !== game.id) this.game = game;
    this.modStack = [];
    this.view = { name: "mods" };
  }

  openMod(modId: number) {
    this.modStack = [...this.modStack, modId];
    this.view = { name: "mod", modId };
  }

  back() {
    if (this.view.name === "mod" && this.modStack.length > 1) {
      this.modStack = this.modStack.slice(0, -1);
      this.view = { name: "mod", modId: this.modStack[this.modStack.length - 1] };
    } else if (this.view.name === "mod") {
      this.modStack = [];
      this.view = { name: "mods" };
    } else {
      this.view = { name: "games" };
    }
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
}

export const nav = new Nav();
