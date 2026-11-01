#!/usr/bin/env python3
"""Measurement core for scripts/perf.sh (nimbus vs. its Quickshell twin).

Subcommands (normally driven by perf.sh / perf-compare.sh):
  run      launch one fresh target process, measure it, write one run JSON
  report   aggregate run JSONs -> perf/results/<label>.json + markdown table
  compare  delta table between two result JSONs
  occlusion  print warnings if the bar is likely occluded right now

Only stdlib; reads /proc and talks to niri over $NIRI_SOCKET directly
(≈0.1 ms per request, vs ≈14 ms for spawning `niri msg`).
"""
import argparse
import hashlib
import json
import os
import platform
import re
import signal
import socket
import statistics
import subprocess
import sys
import threading
import time

CLK_TCK = os.sysconf("SC_CLK_TCK")
STATS_RE = re.compile(
    r"nimbus stats: (\d+) frames in ([\d.]+)s, cpu/frame p50 ([\d.]+)ms p99 ([\d.]+)ms max ([\d.]+)ms"
)

# key, column label, unit kind, lower-is-better (None = informational), absolute noise floor
METRICS = [
    ("startup_layer_ms", "layer ms", "ms", True, 5.0),
    ("startup_gpu_ms", "1st GPU ms", "ms", True, 5.0),
    ("rss_kb", "RSS MiB", "kb", True, 256),
    ("pss_kb", "PSS MiB", "kb", True, 256),
    ("uss_kb", "USS MiB", "kb", True, 256),
    ("vmhwm_kb", "HWM MiB", "kb", True, 256),
    ("threads", "thr", "int", True, 0.5),
    ("cpu_pct", "CPU %", "pct", True, 0.05),
    ("wakeups_s", "wake/s", "rate", True, 2.0),
    ("gpu_pct", "GPU %", "pct", True, 0.05),
    ("gpu_all_pct", "GPU all %", "pct", True, 0.05),
    ("gpu_mem_kb", "GPU MiB", "kb", True, 256),
    ("fps", "fps", "rate", None, 0),
    ("frame_p50_ms", "frm p50 ms", "ms3", True, 0.01),
    ("frame_p99_ms", "frm p99 ms", "ms3", True, 0.05),
    # JSON / compare only (not in the main table)
    ("gpu_req_kb", "GPU requested MiB", "kb", True, 256),
    ("vram_kb", "VRAM MiB", "kb", None, 0),  # VRAM/GTT split migrates; use the sum
    ("gtt_kb", "GTT MiB", "kb", None, 0),
    ("uss_end_kb", "USS end MiB", "kb", None, 0),  # subject to page reclaim
    ("uss_growth_kb", "USS growth KiB", "int", None, 0),
    ("cpu_ticks_pct", "CPU % (ticks)", "pct", None, 0),
    ("vol_cs_s", "vol cs/s", "rate", None, 0),
    ("niri_cpu_pct", "niri CPU %", "pct", None, 0),
    ("niri_gpu_pct", "niri GPU %", "pct", None, 0),
    ("sys_busy_pct", "system busy %", "pct", None, 0),
]
TABLE_KEYS = [m[0] for m in METRICS[:15]]
MDEF = {m[0]: m for m in METRICS}


# ----------------------------------------------------------------- niri IPC
def niri(req):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(2)
    try:
        s.connect(os.environ["NIRI_SOCKET"])
        s.sendall((json.dumps(req) + "\n").encode())
        buf = b""
        while not buf.endswith(b"\n"):
            c = s.recv(1 << 16)
            if not c:
                break
            buf += c
    finally:
        s.close()
    r = json.loads(buf)
    if "Ok" not in r:
        raise RuntimeError(f"niri {req}: {r}")
    return next(iter(r["Ok"].values()))


def layer_count(ns):
    return sum(1 for l in niri("Layers") if l.get("namespace") == ns)


