@echo off
rem ModStudio : recupere les dernieres modifications et lance l'appli en mode developpement.
title ModStudio (dev)
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

echo == Recuperation des dernieres modifications...
git checkout dev
git pull --ff-only
if errorlevel 1 (
  rem npm install peut modifier package-lock.json : on le remet comme sur GitHub, puis nouvel essai.
  echo.
  echo == Nouvel essai apres avoir remis package-lock.json comme sur GitHub...
  git checkout -- package-lock.json
  git pull --ff-only
)
if errorlevel 1 (
  echo.
  echo Impossible de mettre le code a jour. Copie ce message a Claude.
  pause
  exit /b 1
)

echo.
echo == Dependances de l'interface...
call npm install --no-audit --no-fund
if errorlevel 1 (
  echo.
  echo npm install a echoue. Copie ce message a Claude.
  pause
  exit /b 1
)
rem Garde package-lock.json identique a GitHub pour les prochaines mises a jour.
git checkout -- package-lock.json 2>nul

echo.
echo == Lancement de ModStudio (la fenetre s'ouvre une fois la compilation terminee)...
call npm run tauri dev
echo.
echo ModStudio est ferme. Si une erreur s'affiche au-dessus, copie-la a Claude.
pause
