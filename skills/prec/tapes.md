# Tapes

A tape is the keystrokes of one shot, one step a line. It holds only steps; size, theme and font
live in the recipe, so one tape renders at any size or theme.

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
  `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`; chords `Ctrl+C`, `Alt+Enter`,
  `Ctrl+Alt+<key>`.
- `Sleep <duration>`; a duration is `500ms`, `1.5s`, `2m` or bare seconds.
- `Wait /regex/` waits until the screen's last non-blank row matches; `Wait+Screen /regex/` until
  anywhere on the screen matches; `Wait+Exit` until the shell has exited.
- `Idle <duration>` waits until the screen has not changed that long (default `2s`).
- `Squeeze [<gap>] [/regex/]` shortens stretches where only a spinner or counter moves;
  `Squeeze off` stops it.
- `Resize <cols>x<rows> [<duration>]`, each side 2 to 1000; eased travel when a duration is given.
- `Hide` and `Show` cut the steps between them from the recording.
- `Screenshot` marks the cast; `# ...` is a comment.

`@<duration>` after `Type`, a key, `Wait` or `Idle`: the delay per character or key, or the
timeout for `Wait` and `Idle`. `Type@0ms "text"` types at once; `Down@200ms 3` presses Down three
times.

## Wait for the screen, not the clock

Prefer `Wait /ready/` over a guessed `Sleep`. The default timeout is 15 seconds; on a timeout the
recording stops and says what rows it saw. For work with no marker, `Idle 3s`, and `Idle@20m 3s`
for up to 20 minutes.

## Hidden setup

For a command that only prepares the app, use the shot's `launch` (typed hidden, then `settle`).
A tape that opens with `Hide` stays hidden to its first `Show`. Clear the screen (`Ctrl+L`) before
`Show` when a banner stays drawn.

## Long agent sessions

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

## Stills

A still is taken 150 ms after the last step: end on the screen you want, with a `Wait` or `Sleep`
that lets the app finish drawing.

## Refused

`Output`, `Set`, `Require`, `Env`, `Source`, `Copy` and `Paste` stop the run: settings come from
the recipe, and the clipboard has no replay.