def occlusion_warnings():
    """Heuristics for "the bar is not getting frame callbacks"."""
    w = []
    try:
        outputs = niri("Outputs")
        wss = niri("Workspaces")
        wins = niri("Windows")
        if niri("OverviewState").get("is_open"):
            w.append("niri overview is open (bar rendering/visibility differs)")
    except Exception as e:  # noqa: BLE001
        return [f"occlusion check failed: {e}"]
    for name, o in outputs.items():
        lg = o.get("logical")
        if not lg or o.get("current_mode") is None:
            w.append(f"output {name} is disabled")
            continue
        active = {ws["id"] for ws in wss if ws.get("output") == name and ws.get("is_active")}
        for win in wins:
            if win.get("workspace_id") not in active:
                continue
            ww, wh = win.get("layout", {}).get("window_size") or (0, 0)
            # Normal tiles respect the bar's exclusive zone, so a window as tall
            # as the whole output is fullscreen (or windowed-fullscreen).
            if wh >= lg["height"] - 1 and ww >= lg["width"] * 0.5:
                w.append(
                    f"window {win.get('id')} ({win.get('app_id')}) looks fullscreen on {name} "
                    f"({ww}x{wh}); it may occlude the bar and stop frame callbacks"
                )
    return w


# --------------------------------------------------------------- /proc reads
def _kv(path):
    d = {}
    with open(path) as f:
        for line in f:
            k, _, v = line.partition(":")
            d[k.strip()] = v.strip()
    return d


def _num(v):
    try:
        return int(v.split()[0])
    except (ValueError, IndexError, AttributeError):
        return 0


def stat_ticks(pid):
    with open(f"/proc/{pid}/stat") as f:
        s = f.read()
    rest = s[s.rindex(")") + 2 :].split()
    return int(rest[11]) + int(rest[12])  # utime + stime, includes dead threads


def tasks(pid):
    try:
        return os.listdir(f"/proc/{pid}/task")
    except FileNotFoundError:
        return []


def sched_ns(pid):
    out = {}
    for t in tasks(pid):
        try:
            with open(f"/proc/{pid}/task/{t}/schedstat") as f:
                out[t] = int(f.read().split()[0])
        except (FileNotFoundError, ProcessLookupError, ValueError):
            pass
    return out


def ctxt(pid):
    """Per-thread (voluntary, nonvoluntary) context switches."""
    out = {}
    for t in tasks(pid):
        try:
            d = _kv(f"/proc/{pid}/task/{t}/status")
        except (FileNotFoundError, ProcessLookupError):
            continue
        out[t] = (_num(d.get("voluntary_ctxt_switches", "0")), _num(d.get("nonvoluntary_ctxt_switches", "0")))
    return out


def mem(pid):
    st = _kv(f"/proc/{pid}/status")
    sm = _kv(f"/proc/{pid}/smaps_rollup")
    return {
        "rss_kb": _num(sm.get("Rss")),
        "pss_kb": _num(sm.get("Pss")),
        "uss_kb": _num(sm.get("Private_Clean")) + _num(sm.get("Private_Dirty")),
        "pss_anon_kb": _num(sm.get("Pss_Anon")),
        "pss_file_kb": _num(sm.get("Pss_File")),
        "pss_shmem_kb": _num(sm.get("Pss_Shmem")),
        "rss_anon_kb": _num(st.get("RssAnon")),
        "rss_file_kb": _num(st.get("RssFile")),
        "rss_shmem_kb": _num(st.get("RssShmem")),
        "vmhwm_kb": _num(st.get("VmHWM")),
        "threads": _num(st.get("Threads")),
    }


def drm_fds(pid):
    fds = []
    try:
        for fd in os.listdir(f"/proc/{pid}/fd"):
            try:
                if os.readlink(f"/proc/{pid}/fd/{fd}").startswith("/dev/dri/"):
                    fds.append(fd)
            except OSError:
                pass
    except FileNotFoundError:
        pass
    return fds


