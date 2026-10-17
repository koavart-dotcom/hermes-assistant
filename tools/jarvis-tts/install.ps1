# Jarvis voice (XTTS v2) for Hermes on Artem's PC. Run as the desktop user (Koava).
# Installs into D:\hermes\jarvis-tts: Python 3.11 venv (uv), PyTorch CUDA 12.4, coqui-tts,
# model xtts_v2 (~1.9 GB, downloaded on first start). Total ~6 GB on D:.
# Then switches Hermes TTS to the "jarvis" command provider.
param([string]$Root = "D:\hermes\jarvis-tts")
$ErrorActionPreference = "Stop"
$uv = Get-ChildItem "$env:LOCALAPPDATA\hermes\tools" -Recurse -Filter uv.exe | Select-Object -First 1 -ExpandProperty FullName
if (-not $uv) { throw "uv.exe not found under $env:LOCALAPPDATA\hermes\tools" }
New-Item -ItemType Directory -Force $Root | Out-Null
Copy-Item "$PSScriptRoot\server.py" $Root -Force
$env:UV_PYTHON_INSTALL_DIR = "$Root\python"
$env:UV_CACHE_DIR = "$Root\uv-cache"
$env:COQUI_TOS_AGREED = "1"
& $uv venv --python 3.11 "$Root\venv"
$py = "$Root\venv\Scripts\python.exe"
& $uv pip install --python $py --index-url https://download.pytorch.org/whl/cu124 torch torchaudio
& $uv pip install --python $py "coqui-tts>=0.24" soundfile
[Environment]::SetEnvironmentVariable("COQUI_TOS_AGREED", "1", "User")
# Hermes: command provider "jarvis"
$hermes = "$env:LOCALAPPDATA\hermes\bin\hermes.exe"
$cmd = "`"$py`" `"$Root\server.py`" say --text-file {input_path} --out {output_path}"
& $hermes config set tts.providers.jarvis.type command
& $hermes config set tts.providers.jarvis.command $cmd
& $hermes config set tts.providers.jarvis.timeout 300
Write-Output "jarvis-tts installed in $Root; warm-up:"
$t = New-TemporaryFile; Set-Content $t "Проверка голоса. Все системы в норме." -Encoding utf8
& $py "$Root\server.py" say --text-file $t --out "$Root\test.wav"
Write-Output "ok: $Root\test.wav"
