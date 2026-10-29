// Descriptions Nexus Mods : du BBCode (avec quelques <br />) converti en HTML
// simple. Le résultat doit encore passer par DOMPurify avant affichage.

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

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

function list(kind: string | undefined, body: string) {
  const tag = kind && /^[1aAiI]$/.test(kind) ? "ol" : "ul";
  const items = body
    .split(/\[\*\]/)
    .map((i) => i.trim())
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
  [/\[list(?:=([^\]]*))?\]([\s\S]*?)\[\/list\]/gi, (_, kind, body) => list(kind, body)],
  [/\[quote(?:=[^\]]*)?\]([\s\S]*?)\[\/quote\]/gi, "<blockquote>$1</blockquote>"],
  [/\[code\]([\s\S]*?)\[\/code\]/gi, "<pre><code>$1</code></pre>"],
  [/\[spoiler(?:=[^\]]*)?\]([\s\S]*?)\[\/spoiler\]/gi, "<details><summary>Spoiler</summary>$1</details>"],
  [/\[heading\]([\s\S]*?)\[\/heading\]/gi, "<h3>$1</h3>"],
  [/\[line\]/gi, "<hr>"],
];

/** Balises de mise en forme abandonnées (taille, couleur, police, alignement). */
const DROPPED = /\[\/?(?:size|color|colour|font|center|centre|left|right|justify|indent|align|style|h[1-6])(?:=[^\]]*)?\]/gi;

const BLOCK = "(?:ul|ol|li|blockquote|pre|details|summary|h3|hr)";

export function bbcodeToHtml(input: string): string {
  let s = input.replace(/\r\n?/g, "\n").replace(/\n?<br\s*\/?>\n?/gi, "\n");
  s = escapeHtml(s);
  // Les balises imbriquées se résolvent de l'intérieur vers l'extérieur.
  for (let pass = 0; pass < 8; pass++) {
    const before = s;
    for (const [re, rep] of RULES) s = s.replace(re, rep as never);
    if (s === before) break;
  }
  s = s.replace(DROPPED, "");
  s = s.replace(/\n/g, "<br>");
  // Pas de saut de ligne collé aux blocs.
  s = s.replace(new RegExp(`(?:<br>\\s*)+(<\\/?${BLOCK}\\b)`, "g"), "$1");
  s = s.replace(new RegExp(`(<\\/?${BLOCK}\\b[^>]*>)(?:\\s*<br>)+`, "g"), "$1");
  s = s.replace(/(<br>\s*){3,}/g, "<br><br>");
  return s.trim();
}
