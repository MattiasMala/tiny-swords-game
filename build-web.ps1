# Builds the browser client into web/pkg (OWNER: C4).
#   .\build-web.ps1            then: cargo run -p ts-server --release   (http://localhost:8000)
#   .\build-web.ps1 -NoOpt     skip wasm-opt (faster, bigger)
# Steps: cargo (profile wasm-release: opt-level "z" (sim crate 3), fat LTO, 1 CGU) -> wasm-bindgen (must match the wasm-bindgen
# crate version in Cargo.lock) -> wasm-opt (binaryen; downloaded into ~/.cargo/bin when missing).
param([switch]$NoOpt)
# 'Continue': cargo / wasm-bindgen write progress to stderr, which Windows PowerShell 5.1 turns into terminating
# error records when the output is redirected; failures are detected through $LASTEXITCODE instead.
$ErrorActionPreference = 'Continue'
Set-Location $PSScriptRoot

$BinaryenVersion = 'version_133'
$Wasm = 'target/wasm32-unknown-unknown/wasm-release/ts_client.wasm'
$Out = 'web/pkg'

cargo build -p ts-client --lib --profile wasm-release --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# wasm-bindgen-cli must be exactly the version of the wasm-bindgen crate in Cargo.lock
$lock = Get-Content Cargo.lock -Raw
$m = [regex]::Match($lock, 'name = "wasm-bindgen"\r?\nversion = "([^"]+)"')
$want = if ($m.Success) { $m.Groups[1].Value } else { $null }
$have = $null
if (Get-Command wasm-bindgen -ErrorAction SilentlyContinue) {
  $have = ((wasm-bindgen --version) -split ' ')[1]
}
if ($want -and $have -ne $want) {
  Write-Host "installo wasm-bindgen-cli $want (trovato: $have)"
  cargo install wasm-bindgen-cli --version $want --locked
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
wasm-bindgen --target web --no-typescript --out-dir $Out --out-name ts_client $Wasm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$bg = Join-Path $Out 'ts_client_bg.wasm'
$before = (Get-Item $bg).Length
if (-not $NoOpt) {
  $opt = Get-Command wasm-opt -ErrorAction SilentlyContinue
  if (-not $opt) {
    $dest = Join-Path $env:USERPROFILE '.cargo\bin'
    $url = "https://github.com/WebAssembly/binaryen/releases/download/$BinaryenVersion/binaryen-$BinaryenVersion-x86_64-windows.tar.gz"
    Write-Host "wasm-opt non trovato: scarico binaryen $BinaryenVersion in $dest"
    try {
      $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("binaryen-" + [guid]::NewGuid())
      New-Item -ItemType Directory -Force $tmp -ErrorAction Stop | Out-Null
      $tgz = Join-Path $tmp 'b.tar.gz'
      Invoke-WebRequest -Uri $url -OutFile $tgz -UseBasicParsing -ErrorAction Stop
      tar -xzf $tgz -C $tmp
      if ($LASTEXITCODE -ne 0) { throw "tar: codice $LASTEXITCODE" }
      New-Item -ItemType Directory -Force $dest -ErrorAction Stop | Out-Null
      Copy-Item (Join-Path $tmp "binaryen-$BinaryenVersion\bin\wasm-opt.exe") $dest -Force -ErrorAction Stop
      Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
      $opt = Get-Command wasm-opt -ErrorAction SilentlyContinue
      if (-not $opt) { $opt = Get-Command (Join-Path $dest 'wasm-opt.exe') -ErrorAction Stop }
    } catch {
      Write-Host "download di wasm-opt fallito ($_): salto l'ottimizzazione"
      $opt = $null
    }
  }
  if ($opt) {
    # features used by rustc's default wasm32 target. -Oz: about 10 % smaller than -O3 (25 vs 28 MB); the browser JIT
    # inlines small hot functions anyway, and the sim crate is compiled at opt-level 3 by cargo
    & $opt.Source -Oz --strip-debug --strip-producers `
      --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals `
      --enable-reference-types --enable-multivalue `
      -o $bg $bg
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  }
}
$after = (Get-Item $bg).Length
Write-Host ("ts_client_bg.wasm: {0:N1} MB (prima di wasm-opt {1:N1} MB)" -f ($after / 1MB), ($before / 1MB))
Get-ChildItem $Out
