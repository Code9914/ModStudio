// Descriptions Nexus Mods : du BBCode (avec quelques <br />) converti en HTML
// simple. Le résultat doit encore passer par DOMPurify avant affichage.

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const NAMED: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: "\u00a0" };

/**
 * Entités HTML déjà présentes dans le texte (`&#92;` pour `\`, `&quot;`…) :
 * remises en caractères avant l'échappement, sinon elles s'affichent telles
 * quelles. Deux passes pour les entités échappées deux fois (`&amp;#92;`).
 */
function decodeEntities(s: string): string {
  for (let pass = 0; pass < 2; pass++) {
    s = s.replace(/&(?:#(\d{1,7})|#x([0-9a-f]{1,6})|(amp|lt|gt|quot|apos|nbsp));/gi, (m, dec, hex, name) => {
      if (name) return NAMED[name.toLowerCase()];
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

/** Code sur une ligne : dans le texte ; sur plusieurs : un bloc. */
function code(body: string) {
  const text = body.replace(/^\n+|\n+$/g, "");
  return text.includes("\n") ? `<pre><code>${text}</code></pre>` : `<code>${text.trim()}</code>`;
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

function image(url: string) {
  const src = safeUrl(url);
  return src ? `<img src="${src}" alt="" loading="lazy">` : "";
}

/** Liste : `[*]` ouvre un élément, `[/*]` (facultatif) le ferme. */
function list(kind: string | undefined, body: string) {
  const tag = kind && /^[1aAiI]$/.test(kind) ? "ol" : "ul";
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
  [/\[youtube\]([\w-]{6,20})\[\/youtube\]/gi, (_, id) => link(`https://www.youtube.com/watch?v=${id}`, "Vidéo sur YouTube")],
  // Listes imbriquées : la plus intérieure d'abord (corps sans autre [list]).
  [/\[list(?:=([^\]]*))?\]((?:(?!\[list[\]=])[\s\S])*?)\[\/list\]/gi, (_, kind, body) => list(kind, body)],
  [/\[quote(?:=[^\]]*)?\]([\s\S]*?)\[\/quote\]/gi, "<blockquote>$1</blockquote>"],
  [/\[code\]([\s\S]*?)\[\/code\]/gi, (_, body) => code(body)],
  [/\[spoiler(?:=[^\]]*)?\]([\s\S]*?)\[\/spoiler\]/gi, "<details><summary>Spoiler</summary>$1</details>"],
  [/\[heading\]([\s\S]*?)\[\/heading\]/gi, "<h3>$1</h3>"],
  [/\[line\]/gi, "<hr>"],
];

/** Balises de mise en forme abandonnées (taille, couleur, police, alignement), et listes mal fermées. */
const DROPPED = /\[\/?(?:size|color|colour|font|center|centre|left|right|justify|indent|align|style|h[1-6]|list)(?:=[^\]]*)?\]/gi;

const BLOCK = "(?:ul|ol|li|blockquote|pre|details|summary|h3|hr)";

export function bbcodeToHtml(input: string): string {
  let s = input.replace(/\r\n?/g, "\n").replace(/\n?<br\s*\/?>\n?/gi, "\n");
  s = escapeHtml(decodeEntities(s));
  // Les balises imbriquées se résolvent de l'intérieur vers l'extérieur.
  for (let pass = 0; pass < 8; pass++) {
    const before = s;
    for (const [re, rep] of RULES) s = s.replace(re, rep as never);
    if (s === before) break;
  }
  s = s.replace(DROPPED, "");
  // Éléments de liste hors d'un [list] : une puce, sans la balise de fin.
  s = s.replace(/\[\/\*\]/g, "").replace(/\[\*\]/g, "• ");
  s = s.replace(/\n/g, "<br>");
  // Pas de saut de ligne collé aux blocs.
  s = s.replace(new RegExp(`(?:<br>\\s*)+(<\\/?${BLOCK}\\b)`, "g"), "$1");
  s = s.replace(new RegExp(`(<\\/?${BLOCK}\\b[^>]*>)(?:\\s*<br>)+`, "g"), "$1");
  s = s.replace(/(<br>\s*){3,}/g, "<br><br>");
  return s.trim();
}
