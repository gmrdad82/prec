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

## Record a live agent session

A coding agent at work runs for minutes between milestones, so its tape waits on the screen with long timeouts and hides the stretches nobody needs to watch:

```text
Hide
Type "myagent"
Enter
Wait+Screen@60s /> $/
Ctrl+L
Show
Type "Add a --json flag to the list command"
Enter
Wait+Screen@600s /tests pass/
Hide
Wait+Screen@600s /Done/
Show
Sleep 3s
```

- `Hide` from the launch to the first prompt keeps the agent's start-up banner, with its account or plan, out of the picture; what is still on the screen at `Show` is in it, so clear the screen first, as `Ctrl+L` does here, when the banner stays drawn.
- `Wait+Screen@600s` gives a step up to ten minutes; the recording keeps going while it waits, and a `Hide` around a long wait cuts it from the clip.
- A token the agent needs goes in `pass_env` in [render/terminal.toml](set-up-your-app.md#a-session-with-no-private-data), never in the tape or `[env]`: prec hands it from its own environment to the pty and never writes its value.
- `home`, `hostname` and `cwd = "{home}/…"` give the session a neutral home, host and folder, so a banner or a tool line that prints them shows nothing of yours.

prec refuses to replay a cast whose picture shows your home path, hostname, user name, an email or a passed value ([Render a cast again](render-a-cast-again.md#a-picture-with-private-data)); `prec record` keeps the take and prints what it found and when.

## Stills

A still is taken 150 ms after its tape's last step, so end the tape on the screen you want, with a `Wait` or a `Sleep` that lets the app finish drawing.

## What a tape can't hold

VHS's settings commands are refused, so a tape never fights the recipe: `Output`, `Set`, `Require`, `Env` and `Source` each stop the run with `a tape holds steps only; the settings come from terminal.toml`. `Copy` and `Paste` are refused too: the clipboard has no replay, so type the text.
