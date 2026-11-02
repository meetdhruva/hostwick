#!/bin/sh
# Installs Hostwick for the current user (no root needed):
#   binaries -> ~/.local/bin, icon + menu entry -> ~/.local/share
# Run from the extracted archive folder. Re-run to upgrade.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$bin" "$data/applications" "$data/icons/hicolor/512x512/apps"
install -m 755 "$here/hostwick" "$here/hostwick-desktop" "$bin/"
install -m 644 "$here/hostwick.png" "$data/icons/hicolor/512x512/apps/hostwick.png"
sed "s|^Exec=.*|Exec=$bin/hostwick-desktop|" "$here/hostwick.desktop" > "$data/applications/hostwick.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$data/icons/hicolor" || true
echo "Installed to $bin. Hostwick is now in your applications menu."
case ":$PATH:" in *":$bin:"*) ;; *) echo "Note: add $bin to your PATH to run 'hostwick' from a terminal." ;; esac