def drm(pid, fds=None):
    """Per-process amdgpu usage, summed over DRM clients (deduped by client id)."""
    clients = {}
    for fd in fds if fds is not None else drm_fds(pid):
        try:
            d = _kv(f"/proc/{pid}/fdinfo/{fd}")
        except (FileNotFoundError, ProcessLookupError):
            continue
        if "drm-driver" not in d or "drm-client-id" not in d:
            continue
        clients[d["drm-client-id"]] = d
    tot = {"clients": len(clients), "engine_ns": {}, "vram_kb": 0, "gtt_kb": 0, "vram_total_kb": 0, "gtt_total_kb": 0}
    for d in clients.values():
        for k, v in d.items():
            if k.startswith("drm-engine-") and not k.startswith("drm-engine-capacity"):
                e = k[len("drm-engine-") :]
                tot["engine_ns"][e] = tot["engine_ns"].get(e, 0) + _num(v)
        tot["vram_kb"] += _num(d.get("drm-memory-vram", d.get("drm-resident-vram", "0")))
        tot["gtt_kb"] += _num(d.get("drm-memory-gtt", d.get("drm-resident-gtt", "0")))
        tot["vram_total_kb"] += _num(d.get("drm-total-vram", "0"))
        tot["gtt_total_kb"] += _num(d.get("drm-total-gtt", "0"))
    return tot


def sys_cpu():
    with open("/proc/stat") as f:
        v = [int(x) for x in f.readline().split()[1:]]
    idle = v[3] + v[4]
    return sum(v[:8]) - idle, sum(v[:8])


def niri_pid():
    sock = os.environ.get("NIRI_SOCKET", "")
    m = re.search(r"\.(\d+)\.sock$", sock)
    if m:
        try:
            with open(f"/proc/{m.group(1)}/comm") as f:
                if f.read().strip() == "niri":
                    return int(m.group(1))
        except FileNotFoundError:
            pass
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                with open(f"/proc/{p}/comm") as f:
                    if f.read().strip() == "niri":
                        return int(p)
            except OSError:
                pass
    return None


def counters(pid, npid):
    c = {
        "t": time.monotonic(),
        "ticks": stat_ticks(pid),
        "sched": sched_ns(pid),
        "ctxt": ctxt(pid),
        "drm": drm(pid),
        "sys": sys_cpu(),
    }
    if npid:
        try:
            c["niri_sched"] = sum(sched_ns(npid).values())
            c["niri_drm"] = drm(npid)
        except Exception:  # noqa: BLE001
            pass
    return c


def children(pid):
    out = []
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                with open(f"/proc/{p}/stat") as f:
                    s = f.read()
                if int(s[s.rindex(")") + 2 :].split()[1]) == pid:
                    out.append(int(p))
            except (OSError, ValueError):
                pass
    return out


