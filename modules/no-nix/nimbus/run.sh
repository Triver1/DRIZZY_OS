#!/bin/sh
# Start nimbus (niri runs this at login: spawn-at-startup in niri-nimbus.nix).
# The design, accent and wallpaper come from ~/.config/nimbus/config.
# If nimbus crashes it is started again; a clean exit or a kill does not.
cd "$(dirname "$0")" || exit 1
bin=./target/release/nimbus
[ -x "$bin" ] || { echo "nimbus: $bin missing; run cargo build --release" >&2; exit 1; }
log="${XDG_RUNTIME_DIR:-/tmp}/nimbus.log"
tries=0
while :; do
    "$bin" >>"$log" 2>&1
    code=$?
    # 0 = normal exit; 130/143 = Ctrl+C / pkill: stay down.
    case $code in 0|130|143) exit 0 ;; esac
    tries=$((tries + 1))
    if [ "$tries" -ge 5 ]; then
        echo "nimbus: crashed $tries times, giving up" >>"$log"
        # Nothing answers polkit now: hand over to the stock agent.
        systemctl --user start --no-block polkit-gnome-agent.service 2>>"$log"
        exit 1
    fi
    echo "nimbus: exited with $code, restarting" >>"$log"
    sleep 1
done
