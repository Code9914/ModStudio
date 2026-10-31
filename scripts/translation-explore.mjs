// Temporaire : où sont publiés les modèles de traduction de Firefox ?
const note = (t) => {
  const s = String(t);
  for (let i = 0; i < s.length; i += 3500) console.log(`::notice::${s.slice(i, i + 3500).replace(/\r?\n/g, " ")}`);
};
const s3 = await (await fetch("https://bergamot.s3.amazonaws.com/models/index.json")).json();
note(`S3 pairs: ${Object.keys(s3).join(" ")}`);
note(`S3 enfr: ${JSON.stringify(s3.enfr)}`);
note(`S3 esen: ${JSON.stringify(s3.esen)}`);
const rs = (await (await fetch("https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models/records")).json()).data;
const by = {};
for (const d of rs) {
  const k = `${d.fromLang}${d.toLang}`;
  by[k] ??= {};
  by[k][d.version] ??= [];
  by[k][d.version].push(d.fileType);
}
note(`RS: ${Object.entries(by).map(([k, v]) => `${k}=${Object.entries(v).map(([ver, f]) => `${ver}(${f.join("+")})`).join(",")}`).join(" ; ")}`);
const enfr = rs.filter((d) => d.fromLang === "en" && d.toLang === "fr");
note(`RS enfr: ${JSON.stringify(enfr.map((d) => ({ n: d.name, v: d.version, ft: d.fileType, fe: d.filter_expression, a: d.attachment })))}`);
