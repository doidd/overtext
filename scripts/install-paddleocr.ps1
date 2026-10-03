$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$runtimeDir = Join-Path $env:LOCALAPPDATA 'OverText/paddleocr'
$runtimePython = Join-Path $runtimeDir 'python.exe'
$readyFile = Join-Path $runtimeDir 'ready-v1'
$installLock = New-Object System.Threading.Mutex($false, 'Local\OverTextPaddleInstall')
if (!$installLock.WaitOne(0)) { throw 'PaddleOCR installation is already running.' }
try {
    if ((Test-Path -LiteralPath $readyFile) -and (Test-Path -LiteralPath $runtimePython)) { return }
    New-Item -ItemType Directory -Path $runtimeDir -Force | Out-Null
    if (!(Test-Path -LiteralPath $runtimePython)) {
        Write-Output 'Downloading isolated Python 3.12 (no administrator access required)...'
        $pythonZip = Join-Path $runtimeDir 'python.zip'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip' -OutFile $pythonZip
        Expand-Archive -LiteralPath $pythonZip -DestinationPath $runtimeDir -Force
    }
    # Enable site-packages in the official embeddable runtime, without editing PATH.
    $pythonPathFile = Join-Path $runtimeDir 'python312._pth'
    Set-Content -LiteralPath $pythonPathFile -Value "python312.zip`n.`nLib/site-packages`nimport site" -Encoding ASCII
    if (!(Test-Path -LiteralPath (Join-Path $runtimeDir 'Lib/site-packages/pip'))) {
        $pipBootstrap = Join-Path $runtimeDir 'get-pip.py'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://bootstrap.pypa.io/get-pip.py' -OutFile $pipBootstrap
        & $runtimePython $pipBootstrap --disable-pip-version-check --no-warn-script-location
        if ($LASTEXITCODE -ne 0) { throw 'Could not install pip into the isolated runtime.' }
    }
    Write-Output 'Installing CPU PaddleOCR...'
    & $runtimePython -m pip install --disable-pip-version-check --no-warn-script-location --only-binary=:all: 'paddlepaddle==3.2.2' 'paddleocr==3.3.2'
    if ($LASTEXITCODE -ne 0) { throw 'PaddleOCR package installation failed. Check the network and retry.' }
    $env:PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK = 'True'
    & $runtimePython -c 'import paddle; from paddleocr import PaddleOCR'
    if ($LASTEXITCODE -ne 0) { throw 'PaddleOCR runtime verification failed.' }
    Set-Content -LiteralPath $readyFile -Value 'Python 3.12.10 / PaddlePaddle 3.2.2 / PaddleOCR 3.3.2' -Encoding ASCII
    Write-Output 'Installed. Models download on first recognition and are then cached locally.'
} finally {
    $installLock.ReleaseMutex()
    $installLock.Dispose()
}
