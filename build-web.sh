#!/usr/bin/env sh
# Builds the browser client into web/pkg (OWNER: C4).
#   ./build-web.sh            then: cargo run -p ts-server --release   (http://localhost:8000)
#   NOOPT=1 ./build-web.sh    skip wasm-opt (faster, bigger)
# Steps: cargo (profile wasm-release: opt-level "z" (sim crate 3), fat LTO, 1 CGU) -> wasm-bindgen (must match the wasm-bindgen
# crate version in Cargo.lock) -> wasm-opt (binaryen; downloaded into ~/.cargo/bin when missing).
set -eu
cd "$(dirname "$0")"

BINARYEN_VERSION=version_133
WASM=target/wasm32-unknown-unknown/wasm-release/ts_client.wasm
OUT=web/pkg

cargo build -p ts-client --lib --profile wasm-release --target wasm32-unknown-unknown

# wasm-bindgen-cli must be exactly the version of the wasm-bindgen crate in Cargo.lock
want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)
have=""
if command -v wasm-bindgen >/dev/null 2>&1; then
  have=$(wasm-bindgen --version | awk '{ print $2 }')
fi
if [ -n "$want" ] && [ "$have" != "$want" ]; then
  echo "installo wasm-bindgen-cli $want (trovato: ${have:-nessuno})"
  cargo install wasm-bindgen-cli --version "$want" --locked
fi

rm -rf "$OUT"
wasm-bindgen --target web --no-typescript --out-dir "$OUT" --out-name ts_client "$WASM"

BG="$OUT/ts_client_bg.wasm"
size() { wc -c < "$1" | tr -d ' '; }
before=$(size "$BG")
if [ -z "${NOOPT:-}" ]; then
  OPT=""
  if command -v wasm-opt >/dev/null 2>&1; then
    OPT=wasm-opt
  else
    case "$(uname -s)-$(uname -m)" in
      Linux-x86_64) plat=x86_64-linux ;;
      Linux-aarch64) plat=aarch64-linux ;;
      Darwin-arm64) plat=arm64-macos ;;
      Darwin-x86_64) plat=x86_64-macos ;;
      MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64) plat=x86_64-windows ;;
      *) plat="" ;;
    esac
    if [ -n "$plat" ]; then
      dest="$HOME/.cargo/bin"
      tmp=$(mktemp -d)
      url="https://github.com/WebAssembly/binaryen/releases/download/$BINARYEN_VERSION/binaryen-$BINARYEN_VERSION-$plat.tar.gz"
      echo "wasm-opt non trovato: scarico binaryen $BINARYEN_VERSION in $dest"
      if curl -fsSL "$url" -o "$tmp/b.tar.gz" && tar -xzf "$tmp/b.tar.gz" -C "$tmp"; then
        mkdir -p "$dest"
        # the macOS / Linux archives link libbinaryen from ../lib: keep the layout next to the binary
        if [ -d "$tmp/binaryen-$BINARYEN_VERSION/lib" ]; then
          mkdir -p "$HOME/.cargo/lib"
          cp -R "$tmp/binaryen-$BINARYEN_VERSION/lib/." "$HOME/.cargo/lib/" 2>/dev/null || true
        fi
        for f in "$tmp/binaryen-$BINARYEN_VERSION/bin/wasm-opt" "$tmp/binaryen-$BINARYEN_VERSION/bin/wasm-opt.exe"; do
          [ -f "$f" ] && cp "$f" "$dest/"
        done
        if [ -x "$dest/wasm-opt" ]; then OPT="$dest/wasm-opt"; fi
        if [ -x "$dest/wasm-opt.exe" ]; then OPT="$dest/wasm-opt.exe"; fi
      else
        echo "download di wasm-opt fallito: salto l'ottimizzazione"
      fi
      rm -rf "$tmp"
    fi
  fi
  if [ -n "$OPT" ]; then
    # features used by rustc's default wasm32 target. -Oz: about 10 % smaller than -O3 (25 vs 28 MB); the browser JIT
    # inlines small hot functions anyway, and the sim crate is compiled at opt-level 3 by cargo
    "$OPT" -Oz --strip-debug --strip-producers \
      --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals \
      --enable-reference-types --enable-multivalue \
      -o "$BG" "$BG"
  else
    echo "wasm-opt non disponibile: salto l'ottimizzazione"
  fi
fi
after=$(size "$BG")
echo "ts_client_bg.wasm: $after byte (prima di wasm-opt $before)"
ls -l "$OUT"
