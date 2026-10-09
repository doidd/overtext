param([string]$ReportDirectory = 'logs/ocr-benchmark')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$report = Join-Path $root $ReportDirectory
$source = Join-Path $root 'logs/omnidocbench-eval'
$python = Join-Path $root 'logs/omnidocbench-python310/python.exe'
$revision = 'f133a71e9e91c3621c7ce8994200a7b394a06eb3'
if (!(Test-Path $python)) { throw 'Run scripts/install-omnidocbench-eval.ps1 first' }
if ((git -C $source rev-parse HEAD) -ne $revision) { throw 'Unexpected evaluator revision' }
$env:PYTHONUTF8 = '1'
$env:HF_HOME = Join-Path $root 'logs/omnidocbench-hf-cache'
$env:OMNIDOCBENCH_MATCH_WORKERS = '1'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$freeze = & $python -m pip freeze
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect evaluator environment' }
[IO.File]::WriteAllText((Join-Path $report 'evaluator-requirements.txt'), (($freeze | Where-Object { $_ -notmatch '^omnidocbench-eval' }) -join "`n") + "`n", $utf8)
Push-Location $source
try {
    foreach ($dataset in @('omnidocbench', 'jasyn')) {
        foreach ($engine in @('windows', 'rapid-mobile', 'rapid-server')) {
            $config = Join-Path $report "official-inputs/$dataset/$engine.yaml"
            if (!(Test-Path $config)) { throw "Missing evaluator input: $config" }
            $destination = Join-Path $report "official-results/$dataset/$engine"
            New-Item -ItemType Directory -Force $destination | Out-Null
            $arguments = @('pdf_validation.py', '--config', ('"' + $config + '"'))
            $process = Start-Process -FilePath $python -ArgumentList $arguments -WorkingDirectory $source -WindowStyle Hidden -Wait -PassThru `
                -RedirectStandardOutput (Join-Path $destination 'stdout.log') -RedirectStandardError (Join-Path $destination 'stderr.log')
            if ($process.ExitCode -ne 0) { throw "Official evaluation failed: $dataset $engine (see $destination)" }
            $inputDirectory = Join-Path $report "official-inputs/$dataset"
            $hashes = @{}
            $hashes['ground-truth.json'] = (Get-FileHash -LiteralPath (Join-Path $inputDirectory 'ground-truth.json') -Algorithm SHA256).Hash.ToLowerInvariant()
            Get-ChildItem -LiteralPath (Join-Path $inputDirectory $engine) -File -Filter '*.md' | ForEach-Object {
                $hashes["$engine/$($_.Name)"] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            }
            [IO.File]::WriteAllText((Join-Path $destination 'input-sha256.json'), (($hashes | ConvertTo-Json) -replace "`r`n", "`n") + "`n", $utf8)
            $result = Join-Path $source 'result'
            Get-ChildItem -LiteralPath $result | Where-Object { $_.Name -like "$($engine)_quick_match*" } | ForEach-Object {
                Copy-Item -LiteralPath $_.FullName -Destination $destination -Recurse -Force
            }
            Write-Output "Official evaluation complete: $dataset $engine"
        }
    }
} finally { Pop-Location }
