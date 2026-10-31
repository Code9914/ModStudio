// Temporaire : où sont publiés les modèles de traduction de Firefox ?
const note = (t) => console.log(`::notice::${String(t).replace(/\r?\n/g, "%0A").slice(0, 60000)}`);
async function tryJson(url) {
  try {
    const r = await fetch(url);
    const t = await r.text();
    return { status: r.status, text: t };
  } catch (e) {
    return { status: 0, text: String(e) };
  }
}
// Remote Settings (Firefox)
const rs = await tryJson("https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models/records");
note(`RS models status ${rs.status} len ${rs.text.length}`);
try {
  const data = JSON.parse(rs.text).data;
  const pick = data.filter((d) => ["en-fr", "fr-en", "zh-en", "es-en", "de-en"].includes(`${d.fromLang}-${d.toLang}`));
  note(JSON.stringify(pick.map((d) => ({ n: d.name, f: d.fromLang, t: d.toLang, v: d.version, ft: d.fileType, a: d.attachment && { h: d.attachment.hash, s: d.attachment.size, l: d.attachment.location } })), null, 0));
  note(`keys: ${Object.keys(data[0]).join(",")} ; total ${data.length} ; pairs ${[...new Set(data.map((d) => d.fromLang + d.toLang))].join(" ")}`);
} catch (e) {
  note(`RS parse: ${e}`);
}
const wasm = await tryJson("https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-wasm/records");
note(`RS wasm status ${wasm.status}: ${wasm.text.slice(0, 3000)}`);
const srv = await tryJson("https://firefox.settings.services.mozilla.com/v1/");
note(`RS server: ${srv.text.slice(0, 1500)}`);
// Registre GCS
const gcs = await tryJson("https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json");
note(`GCS status ${gcs.status} len ${gcs.text.length} head ${gcs.text.slice(0, 1500)}`);
// browsermt
const bm = await tryJson("https://bergamot.s3.amazonaws.com/models/index.json");
note(`S3 status ${bm.status} head ${bm.text.slice(0, 2500)}`);
