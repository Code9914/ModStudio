// Temporaire : modèles de Firefox (Remote Settings) compatibles avec le moteur
// Bergamot 0.4.9 ? Publie le tableau des fichiers (compressé) en annotations.
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, existsSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";
import { gzipSync } from "node:zlib";
import { chromium } from "playwright";

const note = (t) => console.log(`::notice::${String(t).replace(/\r?\n/g, " ")}`);
const CDN = "https://firefox-settings-attachments.cdn.mozilla.net/";
const rs = (await (await fetch("https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models/records")).json()).data;
const wanted = ["enfr", "fren", "deen", "esen", "iten", "pten", "ruen", "plen", "uken", "csen", "nlen", "sven", "tren", "zh-Hansen", "jaen", "koen", "huen", "roen", "daen", "fien", "elen", "bgen", "viien", "iden"];
const table = {};
for (const d of rs) {
  const pair = `${d.fromLang}${d.toLang}`;
  if (!wanted.includes(pair) || d.filter_expression) continue;
  const major = d.version.split(".")[0];
  table[pair] ??= {};
  const slot = (table[pair][major] ??= { version: d.version, files: {} });
  // Garde la version la plus récente de chaque génération (1.x, 2.x).
  if (d.version.localeCompare(slot.version, undefined, { numeric: true }) > 0) Object.assign(slot, { version: d.version, files: {} });
  if (d.version !== slot.version) continue;
  slot.files[d.fileType] = { h: d.attachment.hash, s: d.attachment.size, l: d.attachment.location };
}
note(`paires trouvées : ${Object.entries(table).map(([p, v]) => `${p}:${Object.values(v).map((x) => x.version).join("/")}`).join(" ")}`);

const dir = join(tmpdir(), "rs-models");
async function download(pair, gen) {
  const slot = table[pair]?.[gen];
  if (!slot) return null;
  const out = {};
  for (const [kind, f] of Object.entries(slot.files)) {
    const path = join(dir, `${pair}-${gen}-${kind}`);
    if (!existsSync(path)) {
      mkdirSync(dir, { recursive: true });
      const buf = Buffer.from(await (await fetch(CDN + f.l)).arrayBuffer());
      const sum = createHash("sha256").update(buf).digest("hex");
      if (sum !== f.h || buf.length !== f.s) throw new Error(`${pair} ${gen} ${kind}: empreinte`);
      writeFileSync(path, buf);
    }
    out[kind] = `/m/${pair}-${gen}-${kind}`;
  }
  return out;
}
const tests = [["enfr", "1"], ["enfr", "2"], ["fren", "2"], ["deen", "2"], ["zh-Hansen", "2"], ["jaen", "2"]];
const files = {};
for (const [p, g] of tests) files[`${p}-${g}`] = await download(p, g);

const worker = join(process.cwd(), "node_modules/@browsermt/bergamot-translator/worker");
const server = createServer((req, res) => {
  const u = new URL(req.url, "http://x").pathname;
  const file = u.startsWith("/worker/") ? join(worker, normalize(u.slice(8))) : u.startsWith("/m/") ? join(dir, normalize(u.slice(3))) : null;
  if (u === "/") return res.end("<!doctype html><title>t</title>");
  if (!file || !existsSync(file)) return (res.writeHead(404), res.end());
  res.writeHead(200, { "Content-Type": { ".js": "text/javascript", ".wasm": "application/wasm" }[extname(file)] ?? "application/octet-stream" });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(`http://127.0.0.1:${server.address().port}/`);
const samples = { en: "<p>This mod adds <b>32 player</b> lobbies and new cosmetics.</p>", fr: "<p>Ce mod ajoute des salons de 32 joueurs.</p>", de: "<p>Diese Mod fügt neue Waffen hinzu.</p>", zh: "<p>这个模组增加了更多的玩家。</p>", ja: "<p>このMODは新しい武器を追加します。</p>" };
const results = await page.evaluate(async ({ files, samples }) => {
  const out = {};
  for (const [key, f] of Object.entries(files)) {
    if (!f) { out[key] = "absent"; continue; }
    const w = new Worker("/worker/translator-worker.js");
    let serial = 0; const pending = new Map();
    w.addEventListener("message", ({ data }) => { const p = pending.get(data.id); pending.delete(data.id); data.error ? p.reject(new Error(data.error.message)) : p.resolve(data.result); });
    const call = (name, ...args) => new Promise((resolve, reject) => { const id = ++serial; pending.set(id, { resolve, reject }); w.postMessage({ id, name, args }); });
    try {
      await call("initialize", { cacheSize: 0, useNativeIntGemm: false });
      const get = async (u) => (await fetch(u)).arrayBuffer();
      const from = key.startsWith("zh") ? "zh" : key.slice(0, 2), to = key.startsWith("zh") ? "en" : key.slice(2, 4);
      const vocabs = f.vocab ? [await get(f.vocab)] : [await get(f.srcvocab), await get(f.trgvocab)];
      await call("loadTranslationModel", { from, to }, { model: await get(f.model), shortlist: await get(f.lex), vocabs });
      const t = performance.now();
      const [r] = await call("translate", { models: [{ from, to }], texts: [{ text: samples[from], html: true }] });
      out[key] = `${r.target.text} (${Math.round(performance.now() - t)} ms)`;
    } catch (e) { out[key] = `ERREUR ${e.message}`; }
    w.terminate();
  }
  return out;
}, { files, samples });
await browser.close(); server.close();
for (const [k, v] of Object.entries(results)) note(`${k}: ${v}`.slice(0, 800));
const packed = gzipSync(JSON.stringify(table)).toString("base64");
note(`table ${packed.length} car.`);
for (let i = 0, n = 0; i < packed.length; i += 3800, n++) note(`T${n}:${packed.slice(i, i + 3800)}`);
