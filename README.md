# ModStudio — client de bureau pour mod.io (v0.3)

ModStudio parcourt les mods de [mod.io](https://mod.io) depuis une appli de bureau
(Windows et Linux), construite avec Tauri 2, Rust et Svelte 5.

> Client **non officiel** : il n'est ni affilié à mod.io ni soutenu par mod.io.

## Parcourir (v0.1)

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

### Développer sur ton PC (mode rapide)

Pour essayer chaque modification sans attendre GitHub :

1. **Une seule fois :** double-clique sur `scripts/setup.bat`. Il installe ce
   qui manque (outils C++ de Visual Studio, Git, Node.js, Rust) avec winget,
   télécharge le projet dans `Documents\ModStudio` (branche `dev`) et crée le
   raccourci **« ModStudio (dev) »** sur le Bureau.
2. **À chaque nouvelle version :** double-clique sur « ModStudio (dev) »
   (`dev.bat`). Il récupère les dernières modifications de la branche `dev` et
   lance l'appli directement depuis le code (`npm run tauri dev`).
3. `build.bat` produit un installateur sur ton PC si besoin.

Le développement se fait sur la branche `dev`, que GitHub ne compile pas. Une
fois la version validée, `dev` est fusionnée dans `main` et GitHub produit
l'installateur officiel.

### Build Windows automatique (GitHub Actions)

À chaque push sur `main` (pas sur `dev`), le workflow `.github/workflows/build-windows.yml`
compile l'appli sur une machine Windows de GitHub, lance les tests, puis publie
l'installateur `.exe` et le paquet `.msi` dans l'onglet **Releases** du dépôt.
Le build se relance aussi à la main depuis l'onglet **Actions** (« Run workflow »).

L'installateur n'est pas signé : au premier lancement, Windows SmartScreen
affiche un avertissement. Clique sur « Informations complémentaires » puis
« Exécuter quand même ».

## Compte et abonnements (v0.2)

- Connexion au compte mod.io, au choix :
  - **par code e-mail** : ModStudio demande un code à mod.io, tu le recopies ;
  - **par jeton d'accès** : tu crées un jeton (lecture + écriture) sur
    <https://mod.io/me/access> et tu le colles. C'est la solution de repli si
    mod.io refuse la connexion par e-mail avec une clé API personnelle.
- Bouton « S'abonner » sur la fiche d'un mod, avec option pour s'abonner aussi
  aux mods nécessaires (dépendances).
- Badge « Abonné » dans les listes de mods.
- Page « Mes mods » : tous tes abonnements, regroupés par jeu, avec filtre et
  désabonnement (annulable tant que la page est ouverte).
- Les jeux qui gèrent mod.io téléchargent tes abonnements à leur prochain lancement.

## Installation manuelle (v0.3)

Pour les jeux qui chargent leurs mods depuis un dossier, sans passer par mod.io :

- Panneau « Sur ce PC » sur la fiche d'un mod : bouton « Installer », barre de
  progression, annulation, puis « Mettre à jour », « Ouvrir le dossier » et
  « Désinstaller ».
- Au premier mod d'un jeu, tu choisis son dossier de mods et le rangement :
  un sous-dossier par mod (par défaut), extraction directe dans le dossier, ou
  archive gardée telle quelle.
- Option pour installer aussi les mods nécessaires (dépendances). Ils sont
  notés comme tels : quand tu désinstalles le mod qui en avait besoin,
  ModStudio propose de les retirer aussi s'ils ne servent plus à rien.
- Jeux qui installent leurs mods eux-mêmes grâce au kit de mod.io : ModStudio
  les repère (dossier `C:\Users\Public\mod.io\<id du jeu>`, ou celui réglé dans
  `%LOCALAPPDATA%\mod.io\globalsettings.json`) et conseille de s'abonner
  plutôt que d'installer à la main.
- Aide au choix du dossier : dossier habituel proposé pour les jeux connus
  (Bonelab pour l'instant, créé s'il n'existe pas encore), alerte si le dossier
  choisi ressemble au dossier d'installation du jeu, conseils pour trouver le
  bon dossier.
- Page « Installés » : mods installés par jeu, versions, mises à jour
  disponibles (badge dans le menu), « Tout mettre à jour », désinstallation.
- Badges « Installé » et « Mise à jour » dans les listes de mods.

Sécurité de l'installation :

- Lien de téléchargement redemandé à mod.io juste avant chaque téléchargement,
  et accepté seulement s'il vient d'un domaine mod.io.
- Somme de contrôle MD5 vérifiée ; un fichier signalé par l'antivirus de
  mod.io n'est jamais téléchargé.
- Extraction protégée contre les chemins qui sortent du dossier (« zip slip »)
  et contre les archives démesurées (« zip bombs »).
- Aucun fichier existant n'est écrasé : si un fichier du mod existe déjà (à toi
  ou à un autre mod), l'installation s'arrête avant de toucher quoi que ce soit.
- Chaque fichier installé est noté dans `installed.json` : la désinstallation
  retire exactement ces fichiers, et laisse en place un dossier où tu as ajouté
  tes propres fichiers.

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
  commands.rs     commandes de navigation (jeux, mods, fichiers…)
  account.rs      connexion, déconnexion, abonnements
  installer.rs    téléchargement, MD5, extraction, placement, désinstallation
  install_commands.rs  commandes d'installation (dossiers, progression…)
  modio_sdk.rs    détection des jeux qui gèrent leurs mods via mod.io
  auth.rs         jeton de connexion et trousseau du système
  settings.rs     réglages (clé, chemin d'API), vérification des domaines
  error.rs        erreurs envoyées à l'interface ({ kind, message, retryAfter })
  api/client.rs   requêtes HTTP vers mod.io, gestion des erreurs 401 / 429
  api/models.rs   objets mod.io (tolérants aux champs nuls ou manquants)
src/
  routes/+page.svelte        coquille : rail de navigation + vues
  lib/api.ts                 types et appels au backend
  lib/nav.svelte.ts          navigation entre les vues
  lib/paged.svelte.ts        listes paginées « afficher plus »
  lib/components/            GamesView, ModsView, ModView, LibraryView,
                             AccountView, InstalledView, InstallPanel,
                             FolderSetup, SettingsView…
  lib/installs.svelte.ts     état des mods installés
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

Pour le compte : connexion par code et par jeton, rangement du jeton dans le
trousseau, session restaurée au lancement, session expirée oubliée, messages
clairs pour un code expiré ou refusé, abonnements idempotents, pagination de
« Mes mods », et déconnexion qui ne révoque que les jetons obtenus par e-mail.

Pour l'installation : installation complète contre un faux mod.io, MD5 faux,
téléchargement annulé, fichier signalé par l'antivirus, « zip slip »,
« zip bomb », conflits de fichiers, mise à jour qui retire les anciens
fichiers, désinstallation qui garde les fichiers ajoutés par l'utilisateur, et
découpage du jeton pour la limite du Gestionnaire d'identification Windows.

## Sécurité

- La clé API ne quitte l'appli que vers un domaine mod.io
  (`api.mod.io`, `*.modapi.io`, `*.test.mod.io`), toujours en HTTPS.
- Elle est enregistrée dans `settings.json`, dans le dossier de configuration :
  `%APPDATA%\fr.modstudio.app\` sous Windows,
  `~/.config/fr.modstudio.app/` sous Linux (lisible par toi seul).
- Le jeton de connexion (accès en écriture au compte) est rangé dans le
  trousseau du système : Gestionnaire d'identification sous Windows, Secret
  Service (GNOME Keyring, KWallet) sous Linux. L'interface ne le voit jamais.
  Si le trousseau est indisponible, la session reste en mémoire et l'appli le
  signale.
- Se déconnecter révoque le jeton obtenu par e-mail. Un jeton créé à la main
  sur mod.io est seulement oublié : supprime-le sur mod.io/me/access pour le
  révoquer.
- Les descriptions de mods (HTML écrit par leurs auteurs) passent par
  DOMPurify, et l'appli applique une politique CSP stricte. Les liens s'ouvrent
  dans ton navigateur, jamais dans l'appli.

## Idées pour la suite

- Noter les mods (pouce haut / bas).
- Profils de mods (activer ou désactiver un ensemble de mods d'un coup).
- Build Linux (.AppImage, .deb) dans GitHub Actions.
