# nimbus perf harness

Measures memory, CPU, GPU and startup of `nimbus` (Rust/wgpu bar) and its
Quickshell twin (`bench/qml/shell.qml`) under the same deterministic
workload. The goal is low memory and low CPU, so the harness is built to catch
small regressions: fresh process per run, interleaved repeats, a fixed
synthetic workload, and an explicit noise floor in the comparison.

## Running it

```sh
cd modules/no-nix/nimbus
scripts/perf.sh --repeat 3 --seconds 20 --label before nimbus:islands qml
# ... change code, then: nix-shell -p pkg-config wayland libxkbcommon --run "cargo build --release"
scripts/perf.sh --repeat 3 --seconds 20 --label after nimbus:islands qml
scripts/perf-compare.sh perf/results/before.json perf/results/after.json
```

`scripts/perf.sh [options] <target>...`

| option | default | meaning |
|---|---|---|
| `--seconds N` | 20 | measurement window per run |
| `--repeat K` | 3 | runs per target × mode; runs are interleaved (A B C A B C …) so drift hits every target equally |
| `--settle S` | 3 | wait after the layer surface appears before the window starts (entrance animation) |
| `--modes` | `idle,anim` | `NIMBUS_BENCH` modes to run |
| `--label L` | `<date>-<sha8>` | result file `perf/results/L.json` |
| `--build` | off | `cargo build --release` (via nix-shell) first |
| `--no-warmup` | off | skip the startup-only warm-up launch per target (warms page cache / QML disk cache) |
| `--cooldown S` | 1 | pause between runs |
| `--force` | off | run even if other bench / twin instances are already running |

Targets: `nimbus:<design>` (`islands monolith tabs braun led swiss`),
`nimbus:islands:<look>` to also set `NIMBUS_LOOK`, and `qml`.
Unknown designs are rejected (nimbus would silently fall back to islands).

Output: a markdown table on stdout (medians, then a min–max spread table and
warnings) and `perf/results/<label>.json` with every raw run. Progress and
warnings go to stderr. `python3 -I scripts/perf.py show <result.json>` prints
the table again.

`scripts/perf-compare.sh [--threshold 5] [--brief] [--all] [--strict] A.json B.json`
prints A → B per target/mode/metric with Δ and Δ%. A row is **REGRESSION**
if B is worse by more than the threshold *and* by more than the metric's
absolute noise floor (0.05 CPU-%-points, 0.05 GPU-%-points, 256 KiB of
memory, 5 ms startup, 2 wakeups/s, 0.01 ms p50, 0.05 ms p99, half a thread).
`(ranges overlap)` means the two runs' min–max ranges overlap, which usually
means noise; rerun with more repeats before believing it. `--strict` exits 2
when any regression is flagged.

### What happens to the live bar

If a daily-use nimbus is running (`pgrep -x nimbus` without `NIMBUS_BENCH`),
the script saves its exact environment, argv, binary path, cwd and stdout /
stderr log targets to `perf/.live-state/`, stops it, and restarts it
afterwards (`setsid`, detached), also on failure or ctrl-c (trap). If the
script itself is SIGKILLed, the next `perf.sh` call (even `--help`) restarts
the live bar and kills the orphaned measured instance. The nimbus binary is
copied to `perf/.work/<pid>/` first, so a concurrent `cargo build` cannot
swap it mid-run. Each measured process runs in its own session and is
always killed at the end of its run.

## Workload

`NIMBUS_BENCH=idle`: static state, nothing should redraw. `NIMBUS_BENCH=anim`:
the active workspace and title change every 350 ms (same in nimbus and the
twin), so spring animations run most of the time, at the display refresh
rate (120 Hz here). The window never contains a wall-clock minute rollover
(both bars show HH:MM): if it would, the window is shifted to start 1.5 s
after the minute (logged as `delayed_for_minute_s` in the run).

## Metrics

All per target process (all threads), sampled at the start and end of the
window unless noted.

