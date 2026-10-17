# Vosk engine (bundled)

The release workflow puts the official vosk-api 0.3.45 Windows build here
(`libvosk.dll`, `libstdc++-6.dll`, `libgcc_s_seh-1.dll`, `libwinpthread-1.dll`,
SHA-256 of the zip is pinned in `.github/workflows/release.yml`). The DLLs are not
committed. The app loads them from the installed resources; it never downloads
executable code at runtime.
