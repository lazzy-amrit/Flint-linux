#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
appdir="$repo_root/AppDir"
output_dir="${OUTPUT_DIR:-$repo_root/src-tauri/target/release/bundle/appimage}"

cd "$repo_root"

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$output_dir"

npm run build
npm run tauri -- build --no-bundle

cp "$repo_root/src-tauri/target/release/flint" "$appdir/usr/bin/flint"
chmod 0755 "$appdir/usr/bin/flint"

# Bundling pass only — no --output here. linuxdeploy's own packaging step
# regenerates AppRun and re-resolves dependencies, undoing both our custom
# AppRun and our --exclude-library flags. So we stop it right after bundling
# and finalize AppRun + package ourselves below.
linuxdeploy \
    --appdir "$appdir" \
    --executable "$appdir/usr/bin/flint" \
    --desktop-file "$repo_root/packaging/flint.desktop" \
    --icon-file "$repo_root/src-tauri/icons/flint.png" \
    --exclude-library 'libEGL.so*' \
    --exclude-library 'libGL.so*' \
    --exclude-library 'libGLX.so*' \
    --exclude-library 'libGLdispatch.so*' \
    --exclude-library 'libGLES*.so*' \
    --exclude-library 'libgbm.so*' \
    --exclude-library 'libwayland-client.so*' \
    --exclude-library 'libwayland-cursor.so*' \
    --exclude-library 'libwayland-egl.so*' \
    --exclude-library 'libwayland-server.so*' \
    --exclude-library 'libxkbcommon.so*' \
    --exclude-library 'libxkbcommon-x11.so*' \
    --exclude-library 'libX11.so*' \
    --exclude-library 'libXext.so*' \
    --exclude-library 'libXfixes.so*' \
    --exclude-library 'libXrandr.so*' \
    --exclude-library 'libxcb.so*'

# Install our AppRun AFTER linuxdeploy, so its own generated one (if any)
# doesn't win. linuxdeploy may leave AppRun as a symlink to usr/bin/flint;
# remove that symlink first, otherwise cp follows it and overwrites the real
# launcher binary with our wrapper, causing infinite self-execution.
rm -f "$appdir/AppRun"
cp "$repo_root/packaging/AppRun" "$appdir/AppRun"
chmod 0755 "$appdir/AppRun"

# WebKitGTK's helper processes are spawned by absolute path, not linked as
# shared libraries, so linuxdeploy's ldd-based scanning never bundles them.
webkit_exec_src=$(find /usr/lib -maxdepth 3 -type d -name 'webkit2gtk-4.1' -print -quit)
if [ -z "$webkit_exec_src" ]; then
    echo 'error: could not locate webkit2gtk-4.1 exec directory on build host' >&2
    exit 1
fi
mkdir -p "$appdir/usr/lib/webkit2gtk-4.1"
cp -a "$webkit_exec_src"/. "$appdir/usr/lib/webkit2gtk-4.1/"
for f in "$appdir/usr/lib/webkit2gtk-4.1"/*; do
    [ -f "$f" ] && file "$f" | grep -q ELF && patchelf --set-rpath '$ORIGIN/..' "$f"
done

# Ubuntu's production WebKitGTK library has its helper directory compiled as
# an absolute host path and ignores WEBKIT_EXEC_PATH unless developer mode is
# enabled. Rewrite that directory to a relative path so AppRun can launch the
# bundled helpers from their own directory.
python3 - "$appdir/usr/lib/libwebkit2gtk-4.1.so.0" <<'PY'
from pathlib import Path
import sys

library = Path(sys.argv[1])
data = bytearray(library.read_bytes())
compiled_path = b"/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1"
plen = len(compiled_path)

# This string can appear more than once: once as the standalone helper
# directory, and once as a prefix of the longer injected-bundle path
# (".../webkit2gtk-4.1/injected-bundle/"). Patch every occurrence in
# place, byte-for-byte, so the file's length never changes.
positions = []
start = 0
while True:
    idx = data.find(compiled_path, start)
    if idx == -1:
        break
    positions.append(idx)
    start = idx + 1

if not positions:
    raise SystemExit("expected at least one WebKit helper path, found 0")

for idx in positions:
    tail = bytes(data[idx + plen: idx + plen + 1])
    if tail in (b"", b"\x00"):
        # standalone string -> "." then pad with NULs
        replacement = b"." + b"\x00" * (plen - 1)
    else:
        # prefix of a longer string -> repeat "./" so the suffix
        # (e.g. "/injected-bundle/") still resolves relative to this dir
        replacement = (b"./" * (plen // 2 + 1))[:plen]
    data[idx:idx + plen] = replacement

library.write_bytes(bytes(data))
PY

if find "$appdir/usr/lib" \( -type f -o -type l \) -name 'libwayland-*.so*' | grep -q .; then
    echo 'error: AppDir still contains bundled libwayland libraries' >&2
    exit 1
fi

# Package directly with appimagetool — skips linuxdeploy's packaging pass
# entirely, so nothing it does can re-touch AppRun or re-add excluded libs.
export ARCH=x86_64
appimagetool "$appdir" "$output_dir/Flint-x86_64.AppImage"

echo "Wrote $output_dir/Flint-x86_64.AppImage"
