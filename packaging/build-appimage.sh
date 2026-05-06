#!/usr/bin/env bash
# Build a self-contained AppImage of sdtd-server-manager.
#
# Output:
#   dist/sdtd-server-manager-x86_64.AppImage
#
# Two build profiles:
#
#   ./build-appimage.sh          # dynamic (glibc) — small file, needs
#                                  glibc >= the build host's version on
#                                  the target machine. Fine for current
#                                  Arch / Fedora / Ubuntu 22.04+.
#
#   ./build-appimage.sh --musl   # fully static via musl. Runs on
#                                  anything with a Linux kernel — no
#                                  glibc, no GTK, nothing required on
#                                  the target. Slightly bigger file.
#
# Build-host requirements:
#   - rustup + cargo
#   - curl, file, bash
#   - For --musl: `rustup target add x86_64-unknown-linux-musl` and
#     either musl-gcc (Arch: `pacman -S musl`) or the musl-cross toolchain.
#
# Run-time requirements on the END user's machine:
#   - For dynamic build: glibc >= build-host glibc, and DBus (every desktop
#     distro has it) for the file picker. NO GTK needed — we use the XDG
#     Desktop Portal which talks DBus to whatever your DE provides.
#   - For musl build: nothing. Just a kernel.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
APPDIR="$DIST/sdtd-server-manager.AppDir"
TOOL_DIR="$ROOT/.tools"
APPIMAGETOOL="$TOOL_DIR/appimagetool-x86_64.AppImage"

USE_MUSL=0
for arg in "$@"; do
    case "$arg" in
        --musl)  USE_MUSL=1 ;;
        --help|-h)
            sed -n '2,/^set -e/p' "$0" | head -n -1
            exit 0
            ;;
        *) echo "unknown arg: $arg" >&2; exit 2 ;;
    esac
done

# ── Sanity checks ───────────────────────────────────────────────────────
need() { command -v "$1" >/dev/null || { echo "missing tool: $1" >&2; exit 1; }; }
need cargo
need curl
need file

mkdir -p "$DIST" "$TOOL_DIR"

# ── Build the binary ────────────────────────────────────────────────────
if [ "$USE_MUSL" = "1" ]; then
    echo "==> Building static musl binary"
    rustup target list --installed | grep -q x86_64-unknown-linux-musl || {
        echo "musl target not installed. Run:" >&2
        echo "  rustup target add x86_64-unknown-linux-musl" >&2
        exit 1
    }
    ( cd "$ROOT" && cargo build --release --target x86_64-unknown-linux-musl )
    BIN="$ROOT/target/x86_64-unknown-linux-musl/release/sdtd-server-manager"
else
    echo "==> Building release binary (dynamic)"
    ( cd "$ROOT" && cargo build --release )
    BIN="$ROOT/target/release/sdtd-server-manager"
fi

if [ ! -x "$BIN" ]; then
    echo "expected binary at $BIN but didn't find it" >&2
    exit 1
fi

# Tell the user what they're shipping.
echo "==> Binary: $BIN"
echo "    size:   $(du -h "$BIN" | cut -f1)"
file "$BIN" | sed 's/^/    /'
if [ "$USE_MUSL" = "0" ]; then
    # For dynamic builds, surface the lowest glibc version required —
    # that's the floor for which distros this AppImage will run on.
    if command -v objdump >/dev/null; then
        glibc_min=$(objdump -T "$BIN" 2>/dev/null \
            | awk '/GLIBC_/ {print $5}' \
            | grep -oE 'GLIBC_[0-9.]+' \
            | sort -V | tail -1 || true)
        if [ -n "$glibc_min" ]; then
            echo "    requires: $glibc_min on target machine"
        fi
    fi
fi

# ── Stage AppDir ────────────────────────────────────────────────────────
echo "==> Staging AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$BIN" "$APPDIR/usr/bin/sdtd-server-manager"
chmod +x "$APPDIR/usr/bin/sdtd-server-manager"

# Icon. AppImage looks for a .png at AppDir root AND in the standard
# hicolor location.
if [ ! -f "$ROOT/assets/icon-256.png" ]; then
    echo "missing $ROOT/assets/icon-256.png — run \`python3 assets/make_icon.py\` first" >&2
    exit 1
fi
cp "$ROOT/assets/icon-256.png"  "$APPDIR/sdtd-server-manager.png"
cp "$ROOT/assets/icon-256.png"  "$APPDIR/usr/share/icons/hicolor/256x256/apps/sdtd-server-manager.png"

# Desktop entry. AppImage expects the .desktop to live at AppDir root
# and (by convention) under usr/share/applications.
cat > "$APPDIR/sdtd-server-manager.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=7DTD Server Manager
Comment=Self-hosted control panel for 7 Days to Die dedicated servers
Exec=sdtd-server-manager
Icon=sdtd-server-manager
Categories=Game;Network;
Terminal=true
EOF
cp "$APPDIR/sdtd-server-manager.desktop" \
   "$APPDIR/usr/share/applications/sdtd-server-manager.desktop"

# AppRun is the entry point. We pass through args, set NO_BROWSER if there's
# no display available (so the AppImage can work as a server-side daemon),
# and unset SOURCE_DATE_EPOCH which appimagetool sometimes leaks.
cat > "$APPDIR/AppRun" <<'EOF'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"

# If running headless (no DISPLAY/WAYLAND_DISPLAY), suppress the auto-open
# of the default browser, since there isn't one.
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ] && [ -z "${NO_BROWSER:-}" ]; then
    export NO_BROWSER=1
fi

exec "$HERE/usr/bin/sdtd-server-manager" "$@"
EOF
chmod +x "$APPDIR/AppRun"

# ── Fetch appimagetool ──────────────────────────────────────────────────
echo "==> Fetching appimagetool (one-time download)"
if [ ! -x "$APPIMAGETOOL" ]; then
    curl -fL --retry 3 -o "$APPIMAGETOOL" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL"
fi

# Some kernels disable user namespaces or FUSE — appimagetool needs both.
# `--appimage-extract-and-run` sidesteps both by extracting first.
echo "==> Packaging AppImage"
out="$DIST/sdtd-server-manager-x86_64.AppImage"
ARCH=x86_64 "$APPIMAGETOOL" --appimage-extract-and-run --no-appstream \
    "$APPDIR" "$out"

echo
echo "==================================================="
echo "OK: $out"
ls -lh "$out"
echo
echo "Test it:"
echo "  chmod +x $out"
echo "  $out"
if [ "$USE_MUSL" = "0" ]; then
    echo
    echo "If users on older distros report 'GLIBC_X.Y not found',"
    echo "rebuild with: $0 --musl"
fi
