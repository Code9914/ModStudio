#!/usr/bin/env bash
# Met le PKGBUILD et le .SRCINFO à la version donnée.
#   packaging/aur/update.sh 0.12.0 <sha256 du .deb>
set -euo pipefail
version="$1"
sum="$2"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version invalide : $version" >&2; exit 1; }
[[ "$sum" =~ ^[0-9a-f]{64}$ ]] || { echo "empreinte invalide : $sum" >&2; exit 1; }
dir="$(cd "$(dirname "$0")" && pwd)"
old="$(sed -n 's/^pkgver=//p' "$dir/PKGBUILD")"
sed -i -e "s/^pkgver=.*/pkgver=$version/" -e "s/^pkgrel=.*/pkgrel=1/" \
  -e "s/^sha256sums=.*/sha256sums=('$sum')/" "$dir/PKGBUILD"
sed -i -e "s/^\tpkgver = .*/\tpkgver = $version/" -e "s/^\tpkgrel = .*/\tpkgrel = 1/" \
  -e "s/^\tsha256sums = .*/\tsha256sums = $sum/" -e "/^\tsource = /s/$old/$version/g" "$dir/.SRCINFO"
echo "PKGBUILD : $old -> $version"
