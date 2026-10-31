// Copie le moteur de traduction (Bergamot, WASM) dans static/bergamot pour que
// l'interface le charge comme un fichier de l'appli. Lancé avant dev et build.
import { copyFileSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const from = join(root, "node_modules/@browsermt/bergamot-translator/worker");
const to = join(root, "static/bergamot");
mkdirSync(to, { recursive: true });
for (const name of readdirSync(from)) copyFileSync(join(from, name), join(to, name));
