#!/usr/bin/env bash
set -e

echo "Building"

cd "/media/karol/kox/Nectan/desktop"

cargo build --release

APP_NAME=$(cargo metadata --no-deps --format-version 1 | grep -oP '"name":"\K[^"]+' | head -n 1)

mkdir -p ~/.local/bin

cp "../target/release/$APP_NAME" ~/.local/bin/

echo "Successfully installed $APP_NAME to ~/.local/bin/"
