#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET_DIR="$SCRIPT_DIR/../../apps/frontend/static/plugins"

# Auto-discover all plugin directories (any subdirectory).
# Plugins with a Cargo.toml: generate JSON from native descriptor binary, then compile WASM.
# Plugins without a Cargo.toml (e.g. trim): copy their hand-written <name>.json directly.
IFS=$'\n' read -r -d '' -a PLUGINS < <(find "$SCRIPT_DIR" -maxdepth 1 -mindepth 1 -type d | xargs -n1 basename | sort && printf '\0')

mkdir -p "$TARGET_DIR"

for plugin in "${PLUGINS[@]}"; do
    plugin_dir="$SCRIPT_DIR/$plugin"

    echo "==> $plugin"

    if [[ -f "$plugin_dir/Cargo.toml" ]]; then
        # Generate descriptor JSON by running the native descriptor binary.
        (cd "$plugin_dir" && cargo build --bin "${plugin}-descriptor" --release 2>/dev/null)
        "$plugin_dir/target/release/${plugin}-descriptor" > "$TARGET_DIR/$plugin.json"
        echo "    generated $plugin.json"

        # Build AudioWorklet WASM (no features — pure C ABI).
        (cd "$plugin_dir" && cargo build --target wasm32-unknown-unknown --release)
        rust_name="${plugin//-/_}"
        cp "$plugin_dir/target/wasm32-unknown-unknown/release/$rust_name.wasm" "$TARGET_DIR/$plugin.wasm"
        echo "    built and copied $plugin.wasm"

        # If the plugin ships a renderer.js, copy it to the renderers output folder.
        if [[ -f "$plugin_dir/renderer.js" ]]; then
            cp "$plugin_dir/renderer.js" "$TARGET_DIR/$plugin.renderer.js"
            echo "    copied renderer.js for $plugin"
        fi
    else
        # JSON-only plugin (e.g. trim): copy the hand-written descriptor.
        json_file="$plugin_dir/$plugin.json"
        if [[ -f "$json_file" ]]; then
            cp "$json_file" "$TARGET_DIR/$plugin.json"
            echo "    copied $plugin.json"
        else
            echo "WARNING: $plugin/$plugin.json not found and no Cargo.toml — skipping"
            continue
        fi
    fi
done

# Generate index.json listing all available plugin IDs in order.
{
    printf '[\n'
    first=true
    for plugin in "${PLUGINS[@]}"; do
        [[ -f "$TARGET_DIR/$plugin.json" ]] || continue
        [[ "$first" == "true" ]] || printf ',\n'
        printf '  "%s"' "$plugin"
        first=false
    done
    printf '\n]\n'
} > "$TARGET_DIR/index.json"
echo "==> wrote index.json"

echo "Done. Processed ${#PLUGINS[@]} edit type(s)."
