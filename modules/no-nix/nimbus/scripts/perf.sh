#!/usr/bin/env bash
# Repeatable performance measurement for nimbus and its Quickshell twin.
#
#   scripts/perf.sh [options] <target>...
#     target:  nimbus:<design>[:<look>]   design ∈ islands monolith tabs braun led swiss
#              qml                        bench/qml/shell.qml under quickshell
#   options:
#     --seconds N     measurement window per run           (default 20)
#     --repeat K      runs per target × mode, interleaved  (default 3)
#     --settle S      seconds after the layer appears before measuring (default 3)
#     --modes LIST    comma list of idle,anim              (default idle,anim)
#     --label L       result name -> perf/results/L.json   (default <date>-<sha8>)
#     --build         cargo build --release first (nix-shell)
#     --no-warmup     skip the discarded startup-only warm-up launch per target
#     --cooldown S    pause between runs                   (default 1)
#     --force         run even if a non-live nimbus / twin instance is running
#
# The user's live nimbus (pgrep -x nimbus without NIMBUS_BENCH) is stopped
# for the duration and restarted afterwards with its exact environment,
# argv, binary path, cwd and log files – also on failure / ctrl-c (trap), and
# on the next invocation if this script was SIGKILLed (perf/.live-state).
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(dirname "$HERE")
PERF=$ROOT/perf
PY=(python3 -I "$HERE/perf.py")
QML=$ROOT/bench/qml/shell.qml
STATE=$PERF/.live-state
DESIGNS=" islands monolith tabs braun led swiss "
DEFAULT_LD=/nix/store/aj8kinwyd2whhs3xg4b2p6j6dd9w6x56-vulkan-loader-1.4.363.0/lib

log() { printf '\033[1m[perf]\033[0m %s\n' "$*" >&2; }
[[ -n ${NIRI_SOCKET:-} && -S $NIRI_SOCKET ]] || { log "NIRI_SOCKET not set / not a socket"; exit 1; }

# ---------------------------------------------------------------- live nimbus
layer_present() { # <namespace>
  niri msg -j layers | python3 -Ic 'import json,sys; ns=sys.argv[1]; sys.exit(0 if any(l["namespace"]==ns for l in json.load(sys.stdin)) else 1)' "$1"
}

is_bench_instance() { # pid -> 0 if it is a benchmark/measurement instance
  tr '\0' '\n' <"/proc/$1/environ" 2>/dev/null | grep -q '^NIMBUS_BENCH='
}

save_live() { # pid
  local pid=$1 d=$STATE/$1 exe fd tgt
  mkdir -p "$d"
  cat "/proc/$pid/environ" >"$d/environ"
  cat "/proc/$pid/cmdline" >"$d/cmdline"
  exe=$(readlink "/proc/$pid/exe"); exe=${exe% (deleted)}
  printf '%s' "$exe" >"$d/exe"
  readlink "/proc/$pid/cwd" | tr -d '\n' >"$d/cwd"
  for fd in 1 2; do
    tgt=$(readlink "/proc/$pid/fd/$fd" 2>/dev/null || true)
    [[ -f $tgt && -w $tgt ]] || tgt=/dev/null
    printf '%s' "$tgt" >"$d/fd$fd"
  done
}

