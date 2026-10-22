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
| `Idle <duration>` | waits until the screen hasn't changed for that long; default `2s` |
| `Squeeze [<gap>] [/regex/]`, `Squeeze off` | from here on, shortens every stretch where only a spinner or a counter moves to `gap` (default `500ms`) |
| `Resize <cols>x<rows> [<duration>]` | resizes the terminal, at once or over the duration; each side is 2 to 1000 |
| `Hide`, `Show` | cut the steps between them from the recording |
| `Screenshot` | keeps a marker in the cast |
| `# …` | a comment |

A duration is `500ms`, `1.5s`, `2m`, or a bare number of seconds.

## Speed and repeats

`Type` and key steps take `@<duration>`, the delay after each character or key: `Type@0ms "…"` types the text at once, `Down@200ms` waits 200 ms after the key. On `Wait` and `Idle` it is the timeout. A key step takes a repeat count: `Down@200ms 3` presses Down three times.

```text
Type@0ms "myapp tui"
Enter
Wait /ready/
Down@200ms 3
Enter
Ctrl+C
```

## Wait for the screen, not the clock

A `Sleep` guesses how long the app takes; a `Wait` watches the screen. `Wait /ready/` returns as soon as the last non-blank row matches the regex, and `Wait+Screen@5s /Loaded \d+ items/` searches the whole screen for up to 5 seconds. The default timeout is 15 seconds; on a timeout the recording stops with the rows it saw, so a tape that waits for the wrong text says what was there. A regex sees each row as it is displayed: a no-break space or any other Unicode space is a plain space, so `/⎿ +done/` matches a row an app drew as `⎿` and a no-break space, and zero-width characters are left out.

When nothing on the screen marks the end, wait for it to go still: `Idle 3s` returns once the screen, colours and cursor included, has stayed the same for 3 seconds, and `Idle@20m 3s` gives it up to 20 minutes (the default is 5). Output that redraws the same picture doesn't count as a change; a spinner or a running timer does, so an app that animates while it works is waited out.

## Hide setup steps

`Hide` and `Show` cut a stretch from the replay: the app still runs it, but its frames never show. For a command that only prepares the app, the shot's `launch` is simpler: prec types it hidden, waits `settle`, then starts showing. A tape that opens with `Hide` stays hidden from the launch to its first `Show`, so the launch line never reaches a frame.

## Squeeze the waits

An app that thinks for a minute behind a spinner makes a long, dull clip. `Squeeze 400ms` keeps recording at real time but shortens, from there on, every stretch where nothing but a spinner or a counter moves to 400 ms: half of it before the jump, half after. A stretch ends when a letter on the screen changes, so typed keys and printed text keep their real speed, while a spinner's glyph, a timer's digits and colours alone count as still. A `Sleep` keeps its full length, and `Squeeze off` stops squeezing.

A status row whose words change while it waits, such as an agent's `✻ Thinking… (12s · thinking)` that turns into `(14s · ↓ 300 tokens)`, is left out with a regex: `Squeeze 400ms /^. \w+… /` ignores every row it matches. The cast keeps the real times and marks each squeezed stretch, so the take's shown length, its clip and its private-data check all follow the squeezed timeline.

## Record a live agent session

A coding agent at work runs for minutes between milestones, so its tape waits on the screen with long timeouts and hides the stretches nobody needs to watch:

```text
Hide
Type "myagent"
Enter
Wait+Screen@60s /> $/
Ctrl+L
Show
Squeeze 400ms /^. \w+… /
Type "Add a --json flag to the list command"
Enter
Wait+Screen@600s /tests pass/
Hide
Idle@20m 5s
Show
Sleep 3s
```

- `Hide` from the launch to the first prompt keeps the agent's start-up banner, with its account or plan, out of the picture; what is still on the screen at `Show` is in it, so clear the screen first, as `Ctrl+L` does here, when the banner stays drawn.
- `Squeeze 400ms /^. \w+… /` cuts the agent's thinking down to 400 ms a stretch, leaving out its status row, while the prompt it is typed and the text it prints keep their pace.
- `Wait+Screen@600s` gives a step up to ten minutes; the recording keeps going while it waits, and a `Hide` around a long wait cuts it from the clip.
- `Idle@20m 5s` waits for the session to finish: the agent's spinner and timer change the screen while it works, so the step returns once it has stopped drawing for 5 seconds.
- A label that stays on the screen, such as the plan or organisation in a banner, is hidden with a `mask` in [render/terminal.toml](set-up-your-app.md#mask-a-region): `mask = [{ match = "Claude (Max|Pro)" }]` fills each match with the theme's background in every frame.
- A token the agent needs goes in `pass_env` in [render/terminal.toml](set-up-your-app.md#a-session-with-no-private-data), never in the tape or `[env]`: prec hands it from its own environment to the pty and never writes its value.
- `home`, `hostname` and `cwd = "{home}/…"` give the session a neutral home, host and folder, so a banner or a tool line that prints them shows nothing of yours.

prec refuses to replay a cast whose picture shows your home path, hostname, user name, an email, a passed value, a plan or organisation label, or a usage limit line that no mask hides ([Render a cast again](render-a-cast-again.md#a-picture-with-private-data)); `prec record` keeps the take and prints what it found and when.

## Resize the terminal

`Resize` changes the terminal's size mid-shot, so a clip can show an app laying itself out again as its window narrows and widens:

```text
Sleep 1.2s
Resize 38x16 3s
Sleep 1.5s
Resize 96x16 3s
Sleep 1.2s
```

`Resize 38x16` resizes at once. `Resize 38x16 3s` travels there over 3 seconds through every whole column and row in between, slow at both ends and fast in the middle (an ease-in-out sine curve), and lands on the target at exactly 3 seconds. Each step sets the pty's size, so the app gets a `SIGWINCH` and reads the new size as it would from a dragged window; a `Wait` or `Idle` after it sees the new grid, and the tape's later steps start from it. A resize to the size the terminal already has does nothing. A `Resize` takes no `@speed`, and a size that isn't `<cols>x<rows>` with each side from 2 to 1000, or a duration that doesn't read, is refused by its line.

The cast's header keeps the shot's starting grid and each step is an asciicast `r` event, such as `[2.4, "r", "61x16"]`. The render fits the font once, to the largest grid the cast holds, so the text never changes size and the output keeps its size; the window is centred for that largest grid, and a smaller grid keeps the same top-left corner, its window frame, bar and lights drawn around it, as if its right and bottom edges were dragged. A `mask` rectangle is checked against the starting grid and clipped to a smaller one; a `match` mask works on the screen as it is. The render's manifest adds `max_grid` when the cast resizes.

## Stills

A still is taken 150 ms after its tape's last step, so end the tape on the screen you want, with a `Wait` or a `Sleep` that lets the app finish drawing.

## What a tape can't hold

VHS's settings commands are refused, so a tape never fights the recipe: `Output`, `Set`, `Require`, `Env` and `Source` each stop the run with `a tape holds steps only; the settings come from terminal.toml`. `Copy` and `Paste` are refused too: the clipboard has no replay, so type the text.
