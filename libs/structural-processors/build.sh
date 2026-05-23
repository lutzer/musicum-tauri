#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FRONTEND_STATIC="$SCRIPT_DIR/../../apps/frontend/static"

echo "==> Building structural-processors native binary..."
(cd "$SCRIPT_DIR" && cargo build --release --bin structural-processor)

echo "==> Generating descriptor JSON..."
"$SCRIPT_DIR/target/release/structural-processor" --descriptors \
    > "$FRONTEND_STATIC/structural-processor-descriptors.json"
echo "    wrote structural-processor-descriptors.json"

echo "==> Building WASM..."
(cd "$SCRIPT_DIR" && cargo build --target wasm32-unknown-unknown --release --lib)
cp "$SCRIPT_DIR/target/wasm32-unknown-unknown/release/structural_processors.wasm" \
    "$FRONTEND_STATIC/structural-processor.wasm"
echo "    wrote structural-processor.wasm"

echo "==> Copying native binary to backend..."
BACKEND_BIN="$SCRIPT_DIR/../../apps/backend/bin"
mkdir -p "$BACKEND_BIN"
cp "$SCRIPT_DIR/target/release/structural-processor" "$BACKEND_BIN/structural-processor"
echo "    wrote apps/backend/bin/structural-processor"

echo "Done."
