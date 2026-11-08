# Themes

`--theme` picks the palette:

- `app` (default): the recipe's `[theme]`, else the config's `theme`, else `Catppuccin Mocha`.
- Any of 348 built-in schemes (VHS v0.11.0's set, compiled in).
- `omarchy:<name>`: an Omarchy theme installed on this machine.

```sh
prec themes
prec themes --all
```

`prec themes` lists `app`, the curated light and dark pairs (catppuccin, tokyo-night, gruvbox,
solarized, github, one-half, nord), the installed Omarchy themes; `--all` adds every built-in
scheme. Names with spaces are quoted: `--theme "Catppuccin Latte"`.

## Several themes from one recording

`--themes "Catppuccin Latte,Catppuccin Mocha"` records once and renders once per theme. Each
theme other than the app's adds its slug to the file names (`...-catppuccin-latte-1920x1080.png`),
and the manifest's `theme` records the scheme and its colours.

## The app's own palette

A theme only sets colours the app does not choose itself: an app that paints in truecolor shows
its own colours in every theme. `[theme]` in the recipe takes `background`, `foreground`,
`selection`, `cursor` and the 16 colours `black` to `brightWhite`, each `#rrggbb`.

For drafts, use one theme. For light and dark deliveries, use a curated pair.
