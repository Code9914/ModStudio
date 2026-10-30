#!/usr/bin/env python3
"""Réduit le schéma de l'écosystème Thunderstore à ce qu'utilise ModStudio.

Source : https://thunderstore.io/api/experimental/schema/dev/latest/
(aussi dans r2modman : src/assets/data/ecosystem.json).

    python3 scripts/trim-thunderstore-ecosystem.py ecosystem.json \
        > src-tauri/src/thunderstore/ecosystem.json

ModStudio télécharge la version à jour au besoin ; ce fichier sert hors ligne.
"""
import json
import sys

src = json.load(open(sys.argv[1], encoding="utf-8"))
games = {}
for key, game in sorted(src["games"].items()):
    entries = []
    for r in game.get("r2modman") or []:
        entries.append({
            "gameInstanceType": r.get("gameInstanceType"),
            "packageLoader": r.get("packageLoader"),
            "installRules": r.get("installRules") or [],
            "relativeFileExclusions": r.get("relativeFileExclusions"),
        })
    out = {"r2modman": entries}
    autolist = (game.get("thunderstore") or {}).get("autolistPackageIds")
    if autolist:
        out["thunderstore"] = {"autolistPackageIds": autolist}
    games[key] = out
communities = {
    key: {"autolistPackageIds": c["autolistPackageIds"]}
    for key, c in sorted(src.get("communities", {}).items())
    if c.get("autolistPackageIds")
}
loaders = [
    {"packageId": m["packageId"], "loader": m["loader"], "rootFolder": m.get("rootFolder", "")}
    for m in src["modloaderPackages"]
]
json.dump(
    {"schemaVersion": src.get("schemaVersion"), "games": games, "communities": communities, "modloaderPackages": loaders},
    sys.stdout,
    ensure_ascii=False,
    separators=(",", ":"),
)
