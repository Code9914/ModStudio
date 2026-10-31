// Temporaire : modèles chinois, japonais et coréens (version ordinateur) avec Bergamot 0.4.9 ?
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, existsSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";
import { gzipSync } from "node:zlib";
import { chromium } from "playwright";
const note = (t) => console.log(`::notice::${String(t).replace(/\r?\n/g, " ")}`);
const CDN = "https://firefox-settings-attachments.cdn.mozilla.net/main-workspace/translations-models/";
const rs = (await (await fetch("https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models/records")).json()).data;
const table = {};
for (const d of rs) {
  if (!["zh-Hans", "ja", "ko"].includes(d.fromLang) || d.toLang !== "en") continue;
  if (d.filter_expression !== "env.appinfo.OS != 'Android' || env.channel != 'release'") continue;
  const pair = `${d.fromLang}${d.toLang}`;
  const cur = table[pair];
  if (!cur || d.version.localeCompare(cur.v, undefined, { numeric: true }) > 0) table[pair] = { v: d.version, f: {} };
  if (table[pair].v !== d.version) continue;
  table[pair].f[d.fileType] = [d.attachment.hash, d.attachment.size, d.attachment.location.replace("main-workspace/translations-models/", "")];
}
const dir = join(tmpdir(), "cjk");
mkdirSync(dir, { recursive: true });
for (const [pair, e] of Object.entries(table))
  for (const [kind, [h, s, l]] of Object.entries(e.f)) {
    const buf = Buffer.from(await (await fetch(CDN + l)).arrayBuffer());
    if (createHash("sha256").update(buf).digest("hex") !== h || buf.length !== s) throw new Error(`${pair} ${kind} empreinte`);
    writeFileSync(join(dir, `${pair}-${kind}`), buf);
  }
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
const kinds = Object.fromEntries(Object.entries(table).map(([p, e]) => [p, Object.keys(e.f)]));
const results = await page.evaluate(async ({ kinds }) => {
  const samples = { "zh-Hans": "<p>这个模组增加了更多的玩家和新的服装。</p>", ja: "<p>このMODは新しい武器を追加します。</p>", ko: "<p>이 모드는 새로운 무기를 추가합니다.</p>" };
  const out = {};
  for (const [pair, ks] of Object.entries(kinds)) {
    const from = pair.slice(0, -2);
    const w = new Worker("/worker/translator-worker.js");
    let serial = 0; const pending = new Map();
    w.addEventListener("message", ({ data }) => { const p = pending.get(data.id); pending.delete(data.id); data.error ? p.reject(new Error(data.error.message)) : p.resolve(data.result); });
    const call = (name, ...args) => new Promise((resolve, reject) => { const id = ++serial; pending.set(id, { resolve, reject }); w.postMessage({ id, name, args }); });
    try {
      await call("initialize", { cacheSize: 0, useNativeIntGemm: false });
      const get = async (k) => (await fetch(`/m/${pair}-${k}`)).arrayBuffer();
      const vocabs = ks.includes("vocab") ? [await get("vocab")] : [await get("srcvocab"), await get("trgvocab")];
      await call("loadTranslationModel", { from, to: "en" }, { model: await get("model"), shortlist: await get("lex"), vocabs });
      const [r] = await call("translate", { models: [{ from, to: "en" }], texts: [{ text: samples[from], html: true }] });
      out[pair] = r.target.text;
    } catch (e) { out[pair] = `ERREUR ${e.message}`; }
    w.terminate();
  }
  return out;
}, { kinds });
await browser.close(); server.close();
note(`résultats : ${JSON.stringify(results)}`);
const packed = gzipSync(JSON.stringify(table)).toString("base64");
for (let i = 0, n = 0; i < packed.length; i += 3900, n++) note(`T${n}:${packed.slice(i, i + 3900)}`);
