// Descriptions Nexus Mods : du BBCode (avec quelques <br />) converti en HTML
// simple. Le résultat doit encore passer par DOMPurify avant affichage.
import { tr } from "./i18n.svelte";

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const NAMED: Record<string, string> = {
  amp: "&",
  lt: "<",
  gt: ">",
  quot: '"',
  apos: "'",
  nbsp: "\u00a0",
  bull: "•",
  middot: "·",
  hellip: "…",
  mdash: "—",
  ndash: "–",
  lsquo: "‘",
  rsquo: "’",
  ldquo: "“",
  rdquo: "”",
  laquo: "«",
  raquo: "»",
  copy: "©",
  reg: "®",
  trade: "™",
  deg: "°",
  times: "×",
  euro: "€",
  hearts: "♥",
  rarr: "→",
  larr: "←",
};

/** Autres entités nommées : le navigateur les connaît toutes (une zone de texte n'exécute rien). */
function namedEntity(name: string): string | null {
  const known = NAMED[name.toLowerCase()];
  if (known) return known;
  if (typeof document === "undefined") return null;
  const area = document.createElement("textarea");
  area.innerHTML = `&${name};`;
  return area.value !== `&${name};` ? area.value : null;
}

/**
 * Entités HTML déjà présentes dans le texte (`&#92;` pour `\`, `&quot;`…) :
 * remises en caractères avant l'échappement, sinon elles s'affichent telles
 * quelles. Deux passes pour les entités échappées deux fois (`&amp;#92;`).
 */
function decodeEntities(s: string): string {
  for (let pass = 0; pass < 2; pass++) {
    s = s.replace(/&(?:#(\d{1,7})|#x([0-9a-f]{1,6})|([a-z][a-z0-9]{1,30}));/gi, (m, dec, hex, name) => {
      if (name) return namedEntity(name) ?? m;
      const code = dec ? Number(dec) : parseInt(hex, 16);
      return code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : m;
    });
  }
  return s;
}

/** Texte simple de Nexus (résumé d'un mod) : entités et `<br />` remis en clair. */
export function plainText(input: string): string {
  return decodeEntities(input.replace(/\s*<br\s*\/?>\s*/gi, " ")).trim();
}

/**
 * Titres : beaucoup d'auteurs écrivent leurs intertitres en grand
 * (`[size=5][b]Installation[/b][/size]`). Un texte court en taille 5 ou plus,
 * seul sur sa ligne, devient un titre ; en taille 4, un sous-titre. Repères
 * `\uE010` + niveau … `\uE011`, changés en <h3> / <h4> à la fin.
 */
function sizeHeadings(s: string): string {
  return s.replace(/\[size=["']?([1-7])["']?\]([^\n]*?)\[\/size\]/gi, (m, n: string, body: string, at: number, all: string) => {
    const level = Number(n) >= 5 ? 3 : Number(n) === 4 ? 4 : 0;
    if (!level) return m;
    const start = all.lastIndexOf("\n", at - 1) + 1;
    const end = all.indexOf("\n", at + m.length);
    const before = all.slice(start, at);
    const after = all.slice(at + m.length, end === -1 ? all.length : end);
    const onlyTags = (t: string) => /^(?:\s|\[[^\]\n]*\])*$/.test(t) && !/\[\*\]/.test(t);
    const text = body.replace(/\[[^\]\n]*\]/g, "").trim();
    if (!text || /\[img/i.test(body) || text.length > (level === 3 ? 120 : 80) || !onlyTags(before) || !onlyTags(after)) {
      return m;
    }
    return `\uE010${level}${body}\uE011`;
  });
}

/** Début de ligne ou d'élément (rien à rattacher avant). */
const LINE_START = /(?:^|\[\*\]|\[list(?:=[^\]]*)?\]|\[quote(?:=[^\]]*)?\])[ \t\n]*$/i;

/**
 * Les [code] sont mis de côté avant les autres balises (leur contenu reste
 * tel quel), remplacés par des repères `\uE000n\uE001`.
 *
 * Sur plusieurs lignes, c'est un bloc. Sur une ligne, il reste dans le texte :
 * beaucoup de descriptions (converties depuis Markdown) entourent un simple
 * `.exe` de retours à la ligne. Ceux d'avant sont remplacés par une espace,
 * sauf après une fin de phrase ; ceux d'après sont retirés si la suite est
 * une ponctuation.
 */
