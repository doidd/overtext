param(
    [int]$PublicPerLanguage = 3,
    [int]$Repeats = 6,
    [string]$Engines = 'windows,rapid-mobile,rapid-server',
    [string]$ReportDirectory = 'logs/ocr-benchmark',
    [switch]$SkipInference,
    [switch]$Official
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$python = Join-Path $env:LOCALAPPDATA 'OverText/rapidocr/python.exe'
if (!(Test-Path $python)) { throw 'Install the OverText RapidOCR runtime first' }
Push-Location $root
try {
    if (!$SkipInference) {
        & $python scripts/ocr-corpus.py prepare --public-per-language $PublicPerLanguage
        if ($LASTEXITCODE -ne 0) { throw 'Corpus preparation failed' }
        if (!(Get-Command cargo -ErrorAction SilentlyContinue)) {
            $toolRoot = Join-Path $env:TEMP 'overtext-windows-tools'
            if (!(Test-Path (Join-Path $toolRoot 'cargo/bin/cargo.exe'))) { throw 'Cargo must be on PATH' }
            $env:CARGO_HOME = Join-Path $toolRoot 'cargo'
            $env:RUSTUP_HOME = Join-Path $toolRoot 'rustup'
            $env:PATH = "$env:CARGO_HOME/bin;$toolRoot/w64devkit/bin;$env:PATH"
            $env:RUSTFLAGS = '-C link-self-contained=yes'
        }
        $env:OVERTEXT_BENCHMARK_MANIFEST = Join-Path $root 'logs/ocr-benchmark/manifest.json'
        $env:OVERTEXT_BENCHMARK_OUTPUT = Join-Path $root 'logs/ocr-benchmark/raw.json'
        $env:OVERTEXT_BENCHMARK_REPEATS = "$Repeats"
        $env:OVERTEXT_BENCHMARK_ENGINES = $Engines
        cargo test --manifest-path src-tauri/Cargo.toml --test desktop benchmark_ocr_corpus --locked -- --ignored --nocapture --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw 'OCR benchmark failed' }
    }
    New-Item -ItemType Directory -Force $ReportDirectory | Out-Null
    Copy-Item -LiteralPath 'logs/ocr-benchmark/raw.json' -Destination (Join-Path $ReportDirectory 'raw-results.json') -Force
    Copy-Item -LiteralPath 'logs/ocr-benchmark/manifest.json' -Destination (Join-Path $ReportDirectory 'corpus-manifest.json') -Force
    & $python scripts/ocr-corpus.py score --output (Join-Path $ReportDirectory 'summary.json')
    if ($LASTEXITCODE -ne 0) { throw 'Corpus scoring failed' }
    node scripts/probe-ocr-render.mjs logs/ocr-benchmark/raw.json (Join-Path $ReportDirectory 'renderer.json')
    if ($LASTEXITCODE -ne 0) { throw 'Renderer probe failed' }
    if ($Official) {
        & ./scripts/run-omnidocbench-eval.ps1 -ReportDirectory $ReportDirectory
        & $python scripts/ocr-corpus.py score --output (Join-Path $ReportDirectory 'summary.json')
        if ($LASTEXITCODE -ne 0) { throw 'Official report aggregation failed' }
    }
} finally { Pop-Location }
