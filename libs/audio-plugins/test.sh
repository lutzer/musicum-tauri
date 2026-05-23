#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

for plugin in $(find "$SCRIPT_DIR" -maxdepth 1 -mindepth 1 -type d | xargs -n1 basename | sort); do
    plugin_dir="$SCRIPT_DIR/$plugin"

    if [[ ! -f "$plugin_dir/Cargo.toml" ]]; then
        continue
    fi

    echo "==> testing $plugin"
    (cd "$plugin_dir" && cargo test)
done

echo "Done."
