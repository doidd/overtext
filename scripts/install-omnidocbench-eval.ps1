$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$source = Join-Path $root 'logs/omnidocbench-eval'
$runtime = Join-Path $root 'logs/omnidocbench-python310'
$revision = 'f133a71e9e91c3621c7ce8994200a7b394a06eb3'
if (!(Test-Path $source)) {
    git clone --depth 1 https://github.com/opendatalab/OmniDocBench.git $source
    if ($LASTEXITCODE -ne 0) { throw 'Evaluator clone failed' }
}
if ((git -C $source rev-parse HEAD) -ne $revision) {
    git -C $source fetch --depth 1 origin $revision
    if ($LASTEXITCODE -ne 0) { throw 'Evaluator revision fetch failed' }
    git -C $source checkout --detach $revision
    if ($LASTEXITCODE -ne 0) { throw 'Evaluator revision checkout failed' }
}
New-Item -ItemType Directory -Force $runtime | Out-Null
$archive = Join-Path $runtime 'python-3.10.11-embed-amd64.zip'
$checksum = '608619f8619075629c9c69f361352a0da6ed7e62f83a0e19c63e0ea32eb7629d'
if (!(Test-Path $archive)) {
    curl.exe --fail --location --retry 2 --output $archive https://www.python.org/ftp/python/3.10.11/python-3.10.11-embed-amd64.zip
    if ($LASTEXITCODE -ne 0) { throw 'Python download failed' }
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLower() -ne $checksum) { throw 'Python checksum mismatch' }
if (!(Test-Path (Join-Path $runtime 'python.exe'))) { Expand-Archive -LiteralPath $archive -DestinationPath $runtime -Force }
$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $runtime 'python310._pth'), "python310.zip`n.`nimport site`n", $utf8)
$python = Join-Path $runtime 'python.exe'
& $python -m pip --version
if ($LASTEXITCODE -ne 0) {
    $bootstrap = Join-Path $runtime 'get-pip.py'
    curl.exe --fail --location --retry 2 --output $bootstrap https://bootstrap.pypa.io/get-pip.py
    if ($LASTEXITCODE -ne 0) { throw 'pip bootstrap download failed' }
    & $python $bootstrap --disable-pip-version-check
    if ($LASTEXITCODE -ne 0) { throw 'pip bootstrap failed' }
}
& $python -m pip install --disable-pip-version-check $source
if ($LASTEXITCODE -ne 0) { throw 'Evaluator dependency installation failed' }
Write-Output $python
