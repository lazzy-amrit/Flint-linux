#!/usr/bin/env bash
# Copies the freshly built AppImage into Releases/<version>/, renaming it
# with the version baked into Cargo.toml so nothing gets overwritten by
# mistake across versions. Run this after packaging/build-appimage.sh
# finishes successfully.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="$(grep -m1 '^version' src-tauri/Cargo.toml | sed -E 's/version *= *"([^"]+)"/\1/')"
built_appimage="src-tauri/target/release/bundle/appimage/Flint-x86_64.AppImage"

if [[ ! -f "$built_appimage" ]]; then
    echo "error: $built_appimage not found — run packaging/build-appimage.sh first" >&2
    exit 1
fi

release_dir="Releases/${version}"
mkdir -p "$release_dir"
dest="$release_dir/Flint-${version}-x86_64.AppImage"
cp "$built_appimage" "$dest"
chmod +x "$dest"

echo "Published: $dest"
