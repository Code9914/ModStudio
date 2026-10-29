# ModStudio — client de bureau pour mod.io (v0.1)

ModStudio parcourt les mods de [mod.io](https://mod.io) depuis une appli de bureau
(Windows et Linux), construite avec Tauri 2, Rust et Svelte 5.

> Client **non officiel** : il n'est ni affilié à mod.io ni soutenu par mod.io.

## Ce que fait la v0.1 (lecture seule)

- Premier lancement guidé : saisie et test de la clé API mod.io.
- Liste de tous les jeux mod.io, avec recherche et tri.
- Liste des mods d'un jeu : recherche, 7 tris (tendances, plus téléchargés,
  mieux notés, nouveautés…), filtres par tags du jeu, chargement progressif.
- Fiche d'un mod : galerie d'images, description, statistiques, évaluations,
  plateformes, tags, dépendances (cliquables) et historique des versions avec
  le statut antivirus de chaque fichier.
- Option pour masquer les mods marqués « contenu adulte ».
- Thème clair ou sombre selon le système. `Échap` revient en arrière.

## Prérequis

### Windows

1. **Microsoft C++ Build Tools** : installe-les avec la charge de travail
   « Développement Desktop en C++ ».
2. **Rust** : installe-le via [rustup](https://rustup.rs) (toolchain MSVC par défaut).
3. **Node.js** 20 ou plus récent.
4. **WebView2** : déjà présent sur Windows 10 et 11.

### Linux (CachyOS / Arch)

```bash
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg nodejs npm
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Linux (Debian / Ubuntu)

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

## Lancer l'appli

```bash
npm install
npm run tauri dev
```

La première compilation Rust prend quelques minutes, les suivantes sont rapides.

Pour produire un installateur (`.msi` / `.exe` sous Windows, `.deb` / `.AppImage` /
`.rpm` sous Linux) :

```bash
npm run tauri build
```

Les fichiers sortent dans `src-tauri/target/release/bundle/`.

### Build Windows automatique (GitHub Actions)

À chaque push sur `main`, le workflow `.github/workflows/build-windows.yml`
compile l'appli sur une machine Windows de GitHub, lance les tests, puis publie
l'installateur `.exe` et le paquet `.msi` dans l'onglet **Releases** du dépôt.
Le build se relance aussi à la main depuis l'onglet **Actions** (« Run workflow »).

L'installateur n'est pas signé : au premier lancement, Windows SmartScreen
affiche un avertissement. Clique sur « Informations complémentaires » puis
« Exécuter quand même ».

## Obtenir ta clé API

1. Ouvre <https://mod.io/me/access> et connecte-toi.
2. Dans la section API, crée une clé.
3. Copie la clé **et** le chemin d'API affiché à côté
   (de la forme `https://u-12345.modapi.io/v1`), puis colle-les dans ModStudio.

La clé est en lecture seule : elle permet de parcourir, pas de modifier ton compte.

## Organisation du code

```
src-tauri/src/
  lib.rs          démarrage de l'appli, enregistrement des commandes
  commands.rs     commandes appelées par l'interface (jeux, mods, fichiers…)
  settings.rs     réglages (clé, chemin d'API), vérification des domaines
  error.rs        erreurs envoyées à l'interface ({ kind, message, retryAfter })
  api/client.rs   requêtes HTTP vers mod.io, gestion des erreurs 401 / 429
  api/models.rs   objets mod.io (tolérants aux champs nuls ou manquants)
src/
  routes/+page.svelte        coquille : rail de navigation + vues
  lib/api.ts                 types et appels au backend
  lib/nav.svelte.ts          navigation entre les vues
  lib/paged.svelte.ts        listes paginées « afficher plus »
  lib/components/            GamesView, ModsView, ModView, SettingsView…
```

## Tests

```bash
cd src-tauri
cargo test
```

Ils couvrent la lecture des réponses mod.io (champs nuls, tableaux vides),
la gestion des erreurs 401 et 429 (avec nouvel essai automatique si l'attente
est courte), la liste blanche des tris et des domaines d'API, et le fait que la
clé API n'apparaît jamais dans un message d'erreur.

## Sécurité

- La clé API ne quitte l'appli que vers un domaine mod.io
  (`api.mod.io`, `*.modapi.io`, `*.test.mod.io`), toujours en HTTPS.
- Elle est enregistrée dans `settings.json`, dans le dossier de configuration :
  `%APPDATA%\fr.modstudio.app\` sous Windows,
  `~/.config/fr.modstudio.app/` sous Linux (lisible par toi seul).
- Les descriptions de mods (HTML écrit par leurs auteurs) passent par
  DOMPurify, et l'appli applique une politique CSP stricte. Les liens s'ouvrent
  dans ton navigateur, jamais dans l'appli.

## Prochaine étape : v0.2

Connexion au compte mod.io par code e-mail, abonnements, page « Mes mods ».
Le jeton de connexion sera rangé dans le trousseau du système.
