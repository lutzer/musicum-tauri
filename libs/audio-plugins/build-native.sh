#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DEST="$SCRIPT_DIR/../../apps/backend/bin"
mkdir -p "$DEST"

PLUGINS=(gain reverb normalize pan oscilloscope level-meter)

for plugin in "${PLUGINS[@]}"; do
    plugin_dir="$SCRIPT_DIR/$plugin"
    echo "==> building ap-$plugin"
    (cd "$plugin_dir" && cargo build --bin "ap-$plugin" --release)
    cp "$plugin_dir/target/release/ap-$plugin" "$DEST/ap-$plugin"
    echo "    copied ap-$plugin to $DEST/"
done

echo "Done."
