# Changelog

## v1.0.0 — 2026-10-14

### Changed

- prec is 1.0.0: the same code as 0.22.0, now its first stable release.
- The README shows every feature with a picture and measured numbers, frame counts against VHS v0.11.0 on the same tape included.
- The brand now reads PITO MD in the README, the notices and the docs.

## v0.22.0 — 2026-10-12

### Added

- An optional config.toml at ${XDG_CONFIG_HOME:-~/.config}/prec/config.toml (or PREC_CONFIG): the archive root, the default theme, Chromium's path and opt-in [confine].
- A default theme, Catppuccin Mocha, for an app whose terminal.toml has no [theme].
- PREC_CHROMIUM names the Chromium or Chrome binary.
- prec doctor shows the config it read.

### Changed

- The archive is off unless the config's archive or PITO_ARCHIVE names a root; a render then records the skip reason unconfigured.
- systemd-run confinement and CPU pinning run only when the config's [confine] asks for them.
- Omarchy themes are read where Omarchy installs them, at run time; prec themes import and themes/omarchy.json are gone.
- The machine cache moved to ${XDG_CACHE_HOME:-~/.cache}/prec/.
- Paths under the working folder print relative to it on a terminal; piped output keeps absolute paths.
- Input prec can't parse gets clap's short answer everywhere, a bare prec and archive with no flag included.
- pito-jobs comes from its public repository at v0.5.0.

### Removed

- Crash reporting: prec reports to no one.
