// Temporaire : tableau des modèles de Firefox (Remote Settings) retenus.
import { gzipSync } from "node:zlib";
const note = (t) => console.log(`::notice::${String(t).replace(/\r?\n/g, " ")}`);
const rs = (await (await fetch("https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models/records")).json()).data;
const cjk = rs.filter((d) => ["zh-Hans", "ja", "ko"].includes(d.fromLang) && d.toLang === "en");
note(`CJK: ${JSON.stringify(cjk.map((d) => [d.fromLang, d.version, d.fileType, d.filter_expression]))}`.slice(0, 3500));
const table = {};
for (const d of rs) {
  const pair = `${d.fromLang}${d.toLang}`;
  if (d.filter_expression || !(d.toLang === "en" || pair === "enfr")) continue;
  const cur = table[pair];
  const newer = !cur || d.version.localeCompare(cur.v, undefined, { numeric: true }) > 0;
  if (newer) table[pair] = { v: d.version, f: {} };
  if (table[pair].v !== d.version) continue;
  table[pair].f[d.fileType] = [d.attachment.hash, d.attachment.size, d.attachment.location.replace("main-workspace/translations-models/", "")];
}
const packed = gzipSync(JSON.stringify(table)).toString("base64");
note(`table ${Object.keys(table).length} paires, ${packed.length} car.`);
for (let i = 0, n = 0; i < packed.length; i += 3900, n++) note(`T${n}:${packed.slice(i, i + 3900)}`);