function setCodeAside(s: string, codes: string[]): string {
  let out = "";
  let rest = s;
  const re = /\[code\]([\s\S]*?)\[\/code\]/i;
  for (let m = re.exec(rest); m; m = re.exec(rest)) {
    let before = out + rest.slice(0, m.index);
    let after = rest.slice(m.index + m[0].length);
    const body = m[1].replace(/^\n+|\n+$/g, "");
    if (body.includes("\n")) {
      codes.push(`<pre><code>${body}</code></pre>`);
    } else {
      codes.push(`<code>${body.trim()}</code>`);
      if (/\n[ \t\n]*$/.test(before) && !LINE_START.test(before) && !/[.!?][ \t\n]*$/.test(before)) {
        before = before.replace(/[ \t\n]+$/, " ");
      }
      after = after.replace(/^[ \t]*\n[ \t\n]*(?=[.,;:!?)])/, "");
      // « [code]SellX3.dll[/code]into » : l'espace perdue à la conversion.
      after = after.replace(/^(?=[\p{L}\p{N}])/u, " ");
      if (/[\p{L}\p{N}]$/u.test(before)) before += " ";
    }
    out = before + `\uE000${codes.length - 1}\uE001`;
    rest = after;
  }
  return out + rest;
}

/** Seuls les liens web sont gardés ; le reste devient du texte. */
function safeUrl(raw: string): string | null {
  const url = raw.trim().replace(/^["']|["']$/g, "").replace(/&amp;/g, "&");
  return /^https?:\/\/[^\s<>"]+$/i.test(url) ? escapeHtml(url) : null;
}

function link(url: string, text: string) {
  const href = safeUrl(url);
  return href ? `<a href="${href}">${text}</a>` : text;
}

/** `[youtube]` : un identifiant (avec `?start=` / `?t=`) ou une adresse YouTube. */
function youtube(value: string): string | null {
  const v = value.replace(/&amp;/g, "&");
  const m =
    /^([\w-]{6,20})(?:[?&](?:start|t)=(\d+))?$/.exec(v) ??
    /^https?:\/\/(?:www\.|m\.)?(?:youtube\.com\/watch\?(?:.*&)?v=|youtu\.be\/)([\w-]{6,20})(?:.*[?&](?:start|t)=(\d+))?/.exec(v);
  if (!m) return null;
  return link(`https://www.youtube.com/watch?v=${m[1]}${m[2] ? `&t=${m[2]}s` : ""}`, tr("Vidéo sur YouTube", "Video on YouTube"));
}

function image(url: string) {
  const src = safeUrl(url);
  return src ? `<img src="${src}" alt="" loading="lazy">` : "";
}

/**
 * Tableau : `[tr]` lignes, `[th]` / `[td]` cellules. Les retours à la ligne
 * entre les balises (un par cellule dans beaucoup de descriptions) sont retirés.
 */
function table(body: string) {
  const html = body
    .replace(/\[(\/?)(tr|th|td)\]/gi, (_, close, tag) => `<${close}${tag.toLowerCase()}>`)
    .replace(/(<\/?(?:tr|th|td)>)\s+(?=<\/?(?:tr|th|td)>)/g, "$1")
    .replace(/<(th|td)>\s+|\s+<\/(th|td)>/g, (m) => m.trim())
    .trim();
  return `<table>${html}</table>`;
}

/** Liste : `[*]` ouvre un élément, `[/*]` (facultatif) le ferme. `[olist]` : numérotée. */
function list(name: string, kind: string | undefined, body: string) {
  const tag = name.toLowerCase() === "olist" || (kind && /^[1aAiI]$/.test(kind)) ? "ol" : "ul";
  const items = body
    .split(/\[\*\]/)
    .map((i) => i.replace(/\[\/\*\]/g, "").trim())
    .filter(Boolean)
    .map((i) => `<li>${i}</li>`)
    .join("");
  return `<${tag}>${items}</${tag}>`;
}

const RULES: [RegExp, string | ((...m: string[]) => string)][] = [
  [/\[b\]([\s\S]*?)\[\/b\]/gi, "<strong>$1</strong>"],
  [/\[i\]([\s\S]*?)\[\/i\]/gi, "<em>$1</em>"],
  [/\[u\]([\s\S]*?)\[\/u\]/gi, "<u>$1</u>"],
  [/\[s\]([\s\S]*?)\[\/s\]/gi, "<s>$1</s>"],
  [/\[url=([^\]]+)\]([\s\S]*?)\[\/url\]/gi, (_, url, text) => link(url, text)],
  [/\[url\]([\s\S]*?)\[\/url\]/gi, (_, url) => link(url, url)],
  [/\[img(?:[^\]]*)\]([\s\S]*?)\[\/img\]/gi, (_, url) => image(url)],
  [/\[youtube\]([^\[\]\s]{6,200})\[\/youtube\]/gi, (m, v) => youtube(v) ?? m],
  // Listes imbriquées : la plus intérieure d'abord (corps sans autre [list]).
  [/\[(o?list)(?:=([^\]]*))?\]((?:(?!\[o?list[\]=])[\s\S])*?)\[\/\1\]/gi, (_, name, kind, body) => list(name, kind, body)],
  // Citations et spoilers imbriqués : le plus intérieur d'abord, comme les listes.
  [/\[quote(?:=[^\]]*)?\]((?:(?!\[quote[\]=])[\s\S])*?)\[\/quote\]/gi, "<blockquote>$1</blockquote>"],
  [/\[table\]((?:(?!\[table\])[\s\S])*?)\[\/table\]/gi, (_, body) => table(body)],
  [
    /\[spoiler(?:=([^\]]*))?\]((?:(?!\[spoiler[\]=])[\s\S])*?)\[\/spoiler\]/gi,
    (_, title: string | undefined, body: string) =>
      `<details><summary>${title?.replace(/^(?:&quot;|')|(?:&quot;|')$/g, "").trim() || "Spoiler"}</summary>${body}</details>`,
  ],
  [/\[heading\]([\s\S]*?)\[\/heading\]/gi, "<h3>$1</h3>"],
  [/\[(?:line|hr)\]/gi, "<hr>"],
];

