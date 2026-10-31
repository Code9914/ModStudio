# ModStudio

Une appli de bureau (Windows et Linux) pour trouver, installer et gérer les
mods de tes jeux depuis plusieurs sources : [mod.io](https://mod.io),
[Nexus Mods](https://www.nexusmods.com), [Thunderstore](https://thunderstore.io)
et les listes tenues par certaines communautés. Construite avec Tauri 2, Rust
et Svelte 5.

> Client **non officiel** : ModStudio n'est affilié à aucun de ces sites, ni
> soutenu par eux.

## Ce qu'il fait

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

## Télécharger

Les installateurs sont dans les [Releases](https://github.com/Code9914/ModStudio/releases/latest).

| Système | Fichier |
| --- | --- |
| Windows 10 / 11 | `ModStudio_x.y.z_x64-setup.exe` |
| Linux | `ModStudio_x.y.z_amd64.AppImage` : rends-le exécutable (`chmod +x`) et lance-le |
| Debian / Ubuntu | `ModStudio_x.y.z_amd64.deb` |
| Arch Linux / CachyOS | paquet AUR `modstudio-bin` (en préparation) |

ModStudio se met à jour tout seul : il cherche une nouvelle version à chaque
lancement et te la propose. Les mises à jour sont signées. Installé depuis
l'AUR, il te laisse les faire avec ton gestionnaire de paquets.

L'installateur Windows n'est pas signé : au premier lancement, SmartScreen
affiche un avertissement (« Informations complémentaires », puis « Exécuter
quand même »).

## Premiers pas

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

## Partager ses mods

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

## Vie privée et sécurité

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

## Compiler depuis les sources

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

### Organisation du code

```
src-tauri/src/        backend Rust
  api/, account.rs    mod.io (requêtes, compte, abonnements)
  nexus/              Nexus Mods (API, page intégrée, règles des jeux, FOMOD, outils)
  thunderstore/       Thunderstore et ResoniteModLoader
  lists/              listes des communautés
  share/              profils partagés (code MS1, r2modman)
  games/              jeux installés (Steam, Epic, Heroic, moteur)
  installer.rs        téléchargement vérifié, extraction, désinstallation
  toggle.rs           activer / désactiver, profils
src/                  interface Svelte (routes/+page.svelte, lib/components/)
packaging/aur/        paquet AUR modstudio-bin
```

### Pour le mainteneur

- **Signature des mises à jour** : la clé publique est dans `tauri.conf.json`.
  La clé privée et son mot de passe sont dans les secrets du dépôt
  (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Sans
  elle, plus aucune mise à jour possible : garde-en une copie.
- **Paquet AUR** : publié automatiquement quand le secret `AUR_SSH_PRIVATE_KEY`
  existe. Pour l'activer :
  1. Crée un compte sur <https://aur.archlinux.org/register>.
  2. Ajoutes-y la clé publique d'une paire faite pour l'AUR
     (`ssh-keygen -t ed25519 -f aur`).
  3. Range la clé privée dans ce secret.

  Sans ce secret, le PKGBUILD de chaque version reste dans les artefacts du
  workflow.
