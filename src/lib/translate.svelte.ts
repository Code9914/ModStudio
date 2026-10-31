// Traduction des descriptions sur l'ordinateur : moteur Bergamot (celui de
// Firefox, en WASM dans un worker) et modèles téléchargés une fois par le
// backend. Le texte des mods ne quitte jamais l'ordinateur.
import DOMPurify from "dompurify";
import { asAppError, translation, type AppError, type TranslationStep } from "./api";
import { english, intlLocale, tr } from "./i18n.svelte";

/** Langue vers laquelle traduire : celle de l'interface. */
export const target = english ? "en" : "fr";

/** Un morceau de description : texte brut, ou HTML déjà assaini. */
export interface Source {
  text: string;
  html?: boolean;
}

export type Progress =
  | { phase: "download"; received: number; total: number }
  | { phase: "translate"; done: number; total: number };

/** Même nettoyage que les descriptions affichées. */
export function cleanHtml(html: string): string {
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "script", "object", "embed"],
    FORBID_ATTR: ["style", "class", "id", "srcset"],
  });
}

const names = (() => {
  try {
    return new Intl.DisplayNames([intlLocale], { type: "language" });
  } catch {
    return null;
  }
})();

/** « anglais », « chinois simplifié »… */
export function languageName(code: string): string {
  try {
    return names?.of(code) ?? code;
  } catch {
    return code;
  }
}

// ---------------------------------------------------------------- Moteur

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void };

/** Le worker Bergamot : un seul pour l'appli, démarré à la première traduction. */
class Engine {
  #worker: Worker | null = null;
  #ready: Promise<unknown> | null = null;
  #serial = 0;
  #pending = new Map<number, Pending>();
  #loaded = new Map<string, Promise<void>>();

  #call<T>(name: string, args: unknown[], transfer: Transferable[] = []): Promise<T> {
    return new Promise((resolve, reject) => {
      const id = ++this.#serial;
      this.#pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.#worker!.postMessage({ id, name, args }, transfer);
    });
  }

  #start() {
    if (!this.#ready) {
      const worker = new Worker("/bergamot/translator-worker.js");
      worker.addEventListener("message", ({ data }) => {
        const p = this.#pending.get(data.id);
        if (!p) return;
        this.#pending.delete(data.id);
        if (data.error) p.reject(new Error(data.error.message ?? String(data.error)));
        else p.resolve(data.result);
      });
      worker.addEventListener("error", (e) => this.#fail(e.message || "worker"));
      this.#worker = worker;
      this.#ready = this.#call("initialize", [{ cacheSize: 0, useNativeIntGemm: false }]);
      this.#ready.catch((e) => this.#fail(String(e?.message ?? e)));
    }
    return this.#ready;
  }

  /** Le moteur s'est arrêté : tout est à refaire à la prochaine traduction. */
  #fail(message: string) {
    for (const p of this.#pending.values()) p.reject(new Error(message));
    this.#pending.clear();
    this.#worker?.terminate();
    this.#worker = null;
    this.#ready = null;
    this.#loaded.clear();
  }

  #load(step: TranslationStep): Promise<void> {
    const key = `${step.from}-${step.to}-${step.version}`;
    let loading = this.#loaded.get(key);
    if (!loading) {
      loading = (async () => {
        await this.#start();
        const get = (kind: string) => translation.file(step.from, step.to, kind);
        const vocabs = step.files.includes("vocab") ? ["vocab"] : ["srcvocab", "trgvocab"];
        const [model, shortlist, ...vocab] = await Promise.all([get("model"), get("lex"), ...vocabs.map(get)]);
        await this.#call(
          "loadTranslationModel",
          [{ from: step.from, to: step.to }, { model, shortlist, vocabs: vocab }],
          [model, shortlist, ...vocab],
        );
      })();
      loading.catch(() => this.#loaded.delete(key));
      this.#loaded.set(key, loading);
    }
    return loading;
  }

  async translate(steps: TranslationStep[], texts: Source[]): Promise<string[]> {
    for (const step of steps) await this.#load(step);
    const out = await this.#call<{ target: { text: string } }[]>("translate", [
      { models: steps.map((s) => ({ from: s.from, to: s.to })), texts: texts.map((t) => ({ text: t.text, html: !!t.html })) },
    ]);
    return out.map((r) => r.target.text);
  }
}

