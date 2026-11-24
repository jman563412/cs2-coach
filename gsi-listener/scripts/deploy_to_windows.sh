#!/usr/bin/env bash
# Builds the listener and copies it, with its Steam launch wrapper, to a local
# Windows drive.
#
# The listener is already a native Windows binary -- WSL only hosts the
# toolchain -- but running it straight out of the repo means Steam launches it
# over \\wsl.localhost\..., which Windows maps to the Internet zone and prompts
# about on every single launch (unsigned publisher, and the "don't ask again"
# answer has nowhere to live on ext4). It also means a stopped WSL distro has to
# cold-start before the game can come up. Deploying to a local drive avoids both.
#
# Point Steam's CS2 launch options at the deployed wrapper:
#   "C:\Windows\System32\wscript.exe" "D:\cs2-coach\bin\launch_with_listener.vbs" %command%
set -euo pipefail

DEST="${1:-/mnt/d/cs2-coach/bin}"
TARGET=x86_64-pc-windows-gnu

cd "$(dirname "$0")/.."

if [ ! -d "$(dirname "$DEST")" ]; then
    echo "destination parent $(dirname "$DEST") does not exist" >&2
    exit 1
fi

cargo build --release --target "$TARGET"

mkdir -p "$DEST"
cp "target/$TARGET/release/gsi-listener.exe" "$DEST/"
cp scripts/launch_with_listener.vbs "$DEST/"

echo "deployed to $DEST"
