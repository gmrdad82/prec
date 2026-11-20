# Record, then render again

Every render keeps its recording as `<stem>.cast`, an asciicast v2 file. `prec render` replays a
cast at any size, frame rate or theme without building or running the app again. One prec version
gives the same bytes from the same cast and flags.

## Record only

```sh
prec record demo typing --app-dir examples/demo
```

This prints the shown time and the cast's path, `tmp/renders/demo/clip/demo-clip-typing.cast`,
and warns about any private data on the screen, so the take can be inspected.

## Replay

```sh
prec render tmp/renders/demo/clip/demo-clip-typing.cast --size 1080p --fps 60 --themes "Catppuccin Latte,TokyoNight"
```

The grid, session, closure and font come from the cast. `render` takes the delivery flags: `--size`,
`--derive`, `--fps`, `--poster`, `--web`, `--theme`, `--themes`, `--font`, `--frame`, `--set`
(`closure`, `closure.crossfade`, `ground`, `inset`), `--out`, `--seed`, `--keep-frames`,
`--no-encode`, `--audio-offset`, `--audio-gain` and `--allow-private`. A cast recorded for a still
renders a still, and one for a clip a clip. Each render prints its manifest's path last, one per
theme.

## When to use it

- Several sizes, rates or themes of one session.
- A 4K master later, without the app or its data.
- An app that is slow, needs credentials or cannot be run twice.
- Fixing only the delivery (poster frame, ground, closure) of a take that was right.

If the take itself was wrong (a tape step, a mask, the data), record again. Masks live in the
cast, so a masked label stays masked in every re-render. Compare hashes only within one prec
version.
