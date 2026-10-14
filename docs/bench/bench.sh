#!/usr/bin/env bash
set -euo pipefail
W=$(git rev-parse --show-toplevel)
B=$W/tmp/bench
cd "$W"
mkdir -p "$B/vhs"
export PREC_DIRECT=1 PITO_JOBS=off
run() { if [ -n "${CPUS:-}" ]; then taskset -c "$CPUS" nice -n 10 "$@"; else nice -n 10 "$@"; fi; }
now() { echo "$EPOCHREALTIME"; }
secs() { awk "BEGIN{printf \"%.2f\", $2 - $1}"; }
frames() { ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames,width,height,r_frame_rate -show_entries format=duration -of csv=p=0 "$1" | tr '\n' ' '; }
echo "$(lscpu | sed -n 's/Model name: *//p'), cpus ${CPUS:-all}, $(prec --version), $(vhs --version)"
for fps in 30 60 120; do
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
  s=$(now); run prec clip demo tour --app-dir examples/demo --size 720p --fps "$fps" --no-archive --out "$B/prec" >"$B/prec-log-$fps" 2>&1; e=$(now)
  m=$(ls "$B"/prec/demo/clip/*-"${fps}"fps.master.mp4)
  echo "prec fps=$fps wall=$(secs "$s" "$e") w,h,rate,frames,seconds=$(frames "$m")"
done
cast=$(ls "$B"/prec/demo/clip/*-30fps.cast)
for spec in "720p 30" "1080p 60" "2160p 30" "2160p 120" "720p 30 again"; do
  read -r size fps tag <<<"$spec"
  out=$B/render-$size-$fps${tag:+-$tag}
  s=$(now); run prec render "$cast" --size "$size" --fps "$fps" --no-archive --out "$out" >"$out.log" 2>&1; e=$(now)
  m=$(ls "$out"/demo/clip/*.master.mp4)
  echo "render $size $fps ${tag:-} wall=$(secs "$s" "$e") w,h,rate,frames,seconds=$(frames "$m") sha256=$(sha256sum "$m" | cut -c1-16)"
done
