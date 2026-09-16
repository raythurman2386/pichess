#!/usr/bin/env bash
# Install Pichess into ~/.local (binary, desktop entry, icon). No root.
set -euo pipefail

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"

cd "$ROOT"
cargo build --release --locked

install -Dm755 "$ROOT/target/release/pichess" "$PREFIX/bin/pichess"
install -Dm644 "$ROOT/dist/pichess.desktop" "$PREFIX/share/applications/pichess.desktop"
install -Dm644 "$ROOT/dist/pichess.svg" "$PREFIX/share/icons/hicolor/scalable/apps/pichess.svg"
install -Dm644 "$ROOT/LICENSE" "$PREFIX/share/licenses/pichess/LICENSE"
install -Dm644 "$ROOT/fonts/OFL.txt" "$PREFIX/share/licenses/pichess/OFL.txt"

if command -v rsvg-convert >/dev/null 2>&1; then
  tmp="$(mktemp --suffix=.png)"
  rsvg-convert -w 128 -h 128 "$ROOT/dist/pichess.svg" -o "$tmp"
  install -Dm644 "$tmp" "$PREFIX/share/icons/hicolor/128x128/apps/pichess.png"
  rm -f "$tmp"
elif command -v magick >/dev/null 2>&1; then
  tmp="$(mktemp --suffix=.png)"
  magick -background none "$ROOT/dist/pichess.svg" -resize 128x128 "$tmp"
  install -Dm644 "$tmp" "$PREFIX/share/icons/hicolor/128x128/apps/pichess.png"
  rm -f "$tmp"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Installed pichess to $PREFIX/bin/pichess"
echo "Launcher: $PREFIX/share/applications/pichess.desktop"
if [[ ":$PATH:" != *":$PREFIX/bin:"* ]]; then
  echo "Add $PREFIX/bin to PATH if the launcher cannot find pichess."
fi