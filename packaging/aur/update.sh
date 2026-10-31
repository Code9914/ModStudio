#!/usr/bin/env bash
# Met le PKGBUILD et le .SRCINFO à la version donnée.
#   packaging/aur/update.sh <version> <sha256 du .deb> [pkgrel]
set -euo pipefail
version="$1"
sum="$2"
rel="${3:-1}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version invalide : $version" >&2; exit 1; }
[[ "$sum" =~ ^[0-9a-f]{64}$ ]] || { echo "empreinte invalide : $sum" >&2; exit 1; }
[[ "$rel" =~ ^[1-9][0-9]*$ ]] || { echo "révision invalide : $rel" >&2; exit 1; }
dir="$(cd "$(dirname "$0")" && pwd)"
old="$(sed -n 's/^pkgver=//p' "$dir/PKGBUILD")"
oldrel="$(sed -n 's/^pkgrel=//p' "$dir/PKGBUILD")"
sed -i -e "s/^pkgver=.*/pkgver=$version/" -e "s/^pkgrel=.*/pkgrel=$rel/" \
  -e "s/^sha256sums=.*/sha256sums=('$sum')/" "$dir/PKGBUILD"
sed -i -e "s/^\tpkgver = .*/\tpkgver = $version/" -e "s/^\tpkgrel = .*/\tpkgrel = $rel/" \
  -e "s/^\tsha256sums = .*/\tsha256sums = $sum/" \
  -e "/^\tsource = /s/ModStudio_${old}-${oldrel}_amd64/ModStudio_$version-${rel}_amd64/" \
  -e "/^\tsource = /s/$old/$version/g" "$dir/.SRCINFO"
echo "PKGBUILD : $old-$oldrel -> $version-$rel"
