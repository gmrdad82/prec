---
title: Write a tape
description: The tape's steps, waiting on the screen instead of guessing a sleep, and hidden stretches.
sidebar:
  order: 3
---

A tape is the keystrokes of one shot, one step a line. It holds only steps: the size, theme, font and the rest live in [render/terminal.toml](set-up-your-app.md), so the same tape renders at any size or theme. This is the sample app's `typing.tape`:

```text
Sleep 300ms
Type "demo tick"
Enter
Sleep 1.6s
Type "demo show"
Enter
Sleep 1.2s
```

## Steps

| step | what it does |
|---|---|
| `Type "…"` | types the text, one character every `typing` (default 40 ms); quote it with `"`, `'` or `` ` `` |
| `Enter`, `Backspace`, `Delete`, `Tab`, `Shift+Tab`, `Escape`, `Space` | one key |
| `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown` | one key |
| `Ctrl+<key>`, `Alt+<key>`, `Ctrl+Alt+<key>` | a chord, such as `Ctrl+C` or `Alt+Enter` |
| `Sleep <duration>` | waits |
| `Wait /regex/` | waits until the screen's last non-blank row matches |
| `Wait+Screen /regex/` | waits until anywhere on the screen matches |
| `Hide`, `Show` | cut the steps between them from the recording |
| `Screenshot` | keeps a marker in the cast |
| `# …` | a comment |

A duration is `500ms`, `1.5s`, `2m`, or a bare number of seconds.

## Speed and repeats

`Type` and key steps take `@<duration>`, the delay after each character or key: `Type@0ms "…"` types the text at once, `Down@200ms` waits 200 ms after the key. On `Wait` it is the timeout. A key step takes a repeat count: `Down@200ms 3` presses Down three times.

```text
Type@0ms "myapp tui"
Enter
Wait /ready/
Down@200ms 3
Enter
Ctrl+C
```

## Wait for the screen, not the clock

A `Sleep` guesses how long the app takes; a `Wait` watches the screen. `Wait /ready/` returns as soon as the last non-blank row matches the regex, and `Wait+Screen@5s /Loaded \d+ items/` searches the whole screen for up to 5 seconds. The default timeout is 15 seconds; on a timeout the recording stops with the rows it saw, so a tape that waits for the wrong text says what was there.

## Hide setup steps

`Hide` and `Show` cut a stretch from the replay: the app still runs it, but its frames never show. For a command that only prepares the app, the shot's `launch` is simpler: prec types it hidden, waits `settle`, then starts showing.

## Stills

A still is taken 150 ms after its tape's last step, so end the tape on the screen you want, with a `Wait` or a `Sleep` that lets the app finish drawing.

## What a tape can't hold

VHS's settings commands are refused, so a tape never fights the recipe: `Output`, `Set`, `Require`, `Env` and `Source` each stop the run with `a tape holds steps only; the settings come from terminal.toml`. `Copy` and `Paste` are refused too: the clipboard has no replay, so type the text.