restore_live() { # never aborts; keeps the state dir if a restart fails
  [[ -d $STATE ]] || return 0
  local d envv argv exe cwd o e i pid ok
  for d in "$STATE"/*/; do
    [[ -f $d/exe ]] || continue
    mapfile -d '' envv <"$d/environ"
    mapfile -d '' argv <"$d/cmdline"
    exe=$(<"$d/exe") cwd=$(<"$d/cwd") o=$(<"$d/fd1") e=$(<"$d/fd2")
    for ((i = 0; i < 120; i++)); do [[ -x $exe ]] && break; sleep 1; done # binary mid-rebuild
    (cd "$cwd" && setsid -f env -i "${envv[@]}" "$exe" "${argv[@]:1}" </dev/null >>"$o" 2>>"$e") || true
    ok=0
    for ((i = 0; i < 100; i++)); do layer_present nimbus && { ok=1; break; }; sleep 0.05; done
    pid=$(pgrep -nx nimbus || true)
    if ((ok)); then
      log "restored live nimbus (pid $pid, $(tr '\0' '\n' <"$d/environ" | grep -E '^NIMBUS_(DESIGN|LOOK)=' | tr '\n' ' '))"
      rm -rf "$d"
    elif [[ -n $pid ]]; then
      log "WARNING: restarted live nimbus (pid $pid) but no layer surface yet; check $o"
      rm -rf "$d"
    else
      log "ERROR: could not restart live nimbus; state kept in $d (next perf.sh run retries)"
    fi
  done
  rmdir "$STATE" 2>/dev/null || true
}

# Recover from a previous SIGKILLed run: kill its orphaned measured
# instances, drop its work dir, restart the live nimbus it stopped.
for d in "$PERF"/.work/*/; do
  [[ -d $d ]] || continue
  p=$(basename "$d")
  kill -0 "$p" 2>/dev/null && [[ $(tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null) == *perf.sh* ]] && continue
  pkill -f "^${d}nimbus" 2>/dev/null || true
  rm -rf "$d"
done
if [[ -d $STATE ]] && ls "$STATE"/*/exe >/dev/null 2>&1; then
  live_now=0
  for p in $(pgrep -x nimbus || true); do is_bench_instance "$p" || live_now=1; done
  if ((live_now)); then rm -rf "$STATE"; else log "restoring live nimbus left stopped by a previous run"; restore_live; fi
fi

seconds=20 repeat=3 settle=3 modes=idle,anim label="" build=0 warmup=1 cooldown=1 force=0
targets=()
while (($#)); do
  case $1 in
    --seconds) seconds=$2; shift 2 ;;
    --repeat) repeat=$2; shift 2 ;;
    --settle) settle=$2; shift 2 ;;
    --modes) modes=$2; shift 2 ;;
    --label) label=$2; shift 2 ;;
    --cooldown) cooldown=$2; shift 2 ;;
    --build) build=1; shift ;;
    --no-warmup) warmup=0; shift ;;
    --force) force=1; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    -*) echo "unknown option $1" >&2; exit 64 ;;
    *) targets+=("$1"); shift ;;
  esac
done
((${#targets[@]})) || { sed -n '2,22p' "$0"; exit 64; }
for t in "${targets[@]}"; do
  case $t in
    qml) [[ -f $QML ]] || { echo "missing $QML" >&2; exit 64; } ;;
    nimbus:*) d=${t#nimbus:}; d=${d%%:*}
      [[ $DESIGNS == *" $d "* ]] || { echo "unknown design '$d' (nimbus silently falls back to islands)" >&2; exit 64; } ;;
    *) echo "bad target '$t'" >&2; exit 64 ;;
  esac
done
IFS=, read -ra MODES <<<"$modes"

WORK=$PERF/.work/$$
mkdir -p "$WORK/runs" "$PERF/results"
BIN=$WORK/nimbus

measured_cleanup() {
  pkill -f "^$WORK/nimbus" 2>/dev/null || true
  pkill -f '[n]imbus/bench/qml/shell.qml' 2>/dev/null || true
}
cleaned=0
cleanup() {
  set +e
  ((cleaned)) && return; cleaned=1
  measured_cleanup
  sleep 0.2
  restore_live
  rm -rf "$WORK"; rmdir "$PERF/.work" 2>/dev/null || true
}
trap cleanup EXIT
PYPID=""
on_signal() {
  log "interrupted"
  if [[ -n $PYPID ]]; then kill -TERM "$PYPID" 2>/dev/null; wait "$PYPID" 2>/dev/null; fi
  exit 130
}
trap on_signal INT TERM HUP

# ------------------------------------------------------------------- build
need_nimbus=0
for t in "${targets[@]}"; do [[ $t == nimbus:* ]] && need_nimbus=1; done
if ((build)); then
  log "building (cargo build --release)"
  (cd "$ROOT" && nix-shell -p pkg-config wayland libxkbcommon --run "cargo build --release") >&2
fi
if ((need_nimbus)); then
  [[ -x $ROOT/target/release/nimbus ]] || { log "no target/release/nimbus (use --build)"; exit 1; }
  # Snapshot so a concurrent rebuild cannot swap the binary mid-measurement.
  cp "$ROOT/target/release/nimbus" "$BIN"
fi

# ------------------------------------------------------ stop the live nimbus
LD=$DEFAULT_LD
stray=()
for p in $(pgrep -x nimbus || true); do
  if is_bench_instance "$p"; then stray+=("$p"); continue; fi
  ld=$(tr '\0' '\n' <"/proc/$p/environ" | sed -n 's/^LD_LIBRARY_PATH=//p')
  [[ -n $ld ]] && LD=$ld
  log "stopping live nimbus pid $p ($(tr '\0' '\n' <"/proc/$p/environ" | grep -E '^NIMBUS_(DESIGN|LOOK)=' | tr '\n' ' '))"
  save_live "$p"
  kill "$p"
  for ((i = 0; i < 100; i++)); do kill -0 "$p" 2>/dev/null || break; sleep 0.05; done
  kill -9 "$p" 2>/dev/null || true
done
for p in $(pgrep -f '[n]imbus/bench/qml/shell.qml' || true); do stray+=("$p"); done
if ((${#stray[@]})); then
  if ((force)); then log "WARNING: other bench instances running: ${stray[*]} (--force)"
  else log "other nimbus bench / QML twin instances are running (pids ${stray[*]}); they would skew results. Stop them or pass --force."; exit 1; fi
fi
for ((i = 0; i < 60; i++)); do layer_present nimbus || break; sleep 0.05; done

while IFS= read -r w; do log "WARNING: $w"; done < <("${PY[@]}" occlusion)

# --------------------------------------------------------------------- runs
run_one() { # target mode rep [extra...]
  local t=$1 m=$2 r=$3; shift 3
  local tag=${t//:/_}-$m-$r
  "${PY[@]}" run --target "$t" --mode "$m" --rep "$r" --seconds "$seconds" --settle "$settle" \
    --bin "$BIN" --qml "$QML" --cwd "$ROOT" --ld-library-path "$LD" \
    --log "$WORK/runs/$tag.log" --out "$WORK/runs/$tag.json" "$@" &
  PYPID=$!            # background + wait so INT/TERM traps fire immediately
  wait "$PYPID" || true
  PYPID=""
  ((r)) && RUNS+=("$WORK/runs/$tag.json")
  sleep "$cooldown"
}

nruns=$((${#targets[@]} * ${#MODES[@]} * repeat))
eta=$(python3 -Ic "print(round($nruns * ($seconds + $settle + $cooldown + 1.5) / 60, 1))")
log "$nruns runs, ~${eta} min"
if ((warmup)); then
  for t in "${targets[@]}"; do log "warm-up $t"; run_one "$t" idle 0 --startup-only; done
fi
RUNS=() n=0
for ((r = 1; r <= repeat; r++)); do # interleave targets so drift hits all equally
  for t in "${targets[@]}"; do
    for m in "${MODES[@]}"; do
      n=$((n + 1)); log "[$n/$nruns] $t $m rep $r"
      run_one "$t" "$m" "$r"
    done
  done
done

[[ -n $label ]] || label=$(date +%Y%m%d-%H%M%S)$( ((need_nimbus)) && printf -- '-%s' "$(sha256sum "$BIN" | cut -c1-8)")
"${PY[@]}" report --label "$label" --out "$PERF/results/$label.json" \
  --seconds "$seconds" --settle "$settle" --repeat "$repeat" \
  --bin "$( ((need_nimbus)) && echo "$BIN")" --qml "$QML" --root "$ROOT" \
  ${PERF_NOTE:+--note "$PERF_NOTE"} "${RUNS[@]}"