| key | table | meaning |
|---|---|---|
| `startup_layer_ms` | layer ms | exec → our namespace (`nimbus` / `nimbus-qml`) shows up in niri's layer list. Polled every 1 ms directly on `$NIRI_SOCKET` (≈0.1 ms per request) |
| `startup_gpu_ms` | 1st GPU ms | exec → first non-zero `drm-engine-gfx` on any of the process's DRM fds. Proxy for "first frame rendered"; it may include an upload or clear just before the first frame |
| `rss_kb` `pss_kb` `uss_kb` | RSS / PSS / USS | `/proc/PID/smaps_rollup` after settle. USS = Private_Clean + Private_Dirty (what killing the process frees); PSS divides shared pages (libs, Mesa) by sharer count, so it moves when *other* processes map the same libs |
| `vmhwm_kb` | HWM | peak RSS (`VmHWM`) at window end, so startup peaks count |
| `threads` | thr | `Threads` after settle |
| `cpu_pct` | CPU % | Σ per-thread `schedstat` run time (ns) over the window, as % of one core. If the thread set changes during the window (Quickshell does), falls back to utime+stime ticks from `/proc/PID/stat` (10 ms resolution; `cpu_source` says which). `cpu_ticks_pct` is always recorded |
| `wakeups_s` | wake/s | Σ per-thread voluntary + involuntary context switches per second. Voluntary ≈ wakeups from sleep (`vol_cs_s`) |
| `gpu_pct` | GPU % | Δ `drm-engine-gfx` ns / window, from `/proc/PID/fdinfo/*` (amdgpu), deduped by `drm-client-id` |
| `gpu_all_pct` | GPU all % | the same summed over all engines (Qt also uses `compute`) |
| `gpu_mem_kb` | GPU MiB | resident `drm-memory-vram` + `drm-memory-gtt` at window end. The VRAM/GTT split (`vram_kb`, `gtt_kb`) changes run to run on this APU while the sum stays fixed, so only the sum is compared. `gpu_req_kb` = `drm-total-*` (allocated BO sizes) |
| `fps` | fps | nimbus only: frames / seconds from `NIMBUS_STATS` lines in the window. 0 in idle means no frame was drawn |
| `frame_p50_ms` `frame_p99_ms` | frm p50/p99 | nimbus only: median over the window's `NIMBUS_STATS` buckets of their per-frame CPU p50 / p99 |
| `uss_end_kb`, `uss_growth_kb` | – | USS at window end and end − settle (leak indicator; page reclaim can make it negative) |
| `niri_cpu_pct`, `niri_gpu_pct` | – | the compositor's CPU / GPU over the same window. Includes everything else on screen, so it is informational only, but anim − idle shows roughly what compositing the bar (blur) costs |
| `sys_busy_pct` | – | whole-machine CPU busy % in the window (noise indicator) |

`NIMBUS_STATS` prints one line per ≈5 s bucket, on the first frame after the
bucket expires. A bucket is used if it ends inside the window and started no
more than 2 s before it, which keeps startup frames out. A 15 s window gives
about 2 buckets and a 20 s window about 3. For the QML twin the frame
columns are empty.

## Noise and pitfalls

* **Occlusion**: a fullscreen window (or the screen being off) stops frame
  callbacks, so CPU and GPU drop to near zero and anim looks great. Before and
  after every window the harness checks niri (a window on the active workspace
  as tall as the output, an open overview, a disabled output) and warns in the
  run, the table and the JSON. It also warns when an anim run has no
  `NIMBUS_STATS` frames or zero GPU time. Do not fullscreen anything while it
  runs.
* **Other load**: cargo builds or busy terminals change CPU clocks and cache
  contention. `sys_busy_pct` is recorded and the run warns above 25 %. Compare
  results taken under similar `sys_busy_pct`.
* **Power state**: governor/EPP, platform profile, AC and amdgpu DPM level go
  into `env` in the JSON. Battery vs AC changes frame times and CPU %. Compare
  like with like.
* **GPU %** has the most noise (DPM clock changes): about ±5–10 % between runs
  at 1.5 %. **CPU %** in anim varies about ±3 % for nimbus, ±2 % for QML.
  **Memory** varies by less than 0.5 % for nimbus and about 2 % for QML (Qt's
  lazy allocations). **Startup** varies by ±5 ms.
* **PSS** depends on what else maps the same libraries, for example a running
  Quickshell or other Vulkan clients. USS does not.
* Frame p50 varies about ±4 % between runs. Use `--repeat 3` or more before
  trusting a 5 % frame-time change.
* `NIMBUS_STATS=1` is always set for nimbus runs (a Vec push per frame and a
  sort every 5 s), so it is part of what is measured.
* A minute rollover in the window is avoided (see Workload). Other clock
  sources are not.

## Files

* `scripts/perf.sh`: orchestration (live bar stop/restore, build, loops).
* `scripts/perf.py`: measurement core (`run`, `report`, `compare`, `show`,
  `occlusion`, `resummarize`). `resummarize` recomputes derived metrics and
  summaries of an old result file in place when metric definitions change.
* `scripts/perf-compare.sh`: wrapper for `perf.py compare`.
* `perf/results/*.json`: results (`baseline.json` is the reference).
* `perf/.work/`, `perf/.live-state/`: transient, git-ignored.