// ---------------------------------------------------------------- Découpage

/** Taille visée d'un appel au moteur (pour suivre l'avancement). */
const BATCH = 1800;
const BREAKS = new Set([
  "P", "DIV", "UL", "OL", "LI", "BR", "HR", "TABLE", "PRE", "BLOCKQUOTE", "SECTION", "DETAILS",
  "H1", "H2", "H3", "H4", "H5", "H6", "IMG", "FIGURE", "CENTER", "DL",
]);

function escapeHtml(s: string) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

type Piece = { keep: string } | { text: string; html: boolean; lead: string; trail: string };

interface Prepared {
  pieces: Piece[];
  html: boolean;
  /** Code mis de côté (pas traduit), remis après. */
  kept: string[];
}

/** Découpe un morceau en parties à traduire ; le code (pre, code) n'est pas traduit. */
function prepare(source: Source): Prepared {
  if (!source.html) {
    // Texte brut : ligne par ligne, en gardant les retours à la ligne.
    const pieces: Piece[] = source.text.split(/(\n+)/).map((part, i) => {
      if (i % 2 === 1 || !part.trim()) return { keep: part };
      const [, lead, text, trail] = part.match(/^(\s*)([\s\S]*?)(\s*)$/)!;
      return { text, html: false, lead, trail };
    });
    return { pieces, html: false, kept: [] };
  }
  const doc = new DOMParser().parseFromString(`<!doctype html><body>${source.text}`, "text/html");
  const kept: string[] = [];
  for (const el of doc.body.querySelectorAll("pre, code")) {
    if (!doc.body.contains(el)) continue; // déjà dans un bloc mis de côté
    el.setAttribute("data-ms-keep", String(kept.length));
    kept.push(el.innerHTML);
    el.innerHTML = "";
  }
  // Par blocs, coupés après un élément qui termine une ligne.
  const pieces: Piece[] = [];
  let current = "";
  const flush = () => {
    if (current.trim()) pieces.push({ text: current, html: true, lead: "", trail: "" });
    else if (current) pieces.push({ keep: current });
    current = "";
  };
  for (const node of [...doc.body.childNodes]) {
    if (node.nodeType === Node.ELEMENT_NODE) current += (node as Element).outerHTML;
    else if (node.nodeType === Node.TEXT_NODE) current += escapeHtml(node.textContent ?? "");
    if (current.length >= BATCH && BREAKS.has(node.nodeName)) flush();
  }
  flush();
  return { pieces, html: true, kept };
}

/** Remet le code mis de côté et assainit le résultat. */
function finish(prepared: Prepared, translated: string): string {
  if (!prepared.html) return translated;
  const doc = new DOMParser().parseFromString(`<!doctype html><body>${translated}`, "text/html");
  for (const el of doc.body.querySelectorAll("[data-ms-keep]")) {
    const i = Number(el.getAttribute("data-ms-keep"));
    el.removeAttribute("data-ms-keep");
    if (Number.isInteger(i) && i >= 0 && i < prepared.kept.length) el.innerHTML = prepared.kept[i];
  }
  return cleanHtml(doc.body.innerHTML);
}

