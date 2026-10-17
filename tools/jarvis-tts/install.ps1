# Jarvis voice (XTTS v2) for Hermes on Artem's PC. Run as the desktop user (Koava).
# Installs into D:\hermes\jarvis-tts: Python 3.11 venv (uv), PyTorch CUDA 12.4, coqui-tts,
# model xtts_v2 (~1.9 GB, downloaded on first start). Total ~6 GB on D:.
# Then switches Hermes TTS to the "jarvis" command provider.
param([string]$Root = "D:\hermes\jarvis-tts")
# Native tools print progress on stderr: do not let PowerShell treat it as an error.
$ErrorActionPreference = "Continue"
$env:UV_NO_PROGRESS = "1"
$uv = Get-ChildItem "$env:LOCALAPPDATA\hermes\tools" -Recurse -Filter uv.exe | Select-Object -First 1 -ExpandProperty FullName
if (-not $uv) { throw "uv.exe not found under $env:LOCALAPPDATA\hermes\tools" }
New-Item -ItemType Directory -Force $Root | Out-Null
Copy-Item "$PSScriptRoot\server.py" $Root -Force
$env:UV_PYTHON_INSTALL_DIR = "$Root\python"
$env:UV_CACHE_DIR = "$Root\uv-cache"
$env:COQUI_TOS_AGREED = "1"
$py = "$Root\venv\Scripts\python.exe"
if (-not (Test-Path $py)) { cmd /c "`"$uv`" venv --python 3.11 `"$Root\venv`" 2>&1" }
cmd /c "`"$uv`" pip install --python `"$py`" --index-url https://download.pytorch.org/whl/cu124 torch torchaudio 2>&1"
# coqui-tts 0.27 needs transformers 4.57+, but transformers 5 dropped an API it uses.
cmd /c "`"$uv`" pip install --python `"$py`" `"coqui-tts>=0.27,<0.28`" `"transformers>=4.57,<5`" soundfile 2>&1"
[Environment]::SetEnvironmentVariable("COQUI_TOS_AGREED", "1", "User")
# Hermes: command provider "jarvis"
$hermes = "$env:LOCALAPPDATA\hermes\bin\hermes.exe"
$cmd = "`"$py`" `"$Root\server.py`" say --text-file {input_path} --out {output_path}"
& $hermes config set --force tts.providers.jarvis.type command
& $hermes config set --force tts.providers.jarvis.command $cmd
& $hermes config set --force tts.providers.jarvis.format wav
& $hermes config set --force tts.providers.jarvis.voice Jarvis
& $hermes config set --force tts.providers.jarvis.timeout 300
Write-Output "jarvis-tts installed in $Root; warm-up:"
$t = New-TemporaryFile; Set-Content $t "Проверка голоса. Все системы в норме." -Encoding utf8
& $py "$Root\server.py" say --text-file $t --out "$Root\test.wav"
Write-Output "ok: $Root\test.wav"
