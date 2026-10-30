// Mises à jour de ModStudio : nouvelle version publiée dans les releases
// GitHub (fichier latest.json signé), téléchargée puis installée à la demande.
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { bytes } from "./format";
import type { Progress } from "./installs.svelte";

class AppUpdates {
  /** Version installée. */
  current = $state<string | null>(null);
  /** Nouvelle version disponible. */
  available = $state<Update | null>(null);
  checking = $state(false);
  /** Dernière recherche faite (même sans résultat), pour l'afficher dans les réglages. */
  checked = $state(false);
  progress = $state<Progress | null>(null);
  error = $state<string | null>(null);
  /** « Plus tard » : le bandeau ne revient qu'au prochain lancement. */
  dismissed = $state(false);

  /** Mode développement (« ModStudio (dev) ») : le code se met à jour par git. */
  get enabled() {
    return !import.meta.env.DEV;
  }

  async check(manual = false) {
    if (!this.enabled || this.checking) return;
    this.checking = true;
    this.error = null;
    try {
      this.current ??= await getVersion();
      this.available = await check();
      if (manual) this.dismissed = false;
    } catch (e) {
      // Hors ligne au lancement : rien à signaler, sauf si on a demandé.
      if (manual) this.error = String(e);
    } finally {
      this.checking = false;
      this.checked = true;
    }
  }

  async install() {
    const update = this.available;
    if (!update || this.progress) return;
    this.error = null;
    let received = 0;
    let total = 0;
    this.progress = { label: "Téléchargement de la mise à jour…", ratio: null };
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          this.progress = {
            label: total ? `Téléchargement : ${bytes(received)} sur ${bytes(total)}` : `Téléchargement : ${bytes(received)}`,
            ratio: total ? Math.min(1, received / total) : null,
          };
        } else if (event.event === "Finished") {
          this.progress = { label: "Installation…", ratio: null };
        }
      });
      // Sous Windows, l'installateur ferme ModStudio lui-même ; ailleurs on relance.
      await relaunch();
    } catch (e) {
      this.error = String(e);
      this.progress = null;
    }
  }
}

export const updates = new AppUpdates();
