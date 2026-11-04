#!/usr/bin/env bash
set -euo pipefail
W=$(git rev-parse --show-toplevel)
B=$W/tmp/bench
cd "$W"
mkdir -p "$B/vhs"
export PREC_DIRECT=1
run() { if [ -n "${CPUS:-}" ]; then taskset -c "$CPUS" nice -n 10 "$@"; else nice -n 10 "$@"; fi; }
now() { echo "$EPOCHREALTIME"; }
secs() { awk "BEGIN{printf \"%.2f\", $2 - $1}"; }
field() { grep -o "\"$2\":[^,}]*" <<<"$1" | cut -d: -f2; }
frames() { j=$(pcod probe --json "$1"); echo "$(field "$j" width),$(field "$j" height),$(field "$j" fps),$(field "$j" frames),$(field "$j" seconds)"; }
only=${ONLY:-}
vhs_version() { if [ -z "$only" ]; then vhs --version; else echo "re-render only"; fi; }
echo "$(lscpu | sed -n 's/Model name: *//p'), cpus ${CPUS:-all}, $(prec --version), $(vhs_version)"
for fps in 30 60 120; do
  if [ -n "$only" ] && [ "$fps" != 30 ]; then continue; fi
  if [ -z "$only" ]; then
  {
    echo "Output \"$B/vhs/tour-$fps.mp4\""
    echo "Set Shell bash"
    echo "Set Width 1280"
    echo "Set Height 720"
    echo "Set FontSize 30"
    echo "Set Framerate $fps"
    echo "Set Theme \"Catppuccin Mocha\""
    echo "Set TypingSpeed 35ms"
    echo "Env PATH \"$W/examples/demo/render/terminal/bin:/usr/bin\""
    echo "Env DEMO_DATA \"$B/vhs/data\""
    cat examples/demo/render/terminal/tour.tape
  } > "$B/vhs/tour-$fps.tape"
  s=$(now); run vhs "$B/vhs/tour-$fps.tape" >"$B/vhs/log-$fps" 2>&1; e=$(now)
  echo "vhs  fps=$fps wall=$(secs "$s" "$e") w,h,rate,frames,seconds=$(frames "$B/vhs/tour-$fps.mp4")"
  fi
  s=$(now); run prec clip demo tour --app-dir examples/demo --size 720p --fps "$fps" --out "$B/prec" >"$B/prec-log-$fps" 2>&1; e=$(now)
  m=$(ls "$B"/prec/demo/clip/*-"${fps}"fps.master.mp4)
  echo "prec fps=$fps wall=$(secs "$s" "$e") w,h,rate,frames,seconds=$(frames "$m")"
done
cast=$(ls "$B"/prec/demo/clip/*-30fps.cast)
for spec in "720p 30" "1080p 60" "2160p 30" "2160p 120" "720p 30 again"; do
  read -r size fps tag <<<"$spec"
  out=$B/render-$size-$fps${tag:+-$tag}
  s=$(now); run prec render "$cast" --size "$size" --fps "$fps" --out "$out" >"$out.log" 2>&1; e=$(now)
  m=$(ls "$out"/demo/clip/*.master.mp4)
  echo "render $size $fps ${tag:-} wall=$(secs "$s" "$e") w,h,rate,frames,seconds=$(frames "$m") sha256=$(sha256sum "$m" | cut -c1-16)"
done
