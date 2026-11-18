# Builds the browser client into web/pkg (OWNER: C4). Usage: .\build-web.ps1   then: cargo run -p ts-server
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
cargo build -p ts-client --lib --profile wasm-release --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
wasm-bindgen --target web --no-typescript --out-dir web/pkg --out-name ts_client `
  target/wasm32-unknown-unknown/wasm-release/ts_client.wasm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if (Get-Command wasm-opt -ErrorAction SilentlyContinue) {
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int `
    -o web/pkg/ts_client_bg.wasm web/pkg/ts_client_bg.wasm
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} else {
  Write-Host "wasm-opt non trovato: salto l'ottimizzazione"
}
Get-ChildItem web/pkg
