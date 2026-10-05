param([ValidateSet('mobile', 'server')][string]$Model = 'mobile')
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$runtimeDir = Join-Path $env:LOCALAPPDATA 'OverText/rapidocr'
$runtimePython = Join-Path $runtimeDir 'python.exe'
$installLock = New-Object System.Threading.Mutex($false, 'Local\OverTextRapidInstall')
if (!$installLock.WaitOne(0)) { throw 'RapidOCR installation is already running.' }
try {
    New-Item -ItemType Directory -Path $runtimeDir -Force | Out-Null
    if (!(Test-Path -LiteralPath $runtimePython)) {
        $pythonZip = Join-Path $runtimeDir 'python-embed.zip'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip' -OutFile $pythonZip
        Expand-Archive -LiteralPath $pythonZip -DestinationPath $runtimeDir -Force
    }
    Set-Content -LiteralPath (Join-Path $runtimeDir 'python312._pth') -Value "python312.zip`n.`nLib/site-packages`nimport site" -Encoding ASCII
    if (!(Test-Path -LiteralPath (Join-Path $runtimeDir 'Lib/site-packages/pip'))) {
        $bootstrap = Join-Path $runtimeDir 'get-pip.py'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://bootstrap.pypa.io/get-pip.py' -OutFile $bootstrap
        & $runtimePython $bootstrap --disable-pip-version-check --no-warn-script-location
        if ($LASTEXITCODE -ne 0) { throw 'Could not install pip.' }
    }
    $runtimeReady = Join-Path $runtimeDir 'runtime-v1'
    if (!(Test-Path -LiteralPath $runtimeReady)) {
        & $runtimePython -m pip install --disable-pip-version-check --no-warn-script-location 'rapidocr==3.9.2' 'onnxruntime==1.30.0'
        if ($LASTEXITCODE -ne 0) { throw 'RapidOCR package installation failed. Check network and retry.' }
        & $runtimePython -c 'from rapidocr import RapidOCR; import onnxruntime'
        if ($LASTEXITCODE -ne 0) { throw 'RapidOCR runtime verification failed.' }
        Set-Content -LiteralPath $runtimeReady -Value 'RapidOCR 3.9.2 / ONNX Runtime 1.30.0' -Encoding ASCII
    }
    $env:OVERTEXT_RAPID_DIR = $runtimeDir
    Write-Output "Downloading and verifying RapidOCR $Model models..."
    & $runtimePython -u (Join-Path $runtimeDir 'worker.py') --install --model $Model --warmup (Join-Path $runtimeDir 'warmup.png')
    if ($LASTEXITCODE -ne 0) { throw 'Model download/verification failed. Check network and retry.' }
    Write-Output "RapidOCR $Model installed. Screenshots are processed locally."
} finally {
    $installLock.ReleaseMutex()
    $installLock.Dispose()
}
