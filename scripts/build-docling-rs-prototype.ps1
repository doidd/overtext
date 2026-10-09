param([switch]$SkipModels)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$source = Join-Path $root 'logs/docling-rs-source'
$worker = Join-Path $root 'logs/docling-rs-worker'
$models = Join-Path $root 'logs/docling-rs-models'
$revision = '29de9d1e842e6ebb35890c3f171ac0c38f273eed'
if (!(Test-Path $source)) {
    git clone --depth 1 --branch v1.104.2 https://github.com/docling-project/docling.rs.git $source
    if ($LASTEXITCODE -ne 0) { throw 'Docling.rs clone failed' }
}
$actual = git -C $source rev-parse HEAD
if ($actual -ne $revision) { throw "Expected Docling.rs $revision, got $actual" }
New-Item -ItemType Directory -Force $worker, $models | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
$manifest = @'
[package]
name = "overtext-docling-prototype"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "overtext-docling-worker"
path = "../../scripts/docling-rs-worker.rs"

[dependencies]
docling-pdf = { path = "../docling-rs-source/crates/docling-pdf", features = ["ort-load-dynamic"] }
serde_json = "1"
serde = { version = "1", features = ["derive"] }
image = "0.25"

[profile.release]
opt-level = 2
'@
[IO.File]::WriteAllText((Join-Path $worker 'Cargo.toml'), $manifest.Replace("`r`n", "`n") + "`n", $utf8)
if (!$SkipModels) {
    # Fixed checksums: models-v1 is a mutable release; never trust its name alone.
    $assets = @{
        'layout_heron_int8.onnx' = '387be1f67bd58ec7fc575c53966492890d91af0810866bb383af45a18cc66161'
        'ocr_det.onnx' = '090f04abcd9d9a7498bc4ebf677e4cb9bdce1fe4197ddb7e529f1ef44e1ff94f'
        'ocr_rec_v6.onnx' = '6f327246b50388f3c176ae304bd95767ea6dc0c9ae92153ef8cbe210b3c14884'
        'ocr_rec_v6_dict.txt' = 'b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d'
    }
    foreach ($name in $assets.Keys) {
        $target = Join-Path $models $name
        if (!(Test-Path $target)) {
            curl.exe --fail --location --retry 2 --output $target "https://github.com/docling-project/docling.rs/releases/download/models-v1/$name"
            if ($LASTEXITCODE -ne 0) { throw "Download failed: $name" }
        }
        if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLower() -ne $assets[$name]) {
            throw "Checksum mismatch: $name. Remove this file before retrying."
        }
    }
}
$env:ORT_SKIP_DOWNLOAD = '1'
cargo build --manifest-path (Join-Path $worker 'Cargo.toml') --release
if ($LASTEXITCODE -ne 0) { throw 'Docling.rs worker build failed' }
Write-Output (Join-Path $worker 'target/release/overtext-docling-worker.exe')
