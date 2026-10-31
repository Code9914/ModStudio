// Vérifie la traduction des descriptions sur une vraie machine : les modèles
// de src-tauri/translation-models.json se téléchargent avec la bonne empreinte,
// et le moteur Bergamot (WASM, dans un navigateur) les charge et traduit.
//
//   node scripts/translation-check.mjs en-fr fr-en zh-Hans-en
//
// Demande playwright (npm i --no-save playwright && npx playwright install chromium).
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, existsSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright";

// Sur GitHub, les résultats et l'erreur éventuelle deviennent des annotations
// (lisibles sans télécharger le journal).
const ci = !!process.env.GITHUB_ACTIONS;
const annotate = (kind, text) => ci && console.log(`::${kind}::${String(text).replace(/\r?\n/g, "%0A")}`);
process.on("uncaughtException", (e) => {
  annotate("error", e?.stack ?? e);
  console.error(e);
  process.exit(1);
});
process.on("unhandledRejection", (e) => {
  annotate("error", e?.stack ?? e);
  console.error(e);
  process.exit(1);
});

const root = new URL("..", import.meta.url).pathname;
const table = JSON.parse(readFileSync(join(root, "src-tauri/translation-models.json"), "utf8"));
const pairs = process.argv.slice(2).length ? process.argv.slice(2) : ["en-fr", "fr-en"];
const entryOf = (pair) => table.models.find((m) => `${m.from}-${m.to}` === pair);
const dir = join(tmpdir(), "modstudio-translation-check");

// ------------------------------------------------------------ Modèles

for (const pair of pairs) {
  const entry = entryOf(pair);
  if (!entry) throw new Error(`pas de modèle ${pair}`);
  mkdirSync(join(dir, pair), { recursive: true });
  for (const [part, file] of Object.entries(entry.files)) {
    const out = join(dir, pair, part);
    if (existsSync(out)) continue;
    const res = await fetch(table.source + file.file);
    if (!res.ok) throw new Error(`${pair}/${part} : HTTP ${res.status} (${table.source + file.file})`);
    const buf = Buffer.from(await res.arrayBuffer());
    const sum = createHash("sha256").update(buf).digest("hex");
    if (buf.length !== file.size || sum !== file.sha256) throw new Error(`${pair}/${part} : empreinte ${sum} (${buf.length} o)`);
    writeFileSync(out, buf);
    console.log(`ok ${pair}/${part} (${entry.version}, ${(buf.length / 1e6).toFixed(1)} Mo)`);
  }
}

// ------------------------------------------------------------ Serveur local

const worker = join(root, "node_modules/@browsermt/bergamot-translator/worker");
const types = { ".js": "text/javascript", ".wasm": "application/wasm", ".html": "text/html" };
const server = createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  let file = null;
  if (url.pathname === "/") file = null;
  else if (url.pathname.startsWith("/worker/")) file = join(worker, normalize(url.pathname.slice(8)));
  else if (url.pathname.startsWith("/models/")) file = join(dir, normalize(url.pathname.slice(8)));
  if (url.pathname === "/") {
    res.writeHead(200, { "Content-Type": "text/html" });
    return res.end("<!doctype html><title>check</title>");
  }
  if (!file || !existsSync(file)) {
    res.writeHead(404);
    return res.end();
  }
  res.writeHead(200, { "Content-Type": types[extname(file)] ?? "application/octet-stream" });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

// ------------------------------------------------------------ Traduction

const samples = {
  en: '<p>This mod adds <b>32 player</b> lobbies and new cosmetics.</p><ul><li>Works with <a href="https://example.org">LateCompany</a></li><li>Everyone needs to have the mod installed.</li></ul>',
  fr: "<p>Ce mod ajoute des <b>salons de 32 joueurs</b> et de nouveaux cosmétiques.</p>",
  "zh-Hans": "<p>这个模组增加了更多的玩家和新的服装。</p>",
};

const browser = await chromium.launch();
const page = await browser.newPage();
page.on("console", (m) => {
  console.log("[page]", m.text());
  if (m.type() === "error") annotate("warning", `[page] ${m.text()}`);
});
await page.goto(base);
const results = await page.evaluate(
  async ({ pairs, samples }) => {
    const w = new Worker("/worker/translator-worker.js");
    let serial = 0;
    const pending = new Map();
    w.addEventListener("message", ({ data }) => {
      const p = pending.get(data.id);
      pending.delete(data.id);
      if (data.error) p.reject(new Error(data.error.message));
      else p.resolve(data.result);
    });
    const call = (name, ...args) =>
      new Promise((resolve, reject) => {
        const id = ++serial;
        pending.set(id, { resolve, reject });
        w.postMessage({ id, name, args });
      });
    const t0 = performance.now();
    await call("initialize", { cacheSize: 0, useNativeIntGemm: false });
    const out = { init: Math.round(performance.now() - t0) };
    const get = async (url) => (await fetch(url)).arrayBuffer();
    const split = (pair) => [pair.slice(0, pair.lastIndexOf("-")), pair.slice(pair.lastIndexOf("-") + 1)];
    for (const { pair, parts } of pairs) {
      const [from, to] = split(pair);
      const t = performance.now();
      const vocabs = parts.includes("vocab") ? [await get(`/models/${pair}/vocab`)] : [await get(`/models/${pair}/srcvocab`), await get(`/models/${pair}/trgvocab`)];
      await call("loadTranslationModel", { from, to }, {
        model: await get(`/models/${pair}/model`),
        shortlist: await get(`/models/${pair}/lex`),
        vocabs,
      });
      out[`load ${pair}`] = Math.round(performance.now() - t);
    }
    for (const { pair } of pairs) {
      const [from, to] = split(pair);
      const t = performance.now();
      const [r] = await call("translate", { models: [{ from, to }], texts: [{ text: samples[from], html: true }] });
      out[pair] = r.target.text;
      out[`time ${pair}`] = Math.round(performance.now() - t);
    }
    // Par l'anglais : chinois → anglais → français.
    if (pairs.some((p) => p.pair === "zh-Hans-en") && pairs.some((p) => p.pair === "en-fr")) {
      const [r] = await call("translate", {
        models: [{ from: "zh-Hans", to: "en" }, { from: "en", to: "fr" }],
        texts: [{ text: samples["zh-Hans"], html: true }],
      });
      out["zh→en→fr"] = r.target.text;
    }
    return out;
  },
  { pairs: pairs.map((pair) => ({ pair, parts: Object.keys(entryOf(pair).files) })), samples },
);
console.log(JSON.stringify(results, null, 1));
annotate("notice", JSON.stringify(results, null, 1));
await browser.close();
server.close();

for (const pair of pairs) {
  const text = results[pair] ?? "";
  if (!text.trim() || text === samples[pair.slice(0, pair.lastIndexOf("-"))]) throw new Error(`${pair} : pas de traduction`);
}
console.log("Traduction locale : ok");
