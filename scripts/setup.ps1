# ModStudio : installation des outils et du projet sur ce PC.
# À lancer une seule fois, via setup.bat (double-clic).

$ErrorActionPreference = "Stop"
$Repo = "https://github.com/Code9914/ModStudio.git"
$Dir = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "ModStudio"

function Step($text) { Write-Host ""; Write-Host "== $text" -ForegroundColor Yellow }
function Ok($text) { Write-Host "   $text" -ForegroundColor Green }
function Has($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
function Refresh-Path {
    $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
                [Environment]::GetEnvironmentVariable("Path", "User")
}
function Install-Tool($id, $name, $extra = @()) {
    Write-Host "   Installation de $name (Windows peut demander une autorisation)..."
    winget install --id $id -e --accept-source-agreements --accept-package-agreements @extra
    # -1978335189 et -1978335135 : déjà installé / rien à mettre à jour.
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189 -and $LASTEXITCODE -ne -1978335135) {
        throw "L'installation de $name a échoué (code $LASTEXITCODE)."
    }
    Refresh-Path
}

try {
    if (-not (Has "winget")) {
        throw "winget est introuvable. Installe « App Installer » depuis le Microsoft Store, puis relance ce script."
    }

    Step "Outils de compilation C++ (Visual Studio Build Tools)"
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    $vc = $null
    if (Test-Path $vswhere) {
        $vc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    }
    if ($vc) { Ok "Déjà installés ($vc)" }
    else {
        Write-Host "   Téléchargement d'environ 2 à 3 Go, cela peut prendre un moment."
        Install-Tool "Microsoft.VisualStudio.2022.BuildTools" "Visual Studio Build Tools" @(
            "--override", "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
        )
    }

    Step "Git"
    if (Has "git") { Ok "Déjà installé" } else { Install-Tool "Git.Git" "Git" }

    Step "Node.js"
    if (Has "node") { Ok "Déjà installé ($(node --version))" } else { Install-Tool "OpenJS.NodeJS.LTS" "Node.js" }

    Step "Rust"
    if (-not (Has "rustup")) { Install-Tool "Rustlang.Rustup" "Rust" }
    Refresh-Path
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if ((Test-Path $cargoBin) -and ($env:Path -notlike "*$cargoBin*")) { $env:Path += ";$cargoBin" }
    rustup default stable | Out-Null
    Ok "$(rustc --version)"

    Step "Projet ModStudio"
    if (Test-Path (Join-Path $Dir ".git")) {
        Ok "Déjà présent dans $Dir"
    } else {
        Write-Host "   Ton navigateur peut s'ouvrir pour te connecter à GitHub : accepte."
        git clone $Repo $Dir
        if ($LASTEXITCODE -ne 0) { throw "Le téléchargement du projet depuis GitHub a échoué." }
    }
    Set-Location $Dir
    git fetch origin
    git checkout dev
    git pull --ff-only
    Ok "Code à jour (branche dev)"

    Step "Dépendances de l'interface"
    npm install --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw "npm install a échoué." }

    Step "Raccourci sur le Bureau"
    $desktop = [Environment]::GetFolderPath("Desktop")
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut((Join-Path $desktop "ModStudio (dev).lnk"))
    $link.TargetPath = Join-Path $Dir "dev.bat"
    $link.WorkingDirectory = $Dir
    $link.IconLocation = (Join-Path $Dir "src-tauri\icons\icon.ico")
    $link.Save()
    Ok "Raccourci « ModStudio (dev) » créé"

    Write-Host ""
    Write-Host "Tout est prêt." -ForegroundColor Green
    Write-Host "Double-clique sur « ModStudio (dev) » sur ton Bureau pour lancer l'appli."
    Write-Host "La première compilation prend 5 à 10 minutes, les suivantes beaucoup moins."
} catch {
    Write-Host ""
    Write-Host "Erreur : $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Copie ce message à Claude pour qu'il t'aide."
}
Write-Host ""
Read-Host "Appuie sur Entrée pour fermer"
