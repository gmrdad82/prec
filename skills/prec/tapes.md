# Tapes

A tape is the keystrokes of one shot, one step a line. It holds only steps; size, theme and font
live in the recipe, so one tape renders at any size or theme. This is `examples/demo`'s
`typing.tape`:

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

- `Type "..."` types one character every `typing` (quote with `"`, `'` or a backtick).
- Keys: `Enter`, `Backspace`, `Delete`, `Tab`, `Shift+Tab`, `Escape`, `Space`, `Up`, `Down`,
  `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`; chords `Ctrl+<key>`, `Alt+<key>`,
  `Ctrl+Alt+<key>`, such as `Ctrl+C` or `Alt+Enter`.
- `Sleep <duration>`; a duration is `500ms`, `1.5s`, `2m` or bare seconds.
- `Wait /regex/` waits until the screen's last non-blank row matches; `Wait+Screen /regex/` until
  anywhere on the screen matches; `Wait+Exit` until the shell has exited (it takes no regex).
- `Idle <duration>` waits until the screen has not changed that long (default `2s`).
- `Squeeze [<gap>] [/regex/]` shortens every stretch where only a spinner or counter moves to
  `gap` (default `500ms`); the regex leaves out rows whose words change; `Squeeze off` stops it.
- `Resize <cols>x<rows> [<duration>]`, each side 2 to 1000; with a duration it travels eased.
- `Hide` and `Show` cut the steps between them from the recording.
- `Screenshot` keeps a marker in the cast; `# ...` is a comment.

`@<duration>` after `Type` or a key is the delay after each character or key; after `Wait`,
`Wait+Screen`, `Wait+Exit` or `Idle` it is the timeout. `Type@0ms "text"` types at once and
`Down@200ms 3` presses Down three times (a repeat count is 1 to 9999). `Sleep`, `Squeeze` and
`Resize` take no `@`.

## Wait for the screen, not the clock

Prefer `Wait /ready/` over a guessed `Sleep`. The default timeout is 15 seconds; on a timeout the
recording stops with `prec: Wait /ready/ timed out after 15.0s; the screen showed:` and the rows
it saw, so a wrong pattern shows what was there. For work with no marker, `Idle 3s` returns once
the screen has stayed still 3 seconds; its default timeout is 5 minutes and `Idle@20m 3s` gives it
20.

## Hidden stretches

For a command that only prepares the app, use the shot's `launch` (typed hidden, then `settle`).
A tape that opens with `Hide` stays hidden from the launch to its first `Show`. What is still on
the screen at `Show` is in the picture, so clear it first with `Ctrl+L`.

```text
Hide
Type@0ms "demo tick"
Enter
Wait+Screen@10s /▇{24}/
Ctrl+L
Show
Squeeze 400ms
Type "demo show"
Enter
Wait+Screen@5s /blocks/
Idle 1s
Down@200ms 2
Resize 48x14 1s
Sleep 500ms
```

## Long agent sessions

A coding agent runs for minutes between milestones. Hide the start-up banner (its account or plan
must not reach a frame), `Wait+Screen@600s` on the milestone, `Squeeze 400ms /regex/` the
spinner's status row, hide the dull stretches and end with `Idle@20m 5s`. Keep a label that stays
drawn out of the picture with a mask, and a token in `pass_env` ([`privacy.md`](privacy.md)).

## Stills

A still is taken 150 ms after the last step: end on the screen you want, with a `Wait` or `Sleep`
that lets the app finish drawing.

## Refused

`Output`, `Set`, `Require`, `Env` and `Source` stop the run, because a tape holds steps only and
the settings come from terminal.toml. So do `Copy` and `Paste`: the clipboard has no replay. A
refusal names its line.
