#!/usr/bin/env sh
# Builds the browser client into web/pkg (OWNER: C4). Usage: ./build-web.sh   then: cargo run -p ts-server
set -eu
cd "$(dirname "$0")"
cargo build -p ts-client --lib --profile wasm-release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg --out-name ts_client \
  target/wasm32-unknown-unknown/wasm-release/ts_client.wasm
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o web/pkg/ts_client_bg.wasm web/pkg/ts_client_bg.wasm
else
  echo "wasm-opt non trovato: salto l'ottimizzazione"
fi
ls -l web/pkg
