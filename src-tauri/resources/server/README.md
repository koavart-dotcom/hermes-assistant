# Server mode (hermesd)

The release workflow builds `hermesd.exe` (`crates/hermesd`) and puts it here, so the one
installer carries both the desktop app and the server service. The installer offers
`hermesd install --interactive`, which copies hermesd and the bundled resources to
`%ProgramFiles%\Hermes Assistant Server` and registers the Windows service. See
`docs/SERVER.md`. The exe is not committed.
