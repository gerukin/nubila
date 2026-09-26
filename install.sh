#!/bin/sh
# Install/update from GitHub Releases. No sudo or shell-profile changes.
set -eu
case "${1:-}" in
  -h|--help) echo 'Usage: sh install.sh [VERSION|latest]'; echo 'NUBILA_INSTALL_DIR defaults to $HOME/.local/bin. Rerun to update.'; exit 0 ;;
esac
[ "$#" -le 1 ] || { echo 'Expected at most one version argument' >&2; exit 1; }
version=${1:-latest}
case "$version" in
  latest) base=https://github.com/gerukin/nubila/releases/latest/download ;;
  *) version=${version#v}
     case "$version" in ''|*[!0-9.]*) echo 'Invalid release version' >&2; exit 1 ;; esac
     base="https://github.com/gerukin/nubila/releases/download/v$version" ;;
esac
case "$(uname -s)" in
  Linux) os=unknown-linux-gnu ;;
  Darwin) os=apple-darwin ;;
  *) echo 'Use the Windows ZIP from GitHub Releases on Windows.' >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) echo 'Supported architectures: x86-64 and ARM64.' >&2; exit 1 ;;
esac
target="$arch-$os"
install_dir=${NUBILA_INSTALL_DIR:-"$HOME/.local/bin"}
work=$(mktemp -d)
pending=
cleanup() { rm -rf "$work"; if [ -n "$pending" ]; then rm -f "$pending"; fi; }
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
curl -fLsS --proto '=https' --tlsv1.2 --retry 2 "$base/SHA256SUMS" -o "$work/SHA256SUMS"
match=$(awk -v suffix="-$target.tar.gz" '$2 ~ /^nubila-[0-9][0-9.]*-/ && substr($2,length($2)-length(suffix)+1)==suffix { print $1, $2 }' "$work/SHA256SUMS")
[ "$(printf '%s\n' "$match" | wc -l | tr -d ' ')" = 1 ] && [ -n "$match" ] || {
  echo "Release has no unique artifact for $target" >&2; exit 1;
}
hash=${match%% *}
archive=${match#* }
case "$hash" in *[!0-9a-fA-F]*|'') echo 'Invalid checksum' >&2; exit 1 ;; esac
[ "${#hash}" = 64 ] || { echo 'Invalid checksum length' >&2; exit 1; }
case "$archive" in *[!a-zA-Z0-9._-]*) echo 'Invalid archive name' >&2; exit 1 ;; esac
if [ "$version" != latest ] && [ "$archive" != "nubila-$version-$target.tar.gz" ]; then
  echo 'Release version and archive disagree' >&2; exit 1
fi
curl -fLsS --proto '=https' --tlsv1.2 --retry 2 "$base/$archive" -o "$work/$archive"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$work/$archive")
else
  actual=$(shasum -a 256 "$work/$archive")
fi
[ "${actual%% *}" = "$hash" ] || { echo 'Checksum mismatch; existing installation unchanged.' >&2; exit 1; }
# Extract only known files, never arbitrary paths into the home directory.
tar -xzf "$work/$archive" -C "$work" ./nubila ./LICENSE ./LICENSE-tapp-ui ./THIRD_PARTY.html ./RELEASE.txt ./LICENSE-timezone-data ./TIMEZONE-DATA.txt
[ -f "$work/nubila" ] && [ ! -L "$work/nubila" ] || { echo 'Invalid executable' >&2; exit 1; }
mkdir -p "$install_dir"
[ ! -d "$install_dir/nubila" ] || { echo 'Destination is a directory' >&2; exit 1; }
notice_dir="$install_dir/../share/doc/nubila"
mkdir -p "$notice_dir"
for notice in LICENSE LICENSE-tapp-ui THIRD_PARTY.html RELEASE.txt LICENSE-timezone-data TIMEZONE-DATA.txt; do
  [ -f "$work/$notice" ] && [ ! -L "$work/$notice" ] || { echo 'Invalid release notice' >&2; exit 1; }
  cp "$work/$notice" "$notice_dir/$notice"
done
pending=$(mktemp "$install_dir/.nubila-install.XXXXXX")
cp "$work/nubila" "$pending"
chmod 755 "$pending"
# Same-filesystem replacement: no truncation or following existing symlinks.
mv -f "$pending" "$install_dir/nubila"
pending=
echo "Installed $archive to $install_dir/nubila"
case ":$PATH:" in *":$install_dir:"*) ;; *) echo "Add $install_dir to PATH to run nubila." ;; esac
echo 'Rerun to update. Remove the executable to uninstall; preferences and caches are preserved.'
