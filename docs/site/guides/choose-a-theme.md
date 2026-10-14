---
title: Choose a theme
description: Render with the app's palette, a built-in scheme or an Omarchy theme, or several at once.
sidebar:
  order: 6
---

`--theme` picks the palette a render paints with:

- `app` (the default): the `[theme]` in the app's `render/terminal.toml`, else the config's `theme`, else `Catppuccin Mocha`;
- any of 348 built-in schemes, VHS v0.11.0's set, compiled into prec (VHS itself isn't needed);
- `omarchy:<name>`, an [Omarchy](https://omarchy.org) theme installed on this machine.

## List them

```sh
prec themes
```

```text
app                          the palette in the app's render/terminal.toml, else "Catppuccin Mocha"

curated pairs (light, dark):
  catppuccin     "Catppuccin Latte", "Catppuccin Mocha"
  tokyo-night    "TokyoNightLight", "TokyoNight"
  gruvbox        "Gruvbox Light", "GruvboxDark"
  solarized      "Builtin Solarized Light", "Builtin Solarized Dark"
  github         "Github", "GitHub Dark"
  one-half       "OneHalfLight", "OneHalfDark"
  nord           "nord-light", "nord"
  also: "Rose Pine", "Dracula", "Kanagawa"

omarchy (installed here):
  omarchy:catppuccin               dark
  omarchy:catppuccin-latte         light
  omarchy:flexoki-light            light
  …
```

`prec themes --all` lists every built-in scheme too. The Omarchy section lists what is installed on the machine, read at run time from `/usr/share/omarchy/themes`, `$OMARCHY_PATH/themes`, `~/.local/share/omarchy/themes` and `~/.config/omarchy/themes` (each theme's `colors.toml`, else its `kitty.conf`).

## Several themes from one recording

`--themes` records once and renders once per theme. These six came from one run:

```sh
prec still demo palette --app-dir examples/demo --themes "Catppuccin Latte,Catppuccin Mocha,TokyoNightLight,TokyoNight,Gruvbox Light,GruvboxDark"
```

![Six stills of the demo palette in a grid, light on the left and dark on the right: Catppuccin Latte and Mocha, Tokyo Night Light and Tokyo Night, Gruvbox Light and Gruvbox Dark.](../images/themes.png)

Each theme other than the app's own adds its slug to the file names, such as `demo-still-palette-catppuccin-latte-1920x1080.png` or `omarchy-flexoki-light`, and the manifest's `theme` records the scheme, its label and the rendered background and ground colours.

## The app's own palette

A `[theme]` table in the recipe sets the app's palette: `background`, `foreground`, `selection`, `cursor` and the 16 colours `black` to `white` and `brightBlack` to `brightWhite`, each `#rrggbb`. A theme only sets colours the app doesn't choose itself: an app that paints in truecolor shows its own colours in every theme.