/** Balises de mise en forme abandonnées (taille, couleur, police, alignement). */
const DROPPED = /\[\/?(?:size|color|colour|font|center|centre|left|right|justify|indent|align|style|h[1-6])(?:=[^\]]*)?\]/gi;

const BLOCK = "(?:ul|ol|li|blockquote|pre|details|summary|h3|h4|hr|table|tr|th|td)";

export function bbcodeToHtml(input: string): string {
  let s = input.replace(/\r\n?/g, "\n").replace(/\n?<br\s*\/?>\n?/gi, "\n");
  s = escapeHtml(decodeEntities(s)).replace(/[\uE000-\uE01F]/g, "");
  s = sizeHeadings(s);
  // Mise en forme retirée d'abord : un [/color] entre un retour à la ligne et
  // un [code] ne doit pas cacher où se trouve le code.
  s = s.replace(DROPPED, "");
  const codes: string[] = [];
  s = setCodeAside(s, codes);
  // Les balises imbriquées se résolvent de l'intérieur vers l'extérieur.
  for (let pass = 0; pass < 8; pass++) {
    const before = s;
    for (const [re, rep] of RULES) s = s.replace(re, rep as never);
    if (s === before) break;
  }
  // Balises restantes sans leur paire (listes mal fermées, [b] oublié…) : retirées.
  s = s.replace(/\[\/?(?:o?list|b|i|u|s|url|img|youtube|quote|spoiler|heading|table|tr|td|th)(?:=[^\]]*)?\]/gi, "");
  // Titres : sans les mises en forme qui les entouraient (« [b][size=5]… »).
  s = s.replace(
    /((?:<(?:strong|em|u|s)>\s*)*)\uE010([34])([\s\S]*?)\uE011((?:\s*<\/(?:strong|em|u|s)>)*)/g,
    (_, open: string, level: string, body: string, close: string) => {
      // Un titre est déjà en gras : le [b] qui l'entoure en entier est retiré.
      const text = body.trim().replace(/^<strong>((?:(?!<\/?strong>)[\s\S])*)<\/strong>$/, "$1");
      const heading = `<h${level}>${text}</h${level}>`;
      const count = (t: string) => (t.match(/</g) ?? []).length;
      return count(open) === count(close) ? heading : open + heading + close;
    },
  );
  // Mise en forme vide (« [b]<br />[/b] ») : retirée.
  s = s.replace(/<(strong|em|u|s)>(\s*)<\/\1>/g, "$2");
  // Éléments de liste hors d'un [list] : une puce, sans la balise de fin.
  s = s.replace(/\[\/\*\]/g, "").replace(/\[\*\]/g, "• ");
  s = s.replace(/\uE000(\d+)\uE001/g, (_, i) => codes[Number(i)]);
  s = s.replace(/\n/g, "<br>");
  // Pas de saut de ligne collé aux blocs.
  s = s.replace(new RegExp(`(?:<br>\\s*)+(<\\/?${BLOCK}\\b)`, "g"), "$1");
  s = s.replace(new RegExp(`(<\\/?${BLOCK}\\b[^>]*>)(?:\\s*<br>)+`, "g"), "$1");
  s = s.replace(/(<br>\s*){3,}/g, "<br><br>");
  return s.trim();
}