# ------------------------------------------------------------------- run one
def cmd_run(a):
    rec = {
        "target": a.target, "mode": a.mode, "rep": a.rep, "ok": False, "error": None,
        "seconds": a.seconds, "settle": a.settle, "warnings": [],
    }
    kind, _, rest = a.target.partition(":")
    env = {k: v for k, v in os.environ.items() if not k.startswith("NIMBUS_")}
    env["NIMBUS_BENCH"] = a.mode
    if kind == "nimbus":
        design, _, look = rest.partition(":")
        argv, ns = [a.bin], "nimbus"
        env["NIMBUS_DESIGN"] = design
        env["NIMBUS_STATS"] = "1"
        if look:
            env["NIMBUS_LOOK"] = look
        if a.ld_library_path:
            env["LD_LIBRARY_PATH"] = a.ld_library_path
    elif kind == "qml":
        argv, ns = ["quickshell", "-p", a.qml], "nimbus-qml"
    else:
        rec["error"] = f"unknown target {a.target}"
        return finish(a, rec)

    rec["warnings"] += occlusion_warnings()
    if layer_count(ns):
        rec["error"] = f"a '{ns}' layer surface already exists; another instance is running"
        return finish(a, rec)
    npid = niri_pid()

    log = open(a.log, "w") if a.log else None
    lines = []

    t0 = time.monotonic()
    p = subprocess.Popen(argv, env=env, cwd=a.cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, start_new_session=True)

    def reader():
        for raw in p.stdout:
            line = raw.decode(errors="replace").rstrip("\n")
            lines.append((time.monotonic(), line))
            if log:
                log.write(f"{time.monotonic() - t0:9.3f} {line}\n")
                log.flush()

    th = threading.Thread(target=reader, daemon=True)
    th.start()
    rec["pid"] = p.pid
    try:
        t_layer = t_gpu = None
        fds = []
        deadline = t0 + a.startup_timeout
        while True:
            now = time.monotonic()
            if p.poll() is not None:
                raise RuntimeError(f"process exited during startup (code {p.returncode})")
            if t_layer is None and layer_count(ns):
                t_layer = time.monotonic()
            if t_gpu is None:
                fds = drm_fds(p.pid)  # rescan: adapter probing opens/closes fds
                if fds and drm(p.pid, fds)["engine_ns"].get("gfx", 0) > 0:
                    t_gpu = time.monotonic()
            if t_layer is not None and (t_gpu is not None or now > t_layer + 2.0):
                break
            if now > deadline:
                raise RuntimeError(f"no '{ns}' layer surface within {a.startup_timeout}s")
            time.sleep(0.001)
        rec["startup_layer_ms"] = round((t_layer - t0) * 1000, 1)
        rec["startup_gpu_ms"] = round((t_gpu - t0) * 1000, 1) if t_gpu else None
        if a.startup_only:
            rec["ok"] = True
            return finish(a, rec)

        time.sleep(max(0.0, t_layer + a.settle - time.monotonic()))
        # Both bars show HH:MM; a minute rollover inside the window triggers a
        # redraw/animation that is not part of the workload. Shift the window
        # to start just after the rollover instead (costs ~W²/120 s on average).
        into = time.time() % 60
        if a.seconds < 55 and into + a.seconds > 59.0:
            rec["delayed_for_minute_s"] = round(60 - into + 1.5, 2)
            time.sleep(60 - into + 1.5)
        if p.poll() is not None:
            raise RuntimeError(f"process exited during settle (code {p.returncode})")
        m0 = mem(p.pid)
        c0 = counters(p.pid, npid)
        time.sleep(a.seconds)
        if p.poll() is not None:
            raise RuntimeError(f"process exited during window (code {p.returncode})")
        c1 = counters(p.pid, npid)
        m1 = mem(p.pid)
        kids = children(p.pid)
        if kids:
            rec["warnings"].append(f"target has child processes {kids} (not included in CPU)")

        dt = c1["t"] - c0["t"]
        rec["window_s"] = round(dt, 3)
        rec.update({k: v for k, v in m0.items()})
        rec["vmhwm_kb"] = m1["vmhwm_kb"]
        rec["uss_end_kb"] = m1["uss_kb"]
        rec["pss_end_kb"] = m1["pss_kb"]
        rec["rss_end_kb"] = m1["rss_kb"]
        rec["uss_growth_kb"] = m1["uss_kb"] - m0["uss_kb"]
        rec["threads_end"] = m1["threads"]

        rec["cpu_ticks_pct"] = round((c1["ticks"] - c0["ticks"]) / CLK_TCK / dt * 100, 3)
        s0, s1 = c0["sched"], c1["sched"]
        if set(s0) == set(s1):
            rec["cpu_pct"] = round(sum(s1[t] - s0[t] for t in s1) / 1e9 / dt * 100, 3)
            rec["cpu_source"] = "schedstat"
        else:
            rec["cpu_pct"] = rec["cpu_ticks_pct"]
            rec["cpu_source"] = "stat-ticks (thread set changed)"
        # per thread, so threads exiting mid-window cannot make it negative
        x0, x1 = c0["ctxt"], c1["ctxt"]
        dv = sum(v - x0.get(t, (0, 0))[0] for t, (v, _) in x1.items())
        di = sum(i - x0.get(t, (0, 0))[1] for t, (_, i) in x1.items())
        rec["wakeups_s"] = round((dv + di) / dt, 2)
        rec["vol_cs_s"] = round(dv / dt, 2)

        g0, g1 = c0["drm"], c1["drm"]
        rec["gpu_clients"] = g1["clients"]
        rec["gpu_pct"] = round((g1["engine_ns"].get("gfx", 0) - g0["engine_ns"].get("gfx", 0)) / 1e9 / dt * 100, 3)
        rec["gpu_engines_pct"] = {
            e: round((v - g0["engine_ns"].get(e, 0)) / 1e9 / dt * 100, 3) for e, v in g1["engine_ns"].items()
        }
        rec["gpu_all_pct"] = round(sum(rec["gpu_engines_pct"].values()), 3)
        for k in ("vram_kb", "gtt_kb", "vram_total_kb", "gtt_total_kb"):
            rec[k] = g1[k]
        derive(rec)

        b0, t0s = c0["sys"]
        b1, t1s = c1["sys"]
        rec["sys_busy_pct"] = round((b1 - b0) / max(1, t1s - t0s) * 100, 2)
        with open("/proc/loadavg") as f:
            rec["loadavg"] = f.read().split()[0]
        if "niri_sched" in c0 and "niri_sched" in c1:
            rec["niri_cpu_pct"] = round((c1["niri_sched"] - c0["niri_sched"]) / 1e9 / dt * 100, 3)
            rec["niri_gpu_pct"] = round(
                (c1["niri_drm"]["engine_ns"].get("gfx", 0) - c0["niri_drm"]["engine_ns"].get("gfx", 0)) / 1e9 / dt * 100, 3
            )

        # NIMBUS_STATS: one line per ~5 s bucket, printed on the first frame
        # after the bucket expires. Use buckets that ended inside the window
        # and did not start more than 2 s before it (keeps startup out).
        stats = []
        for t, line in lines:
            m = STATS_RE.search(line)
            if not m:
                continue
            frames, secs, p50, p99, mx = int(m[1]), float(m[2]), float(m[3]), float(m[4]), float(m[5])
            if c0["t"] < t <= c1["t"] + 0.05 and t - secs >= c0["t"] - 2.0:
                stats.append({"frames": frames, "secs": secs, "p50": p50, "p99": p99, "max": mx})
        rec["stats_lines"] = stats
        if kind == "nimbus":
            if stats:
                rec["fps"] = round(sum(s["frames"] for s in stats) / sum(s["secs"] for s in stats), 2)
                rec["frame_p50_ms"] = round(statistics.median(s["p50"] for s in stats), 4)
                rec["frame_p99_ms"] = round(statistics.median(s["p99"] for s in stats), 4)
            else:
                rec["fps"] = 0.0  # no frame drew a stats line in the window
                if a.mode == "anim":
                    rec["warnings"].append("anim mode but no NIMBUS_STATS lines: no frames (bar occluded / output off?)")
        if a.mode == "anim" and rec["gpu_pct"] == 0:
            rec["warnings"].append("anim mode but zero GPU gfx time: bar probably not rendering (occluded?)")
        rec["warnings"] += [w for w in occlusion_warnings() if w not in rec["warnings"]]
        if rec["sys_busy_pct"] > 25:
            rec["warnings"].append(f"system was busy ({rec['sys_busy_pct']}% of all cores) during the window")
        rec["ok"] = True
    except Exception as e:  # noqa: BLE001
        rec["error"] = str(e)
        rec["log_tail"] = [l for _, l in lines[-15:]]
    finally:
        stop(p, ns)
        if log:
            log.close()
    return finish(a, rec)