/** Le texte lisible d'une description, pour reconnaître sa langue. */
export function plainText(sources: Source[]): string {
  const parts = sources.map((s) => {
    if (!s.html) return s.text;
    const doc = new DOMParser().parseFromString(`<!doctype html><body>${s.text}`, "text/html");
    doc.body.querySelectorAll("pre, code").forEach((el) => el.remove());
    return doc.body.textContent ?? "";
  });
  return parts
    .join("\n")
    .replace(/https?:\/\/\S+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

// ---------------------------------------------------------------- Traducteur

const AUTO_KEY = "modstudio.translate.auto";
const CACHE = 40;

function readAuto() {
  try {
    return localStorage.getItem(AUTO_KEY) === "1";
  } catch {
    return false;
  }
}

class Translator {
  /** Traduire les descriptions sans attendre le bouton. */
  auto = $state(readAuto());
  #engine = new Engine();
  #cache = new Map<string, string[]>();
  #downloads = new Map<string, Promise<unknown>>();
  #listeners = new Map<string, Set<(p: Progress) => void>>();

  setAuto(value: boolean) {
    this.auto = value;
    try {
      if (value) localStorage.setItem(AUTO_KEY, "1");
      else localStorage.removeItem(AUTO_KEY);
    } catch {
      /* stockage indisponible : réglage pour cette session */
    }
  }

  /** Langue de la description quand elle diffère de celle de l'interface et que ModStudio sait la traduire. */
  async detect(sources: Source[]): Promise<string | null> {
    const text = plainText(sources);
    if (text.length < 20) return null;
    try {
      const lang = await translation.detect(text.slice(0, 3000));
      return lang && lang !== target ? lang : null;
    } catch {
      return null;
    }
  }

  /** Télécharge les modèles manquants (un téléchargement par langue, partagé). */
  async #ensure(from: string, onProgress: (p: Progress) => void) {
    const plan = await translation.plan(from, target);
    if (plan.missing === 0) return plan.steps;
    let listeners = this.#listeners.get(from);
    if (!listeners) this.#listeners.set(from, (listeners = new Set()));
    listeners.add(onProgress);
    onProgress({ phase: "download", received: 0, total: plan.missing });
    let running = this.#downloads.get(from);
    if (!running) {
      running = translation
        .download(from, target, (e) => {
          if (e.phase !== "downloading") return;
          for (const l of this.#listeners.get(from) ?? []) l({ phase: "download", received: e.received, total: e.total });
        })
        .finally(() => {
          this.#downloads.delete(from);
          this.#listeners.delete(from);
        });
      this.#downloads.set(from, running);
    }
    try {
      await running;
    } finally {
      listeners.delete(onProgress);
    }
    return (await translation.plan(from, target)).steps;
  }

  /** Traduit chaque morceau de `from` vers la langue de l'interface. */
  async translate(sources: Source[], from: string, onProgress: (p: Progress) => void = () => {}): Promise<string[]> {
    const key = `${from}>${target}:${JSON.stringify(sources)}`;
    const cached = this.#cache.get(key);
    if (cached) return cached;
    try {
      const steps = await this.#ensure(from, onProgress);
      const prepared = sources.map(prepare);
      const todo = prepared.flatMap((p) => p.pieces.filter((x): x is Extract<Piece, { text: string }> => "text" in x));
      const total = todo.reduce((n, p) => n + p.text.length, 0);
      const results = new Map<Piece, string>();
      let done = 0;
      onProgress({ phase: "translate", done, total });
      for (let i = 0; i < todo.length; ) {
        // Plusieurs petites parties par appel, jusqu'à la taille visée.
        const batch = [todo[i++]];
        let size = batch[0].text.length;
        while (i < todo.length && size + todo[i].text.length <= BATCH) {
          size += todo[i].text.length;
          batch.push(todo[i++]);
        }
        const out = await this.#engine.translate(steps, batch);
        batch.forEach((p, j) => results.set(p, p.lead + (out[j] ?? p.text) + p.trail));
        done += size;
        onProgress({ phase: "translate", done, total });
      }
      const translated = prepared.map((p) =>
        finish(p, p.pieces.map((x) => ("keep" in x ? x.keep : results.get(x)!)).join("")),
      );
      this.#cache.set(key, translated);
      if (this.#cache.size > CACHE) this.#cache.delete(this.#cache.keys().next().value!);
      return translated;
    } catch (e) {
      throw failure(e);
    }
  }

  storage = () => translation.storage();

  async clear() {
    await translation.clear();
    this.#cache.clear();
  }
}

function failure(e: unknown): AppError {
  const err = asAppError(e);
  if (err.kind !== "unknown") return err;
  return {
    kind: "translation",
    message: tr(`La traduction a échoué (${err.message}).`, `Translation failed (${err.message}).`),
    retryAfter: null,
  };
}

export const translator = new Translator();
