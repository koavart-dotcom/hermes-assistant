# Голос Джарвиса (XTTS v2, локально)

Клон голоса по образцам из набора реакций (`voice-pack/Jarvis`, 8 самых длинных клипов), русский язык,
на видеокарте ПК. Hermes вызывает его как TTS-провайдер `jarvis` (тип `command`).

- Установка (под пользователем Koava, PowerShell): `powershell -ExecutionPolicy Bypass -File install.ps1`
  — ставит в `D:\hermes\jarvis-tts` Python 3.11 (uv), PyTorch CUDA 12.4, coqui-tts; модель xtts_v2 (~1,9 ГБ)
  качается при первом старте. Всего ~6 ГБ на D:, ~2,5 ГБ видеопамяти во время работы.
- Включение: «Настройки → Распознавание и озвучка → Озвучка: Джарвис» или `hermesctl speech-set large-v3-turbo ru jarvis Jarvis`.
- Сервер (`server.py serve`, 127.0.0.1:8655) поднимается сам при первой фразе и держит модель в памяти;
  журнал — `server.log`.
- Лицензия модели: Coqui Public Model License — только личное некоммерческое использование.
