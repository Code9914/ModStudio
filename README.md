# ModStudio

**[English](#english)** · **[Français](#français)**

---

## English

A desktop app (Windows and Linux) to find, install and manage the mods of your
games from several sources: [mod.io](https://mod.io),
[Nexus Mods](https://www.nexusmods.com), [Thunderstore](https://thunderstore.io)
and mod lists run by some game communities. Built with Tauri 2, Rust and
Svelte 5. The interface is in French.

> **Unofficial** client: ModStudio is not affiliated with or endorsed by any of
> these sites.

### Features

| Source | What ModStudio does |
| --- | --- |
| **mod.io** | Browse every game and its mods, subscribe with your account, install into the game folder. |
| **Nexus Mods** | Browse without an account; install with your API key (Premium: direct download; free account: the file page opens inside the app and you click "Slow download"). FOMOD installers, zip / 7z / RAR archives, required mods. |
| **Thunderstore** | The site's games, with their mod loader (BepInEx, MelonLoader…) and each mod's dependencies, laid out the way r2modman does it. ResoniteModLoader mods for Resonite. |
| **Communities** | Hollow Knight (ModLinks), Outer Wilds (OWML), Beat Saber (BeatMods), ChilloutVR (CVRMG), Raft (RaftModding). |

For every game:

- **Games found automatically** in Steam and Epic Games (Heroic on Linux).
  ModStudio detects the engine (Unreal, Unity, Bethesda) and puts each mod in
  the right place. It can also install the tool a game needs (UE4SS, BepInEx,
  MelonLoader) once you confirm.
- **"Installés" page**: updates, enable or disable a mod without uninstalling
  it, per-game profiles, clean uninstall.
- **Share your mods**: a code you send to a friend, who gets the same mods in
  the same versions (see below).
- **Appearance**: light or dark theme, colours, text and thumbnail size.

### Download

Installers are in the [Releases](https://github.com/Code9914/ModStudio/releases/latest).

| System | File |
| --- | --- |
| Windows 10 / 11 | `ModStudio_x.y.z_x64-setup.exe` |
| Linux | `ModStudio_x.y.z_amd64.AppImage`: make it executable (`chmod +x`) and run it |
| Debian / Ubuntu | `ModStudio_x.y.z_amd64.deb` |
| Arch Linux / CachyOS | AUR package `modstudio-bin` (coming soon) |

ModStudio updates itself: it checks for a new version at each launch and offers
it. Updates are signed. When installed from the AUR, updates go through your
package manager instead.

The Windows installer isn't code-signed: on first launch, SmartScreen shows a
warning ("More info", then "Run anyway").

### Getting started

- **Thunderstore and Communities**: nothing to set up. Pick a game, choose its
  folder (or the one ModStudio found), then install.
- **Nexus Mods**: to install, create an API key at
  <https://www.nexusmods.com/settings/api-keys> and paste it in "Compte Nexus".
- **mod.io**: create an API key at <https://mod.io/me/access> and paste it with
  the API path shown next to it (`https://u-12345.modapi.io/v1`). To subscribe
  to mods, also sign in to your account (e-mail code or access token).
- **On Linux (Proton)**: when a mod loader relies on a DLL, ModStudio shows the
  Steam launch option to add (for example
  `WINEDLLOVERRIDES="winhttp=n,b" %command%`).

### Sharing mods

- **Share**: in "Installés", a game's "Partager" button gives an `MS1-…` code
  with its enabled mods and their versions. The code goes through no server:
  paste it in a chat. You can also save it as a `.modstudio` file.
- **Import**: "Installés", then "Importer un profil". ModStudio shows what you
  already have and installs the rest. For mod.io and Nexus Mods, the imported
  list becomes a profile of the game, and your previous mods stay in
  "Principal".
- **r2modman, Gale, Thunderstore Mod Manager**: their codes and `.r2z` files can
  be imported too. For a Thunderstore game, "Créer un code r2modman" makes a
  code a friend can use in those managers.

### Privacy and security

- **Keys**: the mod.io key stays on your computer (`settings.json` in
  ModStudio's config folder). The Nexus key and the mod.io sign-in are stored in
  the system keychain. Each is only sent to the site that issued it, over HTTPS.
- **No telemetry**: ModStudio sends no usage statistics.
- **Checked files**: mods only come from their source's site, over HTTPS. When
  the source publishes a checksum (mod.io, ResoniteModLoader and most community
  lists), every file is checked against it. A file flagged by mod.io's virus
  scan is never downloaded.
- **Careful installs**: extraction is protected against malicious paths and
  oversized archives. A file that already exists (yours or another mod's) is
  never overwritten. Uninstalling removes exactly the files that were
  installed.
- **Nexus page**: shown inside the app with no access to its commands. Downloads
  only start on your click, and the `nxm://` protocol isn't taken over on your
  system.
- **Mod descriptions**: sanitised (DOMPurify) and shown under a strict CSP.
  Links open in your browser.

### Building from source

You need Rust ([rustup](https://rustup.rs)), Node.js 20 or newer, and
[Tauri's system dependencies](https://tauri.app/start/prerequisites/)
(Windows: Visual Studio C++ build tools; Linux: `webkit2gtk-4.1` and its
development packages).

```bash
npm install
npm run tauri dev      # run the app from source
npm run tauri build    # build an installer (src-tauri/target/release/bundle/)
cd src-tauri && cargo test
```

On Windows, `scripts/setup.bat` installs what's missing, fetches the project
(`dev` branch) and creates a "ModStudio (dev)" shortcut that updates and runs
from source.

Development happens on the `dev` branch. A push to `main` publishes a release:
the "Build" workflow compiles, tests and publishes the Windows and Linux
installers, the update file (`latest.json`) and the AUR package.

```
src-tauri/src/        Rust backend
  api/, account.rs    mod.io (requests, account, subscriptions)
  nexus/              Nexus Mods (API, embedded page, game rules, FOMOD, tools)
  thunderstore/       Thunderstore and ResoniteModLoader
  lists/              community mod lists
  share/              shared profiles (MS1 codes, r2modman)
  games/              installed games (Steam, Epic, Heroic, engine detection)
  installer.rs        checked downloads, extraction, uninstall
  toggle.rs           enable / disable, profiles
src/                  Svelte interface (routes/+page.svelte, lib/components/)
packaging/aur/        AUR package modstudio-bin
```

---

## Français

Une appli de bureau (Windows et Linux) pour trouver, installer et gérer les
mods de tes jeux depuis plusieurs sources : [mod.io](https://mod.io),
[Nexus Mods](https://www.nexusmods.com), [Thunderstore](https://thunderstore.io)
et les listes tenues par certaines communautés. Construite avec Tauri 2, Rust
et Svelte 5.

> Client **non officiel** : ModStudio n'est affilié à aucun de ces sites, ni
> soutenu par eux.

### Ce qu'il fait

| Source | Ce que ModStudio sait faire |
| --- | --- |
| **mod.io** | Parcourir tous les jeux et leurs mods, s'abonner avec ton compte, installer dans le dossier du jeu. |
| **Nexus Mods** | Parcourir sans compte ; installer avec ta clé API (Premium : directement ; compte gratuit : la page du fichier s'ouvre dans l'appli et tu cliques sur « Slow download »). Installateurs FOMOD, archives zip / 7z / RAR, mods nécessaires. |
| **Thunderstore** | Les jeux du site, avec leur chargeur (BepInEx, MelonLoader…) et les dépendances de chaque mod, rangés comme le ferait r2modman. Mods de ResoniteModLoader pour Resonite. |
| **Communautés** | Hollow Knight (ModLinks), Outer Wilds (OWML), Beat Saber (BeatMods), ChilloutVR (CVRMG), Raft (RaftModding). |

Et pour tous les jeux :

- **Jeux trouvés tout seuls** dans Steam et Epic Games (Heroic sous Linux).
  ModStudio reconnaît le moteur (Unreal, Unity, Bethesda) et range chaque mod
  au bon endroit. Il installe aussi l'outil qu'il faut (UE4SS, BepInEx,
  MelonLoader) quand tu le confirmes.
- **Page « Installés »** : mises à jour, activer ou désactiver un mod sans le
  désinstaller, profils par jeu (« Principal », « Coop »…), désinstallation
  propre.
- **Partager ses mods** : un code à envoyer à un ami, qui installe les mêmes
  mods dans les mêmes versions (voir plus bas).
- **Apparence** : thème clair ou sombre, couleurs, taille du texte et des
  vignettes.

### Télécharger

Les installateurs sont dans les [Releases](https://github.com/Code9914/ModStudio/releases/latest).

| Système | Fichier |
| --- | --- |
| Windows 10 / 11 | `ModStudio_x.y.z_x64-setup.exe` |
| Linux | `ModStudio_x.y.z_amd64.AppImage` : rends-le exécutable (`chmod +x`) et lance-le |
| Debian / Ubuntu | `ModStudio_x.y.z_amd64.deb` |
| Arch Linux / CachyOS | paquet AUR `modstudio-bin` (bientôt) |

ModStudio se met à jour tout seul : il cherche une nouvelle version à chaque
lancement et te la propose. Les mises à jour sont signées. Installé depuis
l'AUR, il te laisse les faire avec ton gestionnaire de paquets.

L'installateur Windows n'est pas signé : au premier lancement, SmartScreen
affiche un avertissement (« Informations complémentaires », puis « Exécuter
quand même »).

### Premiers pas

- **Thunderstore et Communautés** : rien à configurer. Choisis un jeu, indique
  son dossier (ou prends celui que ModStudio a trouvé), puis installe.
- **Nexus Mods** : pour installer, crée une clé API sur
  <https://www.nexusmods.com/settings/api-keys> et colle-la dans « Compte Nexus ».
- **mod.io** : crée une clé API sur <https://mod.io/me/access> et colle-la avec
  le chemin d'API affiché à côté (`https://u-12345.modapi.io/v1`). Pour
  t'abonner à des mods, connecte-toi aussi à ton compte (code par e-mail ou
  jeton d'accès).
- **Sous Linux (Proton)** : quand un chargeur de mods passe par une DLL,
  ModStudio affiche l'option de lancement Steam à ajouter (par exemple
  `WINEDLLOVERRIDES="winhttp=n,b" %command%`).

### Partager ses mods

- **Partager** : dans « Installés », le bouton « Partager » d'un jeu donne un
  code `MS1-…` avec la liste de ses mods activés et leurs versions. Le code ne
  passe par aucun serveur : il se colle dans une discussion. Tu peux aussi
  l'enregistrer dans un fichier `.modstudio`.
- **Importer** : « Installés », puis « Importer un profil ». ModStudio montre ce
  que tu as déjà et installe le reste. Pour mod.io et Nexus Mods, le profil
  importé devient un profil du jeu, et tes mods d'avant restent dans
  « Principal ».
- **r2modman, Gale, Thunderstore Mod Manager** : leurs codes et fichiers `.r2z`
  s'importent aussi. Pour un jeu Thunderstore, « Créer un code r2modman »
  donne un code qu'un ami peut utiliser dans ces gestionnaires.

### Vie privée et sécurité

- **Clés** : la clé mod.io reste sur ton ordinateur (`settings.json` dans le
  dossier de configuration de ModStudio). La clé Nexus et la connexion mod.io
  sont rangées dans le trousseau du système. Chacune n'est envoyée qu'au site
  qui l'a créée, en HTTPS.
- **Statistiques** : ModStudio n'envoie aucune statistique d'utilisation.
- **Fichiers vérifiés** : les mods ne viennent que du site de leur source, en
  HTTPS. Quand la source publie une empreinte (mod.io, ResoniteModLoader et la
  plupart des listes des communautés), chaque fichier est contrôlé avec. Un
  fichier signalé par l'antivirus de mod.io n'est jamais téléchargé.
- **Installation prudente** : extraction protégée contre les chemins piégés et
  les archives démesurées. Un fichier qui existe déjà (à toi ou à un autre mod)
  n'est jamais écrasé. La désinstallation retire exactement les fichiers
  installés.
- **Page Nexus** : affichée dans l'appli sans aucun accès à ses commandes. Le
  téléchargement ne démarre que sur ton clic, et le protocole `nxm://` n'est pas
  réquisitionné sur le système.
- **Descriptions de mods** : nettoyées (DOMPurify) et affichées sous une
  politique CSP stricte. Les liens s'ouvrent dans ton navigateur.

### Compiler depuis les sources

Il te faut Rust ([rustup](https://rustup.rs)), Node.js 20 ou plus récent, et
les [dépendances système de Tauri](https://tauri.app/start/prerequisites/)
(sous Windows : les outils C++ de Visual Studio ; sous Linux : `webkit2gtk-4.1`
et ses paquets de développement).

```bash
npm install
npm run tauri dev      # lancer l'appli depuis le code
npm run tauri build    # produire un installateur (src-tauri/target/release/bundle/)
cd src-tauri && cargo test
```

Sous Windows, `scripts/setup.bat` installe ce qui manque, récupère le projet
(branche `dev`) et crée un raccourci « ModStudio (dev) » qui se met à jour et se
lance depuis le code.

Le développement se fait sur la branche `dev`. Un push sur `main` publie une
version : le workflow « Build » compile, teste, et publie les installateurs
Windows et Linux, le fichier de mise à jour (`latest.json`) et le paquet AUR.
L'organisation du code est décrite dans la partie anglaise, plus haut.