def derive(rec):
    """Derived metrics (also applied by `resummarize` to older results)."""
    if "vram_kb" in rec and "gtt_kb" in rec:
        # amdgpu (APU) migrates BOs between VRAM and GTT run to run; the sum is stable.
        rec["gpu_mem_kb"] = rec["vram_kb"] + rec["gtt_kb"]
    if "vram_total_kb" in rec and "gtt_total_kb" in rec:
        rec["gpu_req_kb"] = rec["vram_total_kb"] + rec["gtt_total_kb"]


def stop(p, ns):
    if p.poll() is None:
        try:
            os.killpg(p.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            p.wait(timeout=2)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            p.wait()
    end = time.monotonic() + 3
    while time.monotonic() < end:
        try:
            if not layer_count(ns):
                break
        except Exception:  # noqa: BLE001
            break
        time.sleep(0.01)


def finish(a, rec):
    with open(a.out, "w") as f:
        json.dump(rec, f, indent=1)
    if not rec["ok"]:
        print(f"  !! {rec['target']} {rec['mode']}: {rec['error']}", file=sys.stderr)
        for l in rec.get("log_tail", []):
            print(f"     | {l}", file=sys.stderr)
    for w in rec["warnings"]:
        print(f"  warning: {w}", file=sys.stderr)
    return 0 if rec["ok"] else 1


# --------------------------------------------------------------- formatting
def fmt(key, v):
    if v is None:
        return "–"
    kind = MDEF[key][2] if key in MDEF else "rate"
    if kind == "kb":
        return f"{v / 1024:.2f}"
    if kind == "ms":
        return f"{v:.0f}"
    if kind == "ms3":
        return f"{v:.3f}"
    if kind == "pct":
        return f"{v:.2f}"
    if kind == "int":
        return f"{v:.0f}" if float(v).is_integer() else f"{v:.1f}"
    return f"{v:.1f}"


def summarize(vals):
    vals = [v for v in vals if v is not None]
    if not vals:
        return None
    return {"median": statistics.median(vals), "min": min(vals), "max": max(vals), "n": len(vals)}


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def read1(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return None


def env_info(a):
    info = {
        "kernel": platform.release(),
        "cpu": next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), None),
        "governor": read1("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "epp": read1("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference"),
        "platform_profile": read1("/sys/firmware/acpi/platform_profile"),
        "ac_online": None,
        "gpu_dpm": None,
    }
    try:
        info["niri"] = niri("Version")
    except Exception:  # noqa: BLE001
        info["niri"] = None
    ps = "/sys/class/power_supply"
    for d in os.listdir(ps) if os.path.isdir(ps) else []:
        if read1(f"{ps}/{d}/type") == "Mains":
            info["ac_online"] = read1(f"{ps}/{d}/online") == "1"
    for c in sorted(os.listdir("/sys/class/drm")):
        v = read1(f"/sys/class/drm/{c}/device/power_dpm_force_performance_level")
        if v:
            info["gpu_dpm"] = v
            break
    if a.bin and os.path.exists(a.bin):
        info["nimbus_sha256"] = sha256(a.bin)
        info["nimbus_mtime"] = time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(os.path.getmtime(a.bin)))
    if a.qml and os.path.exists(a.qml):
        h = hashlib.sha256()
        d = os.path.dirname(a.qml)
        for n in sorted(os.listdir(d)):
            if n.endswith(".qml"):
                h.update(n.encode())
                h.update(open(os.path.join(d, n), "rb").read())
        info["qml_sha256"] = h.hexdigest()
    try:
        root = a.root
        info["git_head"] = subprocess.run(["git", "-C", root, "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
        dirty = subprocess.run(["git", "-C", root, "status", "--porcelain", "--", "src", "Cargo.toml", "Cargo.lock", "bench"],
                               capture_output=True, text=True).stdout.strip()
        info["git_src_dirty"] = bool(dirty)
    except Exception:  # noqa: BLE001
        pass
    try:
        info["quickshell"] = subprocess.run(["quickshell", "--version"], capture_output=True, text=True, timeout=5).stdout.strip()
    except Exception:  # noqa: BLE001
        info["quickshell"] = None
    return info


def build_results(runs, order):
    results = {}
    warnings = []
    for t, m in order:
        rs = [r for r in runs if r["target"] == t and r["mode"] == m]
        for r in rs:
            derive(r)
        ok = [r for r in rs if r["ok"]]
        summ = {key: summarize([r.get(key) for r in ok]) for key in MDEF}
        summ = {k: v for k, v in summ.items() if v is not None}
        results.setdefault(t, {})[m] = {"summary": summ, "runs": rs, "ok": len(ok), "failed": len(rs) - len(ok)}
        for r in rs:
            for w in r.get("warnings", []):
                warnings.append(f"{t} {m} rep{r['rep']}: {w}")
            if not r["ok"]:
                warnings.append(f"{t} {m} rep{r['rep']}: FAILED: {r['error']}")
    return results, warnings


def cmd_resummarize(a):
    """Recompute derived metrics + summaries of a result file in place."""
    res = json.load(open(a.result))
    runs = [r for modes in res["results"].values() for d in modes.values() for r in d["runs"]]
    order = [(t, m) for t, modes in res["results"].items() for m in modes]
    res["results"], res["warnings"] = build_results(runs, order)
    with open(a.result, "w") as f:
        json.dump(res, f, indent=1)
    print_table(res)
    return 0


def cmd_report(a):
    runs = []
    for p in a.runs:
        with open(p) as f:
            runs.append(json.load(f))
    order = []
    for r in runs:
        k = (r["target"], r["mode"])
        if k not in order:
            order.append(k)
    results, warnings = build_results(runs, order)
    out = {
        "label": a.label,
        "created": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "params": {"seconds": a.seconds, "repeat": a.repeat, "settle": a.settle},
        "env": env_info(a),
        "notes": a.note or [],
        "warnings": warnings,
        "results": results,
    }
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    with open(a.out, "w") as f:
        json.dump(out, f, indent=1)
    print_table(out)
    print(f"\nresults: {a.out}")
    return 0


def print_table(res):
    p = res["params"]
    e = res["env"]
    print(f"## nimbus perf: {res['label']}  ({res['created']})\n")
    print(f"window {p['seconds']}s, settle {p['settle']}s, repeat {p['repeat']}; "
          f"nimbus {str(e.get('nimbus_sha256', ''))[:12]} (git {e.get('git_head')}{' dirty' if e.get('git_src_dirty') else ''}), "
          f"governor {e.get('governor')}/{e.get('epp')}, AC {e.get('ac_online')}\n")
    print("Medians. Memory in MiB; CPU % = % of one core; GPU % = gfx-engine busy; wake/s = context switches/s (all threads).\n")
    hdr = ["target", "mode"] + [MDEF[k][1] for k in TABLE_KEYS]
    print("| " + " | ".join(hdr) + " |")
    print("|" + "|".join(["---"] * 2 + ["---:"] * len(TABLE_KEYS)) + "|")
    for t, modes in res["results"].items():
        for m, d in modes.items():
            s = d["summary"]
            row = [t, m + ("" if not d["failed"] else f" ({d['failed']} failed)")]
            row += [fmt(k, s[k]["median"]) if k in s else "–" for k in TABLE_KEYS]
            print("| " + " | ".join(row) + " |")
    print("\nSpread across repeats (min–max):\n")
    keys = ["startup_layer_ms", "pss_kb", "uss_kb", "cpu_pct", "wakeups_s", "gpu_pct", "gpu_mem_kb", "frame_p50_ms", "niri_cpu_pct", "sys_busy_pct"]
    print("| target | mode | " + " | ".join(MDEF[k][1] for k in keys) + " |")
    print("|---|---|" + "|".join(["---:"] * len(keys)) + "|")
    for t, modes in res["results"].items():
        for m, d in modes.items():
            s = d["summary"]
            row = [t, m]
            for k in keys:
                row.append(f"{fmt(k, s[k]['min'])}–{fmt(k, s[k]['max'])}" if k in s else "–")
            print("| " + " | ".join(row) + " |")
    if res.get("notes"):
        print("\nNotes:")
        for n in res["notes"]:
            print(f"- {n}")
    if res["warnings"]:
        print("\nWarnings:")
        for w in res["warnings"]:
            print(f"- {w}")


def cmd_compare(a):
    A = json.load(open(a.a))
    B = json.load(open(a.b))
    thr = a.threshold
    regressions = 0
    print(f"## {A['label']} → {B['label']}  (regression = worse by >{thr:g}% and above the noise floor)\n")
    for side, r in (("A", A), ("B", B)):
        e = r["env"]
        print(f"- {side}: {r['label']}  nimbus {str(e.get('nimbus_sha256', ''))[:12]}  window {r['params']['seconds']}s ×{r['params']['repeat']}")
    keys = [k for k in MDEF]
    print("\n| target | mode | metric | A | B | Δ | Δ% | |")
    print("|---|---|---|---:|---:|---:|---:|---|")
    for t, modes in B["results"].items():
        for m, d in modes.items():
            sa = A["results"].get(t, {}).get(m, {}).get("summary")
            if not sa:
                print(f"| {t} | {m} | (not in A) | | | | | |")
                continue
            sb = d["summary"]
            for k in keys:
                if k not in sa or k not in sb:
                    continue
                va, vb = sa[k]["median"], sb[k]["median"]
                delta = vb - va
                pct = (delta / va * 100) if va else (0.0 if delta == 0 else float("inf"))
                _, label, kind, lower, floor = MDEF[k]
                # noise: the two runs' ranges overlap
                overlap = sb[k]["min"] <= sa[k]["max"] and sa[k]["min"] <= sb[k]["max"]
                flag = ""
                if lower is not None and abs(delta) > floor and abs(pct) > thr:
                    worse = delta > 0 if lower else delta < 0
                    flag = "REGRESSION" if worse else "better"
                    if overlap:
                        flag += " (ranges overlap)"
                    if worse:
                        regressions += 1
                if not a.all and not flag and k not in TABLE_KEYS:
                    continue
                if a.brief and not flag:
                    continue
                d_s = fmt(k, abs(delta))
                if d_s.strip("0.") and delta:
                    d_s = ("+" if delta > 0 else "-") + d_s
                pct_s = "∞" if pct == float("inf") else f"{pct:+.1f}%"
                unit = ""
                print(f"| {t} | {m} | {label}{unit} | {fmt(k, va)} | {fmt(k, vb)} | {d_s} | {pct_s} | {'**' + flag + '**' if flag.startswith('REG') else flag} |")
    print(f"\n{regressions} regression(s).")
    return 2 if (a.strict and regressions) else 0


def _bail(signum, _frame):
    sys.exit(128 + signum)  # unwinds through cmd_run's finally -> stop()


def main():
    signal.signal(signal.SIGTERM, _bail)
    signal.signal(signal.SIGINT, _bail)
    signal.signal(signal.SIGHUP, _bail)
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--target", required=True)
    r.add_argument("--mode", required=True, choices=["idle", "anim"])
    r.add_argument("--rep", type=int, default=1)
    r.add_argument("--seconds", type=float, default=20)
    r.add_argument("--settle", type=float, default=3)
    r.add_argument("--startup-timeout", type=float, default=10)
    r.add_argument("--startup-only", action="store_true")
    r.add_argument("--bin")
    r.add_argument("--qml")
    r.add_argument("--cwd")
    r.add_argument("--ld-library-path")
    r.add_argument("--log")
    r.add_argument("--out", required=True)
    rp = sub.add_parser("report")
    rp.add_argument("--label", required=True)
    rp.add_argument("--out", required=True)
    rp.add_argument("--seconds", type=float)
    rp.add_argument("--settle", type=float)
    rp.add_argument("--repeat", type=int)
    rp.add_argument("--bin")
    rp.add_argument("--qml")
    rp.add_argument("--root")
    rp.add_argument("--note", action="append")
    rp.add_argument("runs", nargs="+")
    c = sub.add_parser("compare")
    c.add_argument("a")
    c.add_argument("b")
    c.add_argument("--threshold", type=float, default=5.0)
    c.add_argument("--all", action="store_true", help="also show informational metrics")
    c.add_argument("--strict", action="store_true", help="exit 2 if any regression")
    c.add_argument("--brief", action="store_true", help="only rows flagged better/regression")
    sub.add_parser("occlusion")
    sub.add_parser("show").add_argument("result")
    sub.add_parser("resummarize").add_argument("result")
    a = ap.parse_args()
    if a.cmd == "run":
        return cmd_run(a)
    if a.cmd == "report":
        return cmd_report(a)
    if a.cmd == "compare":
        return cmd_compare(a)
    if a.cmd == "resummarize":
        return cmd_resummarize(a)
    if a.cmd == "show":
        print_table(json.load(open(a.result)))
        return 0
    if a.cmd == "occlusion":
        for w in occlusion_warnings():
            print(w)
        return 0


if __name__ == "__main__":
    sys.exit(main())
