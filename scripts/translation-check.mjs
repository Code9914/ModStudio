// Vérifie la traduction des descriptions sur une vraie machine : les modèles
// de src-tauri/translation-models.json se téléchargent avec la bonne empreinte,
// et le moteur Bergamot (WASM, dans un navigateur) les charge et traduit.
//
//   node scripts/translation-check.mjs enfr fren zhen
//
// Demande playwright (npm i --no-save playwright && npx playwright install chromium).
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, existsSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";
import { gunzipSync } from "node:zlib";
import { chromium } from "playwright";

const root = new URL("..", import.meta.url).pathname;
const table = JSON.parse(readFileSync(join(root, "src-tauri/translation-models.json"), "utf8"));
const pairs = process.argv.slice(2).length ? process.argv.slice(2) : ["enfr", "fren"];
const dir = join(tmpdir(), "modstudio-translation-check");

// ------------------------------------------------------------ Modèles

for (const pair of pairs) {
  const entry = table.models[pair];
  if (!entry) throw new Error(`pas de modèle ${pair}`);
  mkdirSync(join(dir, pair), { recursive: true });
  for (const part of ["model", "lex", "vocab"]) {
    const out = join(dir, pair, part);
    if (existsSync(out)) continue;
    const file = entry[part];
    const res = await fetch(table.source + file.path);
    if (!res.ok) throw new Error(`${pair}/${part} : HTTP ${res.status} (${table.source + file.path})`);
    const gz = Buffer.from(await res.arrayBuffer());
    const sum = createHash("sha256").update(gz).digest("hex");
    if (gz.length !== file.size || sum !== file.sha256) throw new Error(`${pair}/${part} : empreinte ${sum} (${gz.length} o)`);
    writeFileSync(out, gunzipSync(gz));
    console.log(`ok ${pair}/${part} (${entry.arch}, ${(gz.length / 1e6).toFixed(1)} Mo)`);
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
  zh: "<p>这个模组增加了更多的玩家和新的服装。</p>",
};

const browser = await chromium.launch();
const page = await browser.newPage();
page.on("console", (m) => console.log("[page]", m.text()));
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
    for (const pair of pairs) {
      const [from, to] = [pair.slice(0, 2), pair.slice(2, 4)];
      const t = performance.now();
      await call("loadTranslationModel", { from, to }, {
        model: await get(`/models/${pair}/model`),
        shortlist: await get(`/models/${pair}/lex`),
        vocabs: [await get(`/models/${pair}/vocab`)],
      });
      out[`load ${pair}`] = Math.round(performance.now() - t);
    }
    for (const pair of pairs) {
      const [from, to] = [pair.slice(0, 2), pair.slice(2, 4)];
      const t = performance.now();
      const [r] = await call("translate", { models: [{ from, to }], texts: [{ text: samples[from], html: true }] });
      out[pair] = r.target.text;
      out[`time ${pair}`] = Math.round(performance.now() - t);
    }
    // Par l'anglais : chinois → anglais → français.
    if (pairs.includes("zhen") && pairs.includes("enfr")) {
      const [r] = await call("translate", {
        models: [{ from: "zh", to: "en" }, { from: "en", to: "fr" }],
        texts: [{ text: samples.zh, html: true }],
      });
      out["zh→en→fr"] = r.target.text;
    }
    return out;
  },
  { pairs, samples },
);
console.log(JSON.stringify(results, null, 1));
await browser.close();
server.close();

for (const pair of pairs) {
  const text = results[pair] ?? "";
  if (!text.trim() || text === samples[pair.slice(0, 2)]) throw new Error(`${pair} : pas de traduction`);
}
console.log("Traduction locale : ok");
