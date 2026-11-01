//! Process monitor: live CPU and memory graphs over the last minute, and a
//! sortable, filterable process table with SIGTERM / SIGKILL.
//!
//! Sampling runs on a worker thread that exists only while the window is
//! open. It reads /proc once a second (stat, meminfo, loadavg, uptime and
//! every user process's stat; a process's cmdline only the first time it is
//! seen), derives CPU% from deltas, rasterises both graphs at the size they
//! are drawn and hands everything over as one `Fact::Custom(Snapshot)`.
//!
//! Motion: the window grows out of the screen centre; graphs scroll by
//! exactly one sample per snapshot on a critically damped spring that lands
//! well inside the sample period (and snaps once it is sub-pixel), then sit
//! still; table rows are keyed by pid and slide to their new rank on
//! springs (the climbing row passes over the falling one like a card), new
//! ones fade/scale in, exited ones collapse, rows that drop out of view slide
//! off the bottom. Ranks use a smoothed CPU value with hysteresis and hold
//! still while the pointer is over the table or a row is selected. Numbers
//! cross-fade in place. Between snapshots the window draws nothing at all.
//!
//! Look: Prism Ink. The window is flat matte ink edged by the prism
//! hairline; the graph, memory and table sections are open (no cards),
//! split by hairline dividers. Graphs, bars and the sort / selection
//! accents are iridescent (the accent gradient under another accent);
//! nothing glows.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::Write as _,
    io::Read,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use glyphon::FontSystem;
use resvg::tiny_skia;

use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Font, Rect, TextCache},
    gfx::{DrawList, IconDraw, ImageData, ImageDraw, TextDraw},
    icons::IconSet,
    paint,
    services::Fact,
    settings::Action,
    theme::{self, Rgba},
    window::{Chrome, Ctx, Cursor, KeyInput, Placement, WinScene, keys},
};

// ── geometry ────────────────────────────────────────────────────────────

/// Window size (logical) and the shadow margin around it on the surface.
const FW: f32 = 880.0;
const FH: f32 = 580.0;
const M: f32 = 40.0;
const SW: f32 = FW + 2.0 * M;
const SH: f32 = FH + 2.0 * M;
const PAD: f32 = 18.0;
const GAP: f32 = 12.0;

const CARD_Y: f32 = 58.0;
const CARD_H: f32 = 146.0;
const CPU_W: f32 = 500.0;
const MEM_W: f32 = FW - 2.0 * PAD - CPU_W - GAP;
const GRAPH_Y: f32 = 80.0;
const GRAPH_H: f32 = 54.0;
const GRAPH_IN: f32 = 12.0;

const TABLE_Y: f32 = 216.0;
const TABLE_H: f32 = 302.0;
const ROWS: usize = 8;
const ROW_H: f32 = 32.0;
const ROWS_TOP: f32 = 40.0;
const FOOT_Y: f32 = TABLE_Y + TABLE_H + 10.0;
const FOOT_H: f32 = 34.0;

// Table columns, relative to the table card.
const NAME_X: f32 = 46.0;
const PID_R: f32 = 452.0;
const CPU_BAR: f32 = 500.0;
const CPU_R: f32 = 640.0;
const MEM_BAR: f32 = 684.0;
const MEM_R: f32 = FW - 2.0 * PAD - 16.0;
const BAR_W: f32 = 64.0;

/// Samples visible in a graph (one per second).
const HIST: usize = 60;
const ARM_TIMEOUT: f32 = 3.0;
const STATUS_TIME: f32 = 3.2;
/// A signalled row stays dimmed this long if the process ignores it.
const ENDED_TIME: f32 = 4.0;
/// Ranks stay put this long after the pointer leaves the table.
const HOLD_AFTER: f32 = 1.0;
/// CPU smoothing for ranking (per sample) and the hysteresis of the rank
/// key: it only follows the smoothed value once that moved clearly.
const EMA: f32 = 0.5;
/// App icons in the table (logical px).
const ICON: f32 = 20.0;

// ── look ────────────────────────────────────────────────────────────────

const F_TITLE: Font = Font::new(17.0, 650).family("Geist");
const F_SUB: Font = Font::new(13.0, 450).family("Geist");
const F_BIG: Font = Font::new(28.0, 650).family("Geist");
const F_SECTION: Font = Font::new(11.0, 650).family("Geist").track(0.08);
const F_ROW: Font = Font::new(13.0, 550).family("Geist");
const F_NUM: Font = Font::new(12.5, 500).family("Geist");
const F_SMALL: Font = Font::new(12.0, 500).family("Geist");
const F_TINY: Font = Font::new(10.5, 600).family("Geist");
const F_MONO: Font = Font::new(11.0, 700).family("Geist");
const F_BTN: Font = Font::new(12.5, 600).family("Geist");

/// Window rounding (inner surfaces are concentric with it).
const RADIUS: f32 = 28.0;
/// Backing a row carries while it travels, so it covers what it passes.
const LIFT: Rgba = Rgba::hex(0x0e0e12, 0.97);
/// Faint white: hover, quiet fills, inactive monogram tiles.
const FAINT: Rgba = Rgba::hex(0xffffff, 0.08);
const HAIR: Rgba = Rgba::hex(0xffffff, 0.08);
const SHADOW: Rgba = Rgba::hex(0x000000, 0.6);
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
const DIM: Rgba = Rgba::hex(0x5d5f69, 1.0);
const INK: Rgba = Rgba::hex(0x0b0b0e, 1.0);
const TRACK: Rgba = Rgba::hex(0xffffff, 0.10);
const BAD: Rgba = Rgba::hex(0xff7b93, 1.0);
const GOOD: Rgba = Rgba::hex(0x7ee2b8, 1.0);

const OPEN_W: SpringCfg = SpringCfg::new(0.4, 0.84);
const OPEN_H: SpringCfg = SpringCfg::new(0.5, 0.8);
/// Rows sliding to a new rank: quick with a hint of overshoot.
const SLIDE: SpringCfg = SpringCfg::new(0.5, 0.8);
const APPEAR: SpringCfg = SpringCfg::new(0.42, 0.92);
const BARS: SpringCfg = SpringCfg::new(0.55, 0.9);
/// One sample of graph scroll: lands well before the next sample (and is
/// snapped once the rest is sub-pixel, see `settle`).
const SCROLL: SpringCfg = SpringCfg::new(0.45, 1.0);
/// Content sections pouring in once the slab has (nearly) its size.
const REVEAL: SpringCfg = SpringCfg::new(0.42, 1.0);
/// The CPU graph's vertical scale (its ceiling) between 25/50/75/100%.
const CEIL: SpringCfg = SpringCfg::new(0.55, 1.0);
const XFADE: SpringCfg = SpringCfg::new(0.32, 1.0);
const SEL: SpringCfg = SpringCfg::new(0.36, 0.82);

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn sat(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// Stop a spring once what is left of its motion is invisible: under
/// `0.25 / px_per_unit` from the target and slower than 6 px/s. Keeps
/// sub-pixel tails from costing whole frames.
fn settle(s: &mut Spring, px_per_unit: f32) {
    if !s.at_rest() && ((s.pos - s.target) * px_per_unit).abs() < 0.25 && (s.vel * px_per_unit).abs() < 6.0 {
        s.snap(s.target);
    }
}

/// An input-time stamp still waiting for the next `step` (input handlers
/// run between frames, when `chrome.time` is stale).
const PENDING: f32 = f32::NEG_INFINITY;

// ── sampling (worker thread) ────────────────────────────────────────────

#[derive(Clone)]
pub struct Proc {
    pub pid: u32,
    pub name: Arc<str>,
    /// Percent of one core (top-style; can exceed 100 for threaded work).
    pub cpu: f32,
    pub rss: u64,
}

/// One graph texture plus the last two values (for the live dot).
pub struct Graph {
    img: ImageData,
    prev: f32,
    last: f32,
    /// Value at the top of the plot (the CPU graph zooms to 25/50/75/100%).
    ceil: f32,
}

pub struct Snapshot {
    /// Which window asked for it (several outputs may each have one).
    id: u64,
    cpu: f32,
    cores: Vec<f32>,
    mem_total: u64,
    mem_used: u64,
    swap_total: u64,
    swap_used: u64,
    load: [f32; 3],
    uptime: f64,
    threads: u32,
    procs: Vec<Proc>,
    graphs: Option<[Graph; 2]>,
    /// App icons resolved since the last snapshot (name, RGBA at the drawn
    /// size). Each name is sent once per worker.
    icons: Vec<(Arc<str>, ImageData)>,
}

struct Sampler {
    clk: f32,
    page: u64,
    /// (busy, total) jiffies per /proc/stat cpu line (0 = all).
    prev_cpu: Vec<(u64, u64)>,
    prev_ticks: HashMap<u32, u64>,
    next_ticks: HashMap<u32, u64>,
    /// (comm, display name) per pid; the name is redone when comm changes
    /// (exec, prctl renames).
    names: HashMap<u32, (Box<str>, Arc<str>)>,
    last: Option<Instant>,
    buf: String,
    path: String,
}

fn read_into(path: &str, buf: &mut String) -> bool {
    buf.clear();
    std::fs::File::open(path).and_then(|mut f| f.read_to_string(buf)).is_ok()
}

/// A readable name: the full executable name when `comm` was truncated,
/// without NixOS wrapper decorations (`.foo-wrapped`).
fn proc_name(comm: &str, cmdline: &[u8]) -> Arc<str> {
    let mut name = comm;
    let arg0 = cmdline.split(|b| *b == 0).next().unwrap_or(&[]);
    let arg0 = std::str::from_utf8(arg0).unwrap_or("");
    // setproctitle()-style command lines put everything in argv[0].
    let arg0 = if arg0.starts_with('/') { arg0.split(' ').next().unwrap_or(arg0) } else { arg0 };
    let base = arg0.rsplit('/').next().unwrap_or("");
    if comm.len() >= 15 {
        // Truncated: find the full name in argv (also `python3 foo-bar.py`).
        let full = std::iter::once(base)
            .chain(cmdline.split(|b| *b == 0).skip(1).filter_map(|a| std::str::from_utf8(a).ok()).map(|a| a.rsplit('/').next().unwrap_or("")))
            .find(|b| b.starts_with(comm));
        if let Some(f) = full {
            name = f;
        }
    }
    let name = name.strip_prefix('.').unwrap_or(name);
    let name = name.strip_suffix("-wrapped").unwrap_or(name);
    Arc::from(if name.is_empty() { comm } else { name })
}

impl Sampler {
    fn new() -> Self {
        let clk = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f32;
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        Self {
            clk,
            page,
            prev_cpu: Vec::new(),
            prev_ticks: HashMap::new(),
            next_ticks: HashMap::new(),
            names: HashMap::new(),
            last: None,
            buf: String::with_capacity(4096),
            path: String::with_capacity(32),
        }
    }

    fn sample(&mut self, id: u64) -> Snapshot {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |l| (now - l).as_secs_f32());
        self.last = Some(now);
        let mut s = Snapshot {
            id,
            cpu: 0.0,
            cores: Vec::new(),
            mem_total: 0,
            mem_used: 0,
            swap_total: 0,
            swap_used: 0,
            load: [0.0; 3],
            uptime: 0.0,
            threads: 0,
            procs: Vec::with_capacity(self.prev_ticks.len() + 16),
            graphs: None,
            icons: Vec::new(),
        };

        // CPU: busy share of the jiffies since the last sample.
        if read_into("/proc/stat", &mut self.buf) {
            let mut k = 0;
            for line in self.buf.lines().take_while(|l| l.starts_with("cpu")) {
                let mut f = line.split_ascii_whitespace().skip(1).map(|v| v.parse::<u64>().unwrap_or(0));
                let v: [u64; 8] = std::array::from_fn(|_| f.next().unwrap_or(0));
                let total: u64 = v.iter().sum();
                let busy = total - v[3] - v[4];
                let usage = match self.prev_cpu.get(k) {
                    Some(&(pb, pt)) if total > pt => (busy.saturating_sub(pb)) as f32 / (total - pt) as f32,
                    _ => 0.0,
                };
                if k < self.prev_cpu.len() {
                    self.prev_cpu[k] = (busy, total);
                } else {
                    self.prev_cpu.push((busy, total));
                }
                if k == 0 {
                    s.cpu = usage.clamp(0.0, 1.0);
                } else {
                    s.cores.push(usage.clamp(0.0, 1.0));
                }
                k += 1;
            }
        }
        if read_into("/proc/meminfo", &mut self.buf) {
            let (mut avail, mut swap_free) = (0, 0);
            for line in self.buf.lines() {
                let mut it = line.split_ascii_whitespace();
                let (Some(key), Some(v)) = (it.next(), it.next().and_then(|v| v.parse::<u64>().ok())) else { continue };
                match key {
                    "MemTotal:" => s.mem_total = v * 1024,
                    "MemAvailable:" => avail = v * 1024,
                    "SwapTotal:" => s.swap_total = v * 1024,
                    "SwapFree:" => swap_free = v * 1024,
                    _ => {}
                }
            }
            s.mem_used = s.mem_total.saturating_sub(avail);
            s.swap_used = s.swap_total.saturating_sub(swap_free);
        }
        if read_into("/proc/loadavg", &mut self.buf) {
            for (i, v) in self.buf.split_ascii_whitespace().take(3).enumerate() {
                s.load[i] = v.parse().unwrap_or(0.0);
            }
        }
        if read_into("/proc/uptime", &mut self.buf) {
            s.uptime = self.buf.split_ascii_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        }

        // Processes (user space only: kernel threads have no memory and
        // only clutter the list).
        self.next_ticks.clear();
        if let Ok(dir) = std::fs::read_dir("/proc") {
            for e in dir.flatten() {
                let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
                self.path.clear();
                let _ = write!(self.path, "/proc/{pid}/stat");
                if !read_into(&self.path, &mut self.buf) {
                    continue;
                }
                let (Some(open), Some(close)) = (self.buf.find('('), self.buf.rfind(')')) else { continue };
                let Some(rest) = self.buf.get(close + 2..) else { continue };
                let mut it = rest.split_ascii_whitespace();
                // Fields after the name: state(0) … flags(6) … utime(11)
                // stime(12) … num_threads(17) … rss(21).
                let flags: u64 = it.nth(6).and_then(|v| v.parse().ok()).unwrap_or(0);
                if flags & 0x0020_0000 != 0 {
                    continue;
                }
                let ut: u64 = it.nth(4).and_then(|v| v.parse().ok()).unwrap_or(0);
                let st: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let threads: u32 = it.nth(4).and_then(|v| v.parse().ok()).unwrap_or(1);
                let rss: u64 = it.nth(3).and_then(|v| v.parse().ok()).unwrap_or(0);
                let ticks = ut + st;
                let cpu = match self.prev_ticks.get(&pid) {
                    Some(&p) if dt > 0.0 => ticks.saturating_sub(p) as f32 / self.clk / dt * 100.0,
                    _ => 0.0,
                };
                self.next_ticks.insert(pid, ticks);
                let comm = &self.buf[open + 1..close];
                let name = match self.names.get(&pid) {
                    Some((c, n)) if **c == *comm => n.clone(),
                    _ => {
                        let comm: Box<str> = comm.into();
                        self.path.clear();
                        let _ = write!(self.path, "/proc/{pid}/cmdline");
                        let mut cmd = Vec::new();
                        let _ = std::fs::File::open(&self.path).and_then(|mut f| f.by_ref().take(512).read_to_end(&mut cmd));
                        let n = proc_name(&comm, &cmd);
                        self.names.insert(pid, (comm, n.clone()));
                        n
                    }
                };
                s.threads += threads;
                s.procs.push(Proc { pid, name, cpu, rss: rss * self.page });
            }
        }
        std::mem::swap(&mut self.prev_ticks, &mut self.next_ticks);
        let alive = &self.prev_ticks;
        self.names.retain(|pid, _| alive.contains_key(pid));
        s
    }
}

/// Rasterises the two area graphs. Lives on the worker with the history.
#[derive(Default)]
struct Painter {
    hist: [VecDeque<f32>; 2],
    pix: Option<tiny_skia::Pixmap>,
    /// The CPU graph's current ceiling (0 = not chosen yet).
    ceil: f32,
}

impl Painter {
    fn push(&mut self, cpu: f32, mem: f32) {
        for (h, v) in self.hist.iter_mut().zip([cpu, mem]) {
            if h.len() == HIST + 2 {
                h.pop_front();
            }
            h.push_back(v.clamp(0.0, 1.0));
        }
    }

    /// The texture covers HIST + 1 sample intervals: the visible HIST plus
    /// the one the view scrolls through when the next sample lands.
    fn paint(&mut self, which: usize, w_log: f32, scale: f32, acc: (Rgba, Rgba)) -> Graph {
        let stops = graph_stops(acc);
        let w = (w_log * scale).round().max(4.0);
        let tw = (w * (HIST as f32 + 1.0) / HIST as f32).round() as u32;
        let th = (GRAPH_H * scale).round().max(4.0) as u32;
        let hist = &self.hist[which];
        let n = hist.len();
        let last = hist.back().copied().unwrap_or(0.0);
        let prev = if n >= 2 { hist[n - 2] } else { last };
        let pm = match self.pix.take() {
            Some(mut p) if p.width() == tw && p.height() == th => {
                p.fill(tiny_skia::Color::TRANSPARENT);
                p
            }
            _ => tiny_skia::Pixmap::new(tw, th).expect("graph pixmap"),
        };
        let mut pm = pm;
        let (fw, fh) = (tw as f32, th as f32);
        let top = 5.0 * scale;
        let bottom = fh - 1.0 * scale;
        let dx = fw / (HIST as f32 + 1.0);
        let ceil = if which == 0 {
            // Up as soon as the peak needs it; down only once the peak is
            // clearly below the next step down (no flapping at a border).
            let peak = hist.iter().skip(n.saturating_sub(HIST + 1)).fold(0.0f32, |a, v| a.max(*v)) * 1.12;
            let fit = [0.25, 0.5, 0.75, 1.0].into_iter().find(|c| *c >= peak).unwrap_or(1.0);
            if fit >= self.ceil || peak < 0.8 * (self.ceil - 0.25) {
                self.ceil = fit;
            }
            self.ceil
        } else {
            1.0
        };
        let pt = |j: usize| (fw - (n - 1 - j) as f32 * dx, bottom - (hist[j] / ceil).min(1.0) * (bottom - top));
        // The fill is strongest just under the curve's highest point.
        let crest = (0..n).map(|j| pt(j).1).fold(bottom, f32::min).min(bottom - 8.0 * scale);
        let first_x = if n > 0 { pt(0).0 } else { fw };
        if n >= 2 {
            // Catmull-Rom through the samples, as cubic Béziers, with the
            // control points kept inside the plot.
            let curve = |pb: &mut tiny_skia::PathBuilder| {
                let (x0, y0) = pt(0);
                pb.move_to(x0, y0);
                for i in 0..n - 1 {
                    let p0 = pt(i.saturating_sub(1));
                    let p1 = pt(i);
                    let p2 = pt(i + 1);
                    let p3 = pt((i + 2).min(n - 1));
                    let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, (p1.1 + (p2.1 - p0.1) / 6.0).clamp(top, bottom));
                    let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, (p2.1 - (p3.1 - p1.1) / 6.0).clamp(top, bottom));
                    pb.cubic_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
                }
            };
            let mut pb = tiny_skia::PathBuilder::new();
            curve(&mut pb);
            let line = pb.finish();
            if let Some(line) = line {
                let grad = |a: f32| {
                    tiny_skia::LinearGradient::new(
                        tiny_skia::Point::from_xy(0.0, 0.0),
                        tiny_skia::Point::from_xy(fw, 0.0),
                        stops.iter().map(|(t, c)| tiny_skia::GradientStop::new(*t, color(*c, a))).collect(),
                        tiny_skia::SpreadMode::Pad,
                        tiny_skia::Transform::identity(),
                    )
                };
                // Area under the curve, then a vertical alpha ramp on it.
                let mut area = tiny_skia::PathBuilder::new();
                curve(&mut area);
                area.line_to(fw, fh);
                area.line_to(first_x, fh);
                area.close();
                if let (Some(area), Some(shader)) = (area.finish(), grad(1.0)) {
                    let paint = tiny_skia::Paint { shader, anti_alias: true, ..Default::default() };
                    pm.fill_path(&area, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
                    let row = tw as usize * 4;
                    for (y, px) in pm.data_mut().chunks_exact_mut(row).enumerate() {
                        let t = ((y as f32 - crest) / (fh - crest)).clamp(0.0, 1.0);
                        let k = 0.36 * (1.0 - t).powf(1.4) + 0.03;
                        for c in px.iter_mut() {
                            *c = (*c as f32 * k) as u8;
                        }
                    }
                }
                if let Some(shader) = grad(1.0) {
                    let paint = tiny_skia::Paint { shader, anti_alias: true, ..Default::default() };
                    let stroke = tiny_skia::Stroke {
                        width: 1.7 * scale,
                        line_cap: tiny_skia::LineCap::Round,
                        line_join: tiny_skia::LineJoin::Round,
                        ..Default::default()
                    };
                    pm.stroke_path(&line, &paint, &stroke, tiny_skia::Transform::identity(), None);
                }
            }
        }
        // Straight alpha for the GPU; transparent texels take the line
        // colour so linear filtering never darkens the edges. Old samples
        // fade out at the left edge (and the very first ones fade in).
        let mut out = Vec::with_capacity((tw * th * 4) as usize);
        let fade_from = first_x.max(0.0);
        let fade_w = (fw * 0.14).max(1.0);
        let cols: Vec<([u8; 3], f32)> = (0..tw)
            .map(|x| {
                let c = stop_at(&stops, x as f32 / fw);
                let ss = |f: f32| f * f * (3.0 - 2.0 * f);
                let old = ss((x as f32 / fade_w).clamp(0.0, 1.0));
                let start = ss(((x as f32 - fade_from) / (1.5 * dx)).clamp(0.0, 1.0));
                ([(c.0 * 255.0) as u8, (c.1 * 255.0) as u8, (c.2 * 255.0) as u8], old * start)
            })
            .collect();
        for px in pm.data().chunks_exact(tw as usize * 4) {
            for (x, p) in px.chunks_exact(4).enumerate() {
                let (rgb, f) = cols[x];
                let a = p[3];
                if a == 0 {
                    out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0]);
                } else {
                    let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
                    out.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2]), (a as f32 * f) as u8]);
                }
            }
        }
        self.pix = Some(pm);
        Graph { img: ImageData { w: tw, h: th, rgba: Arc::new(out) }, prev: prev / ceil, last: last / ceil, ceil }
    }

    fn paint_both(&mut self, scale: f32) -> [Graph; 2] {
        let (a, b) = theme::accent();
        [self.paint(0, CPU_W - 2.0 * GRAPH_IN, scale, (a, b)), self.paint(1, MEM_W - 2.0 * GRAPH_IN, scale, (b, a))]
    }
}

/// Full-colour app icons for process names, on the worker. The hicolor
/// `apps` folders (and pixmaps) are indexed once per worker, so a name costs
/// a few hash lookups; only hits are rasterised, once, at the drawn size.
#[derive(Default)]
struct IconFinder {
    /// Lower-case icon name → (preference, path); lower is better.
    index: Option<HashMap<String, (u32, std::path::PathBuf)>>,
    /// Every name looked at so far (hit or miss).
    seen: HashSet<Arc<str>>,
    px: u32,
}

impl IconFinder {
    fn build(px: u32) -> HashMap<String, (u32, std::path::PathBuf)> {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        if let Some(h) = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        {
            dirs.push(h);
        }
        let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/run/current-system/sw/share:/usr/share".into());
        dirs.extend(sys.split(':').filter(|d| !d.is_empty()).map(std::path::PathBuf::from));
        let mut index: HashMap<String, (u32, std::path::PathBuf)> = HashMap::new();
        let mut add = |file: std::path::PathBuf, size: Option<u32>| {
            let (Some(stem), Some(ext)) = (file.file_stem().and_then(|s| s.to_str()), file.extension().and_then(|s| s.to_str())) else { return };
            if stem.contains("symbolic") || !(ext == "svg" || ext == "png") {
                return;
            }
            // Vector first, then the smallest bitmap at least as big as drawn.
            let rank = match (ext, size) {
                ("svg", _) => 0,
                (_, Some(s)) if s >= px => 1000 + s,
                (_, Some(s)) => 100_000 - s,
                _ => 200_000,
            };
            let key = stem.to_ascii_lowercase();
            if index.get(&key).is_none_or(|(r, _)| rank < *r) {
                index.insert(key, (rank, file));
            }
        };
        for base in &dirs {
            if let Ok(sizes) = std::fs::read_dir(base.join("icons/hicolor")) {
                for sz in sizes.flatten() {
                    let n = sz.file_name();
                    let n = n.to_string_lossy();
                    let size = if n == "scalable" { None } else { n.split('x').next().and_then(|v| v.parse::<u32>().ok()) };
                    if (n != "scalable" && size.is_none()) || n.contains('@') {
                        continue;
                    }
                    if let Ok(files) = std::fs::read_dir(sz.path().join("apps")) {
                        for f in files.flatten() {
                            add(f.path(), size);
                        }
                    }
                }
            }
            if let Ok(files) = std::fs::read_dir(base.join("pixmaps")) {
                for f in files.flatten() {
                    add(f.path(), Some(48));
                }
            }
        }
        index
    }

    fn lookup<'a>(index: &'a HashMap<String, (u32, std::path::PathBuf)>, name: &str) -> Option<&'a std::path::Path> {
        let n = name.to_ascii_lowercase();
        let n = n.trim_end_matches("-bin");
        if n.len() < 3 || n.contains(' ') {
            return None;
        }
        if let Some((_, p)) = index.get(n) {
            return Some(p);
        }
        // Reverse-DNS ids (org.mozilla.firefox) and `-client`/`-desktop`
        // style suffixes; the best-ranked match wins.
        let dotted = format!(".{n}");
        let dashed = format!("{n}-");
        index.iter().filter(|(k, _)| k.ends_with(&dotted) || k.starts_with(&dashed)).min_by_key(|(k, (r, _))| (k.len(), *r)).map(|(_, (_, p))| p.as_path())
    }

    /// Icons for names not looked at before (at `px` device pixels).
    fn resolve<'a>(&mut self, names: impl Iterator<Item = &'a Arc<str>>, px: u32) -> Vec<(Arc<str>, ImageData)> {
        if px != self.px {
            // New scale: everything is redone at the new size.
            self.px = px;
            self.seen.clear();
            self.index = None;
        }
        let mut out = Vec::new();
        for n in names {
            if self.seen.contains(n) {
                continue;
            }
            self.seen.insert(n.clone());
            let index = self.index.get_or_insert_with(|| Self::build(px));
            if let Some(img) = Self::lookup(index, n).and_then(|p| crate::appicons::load(p, px)) {
                out.push((n.clone(), img));
            }
        }
        out
    }
}

fn icon_px(scale: f32) -> u32 {
    (ICON * scale).round().max(8.0) as u32
}

/// Colour stops of a graph's gradient (left → right): the Prism Ink
/// iridescence, slightly desaturated like the shader's, or the accent pair.
fn graph_stops((ca, cb): (Rgba, Rgba)) -> Vec<(f32, Rgba)> {
    if theme::prism() {
        let d = |c: Rgba| {
            let l = 0.299 * c.0 + 0.587 * c.1 + 0.114 * c.2;
            c.mix(Rgba(l, l, l, c.3), 0.14)
        };
        vec![
            (0.0, d(Rgba::hex(0x9fdcff, 1.0))),
            (0.4, d(Rgba::hex(0xc9b2ff, 1.0))),
            (0.7, d(Rgba::hex(0xffb3e4, 1.0))),
            (1.0, d(Rgba::hex(0xffd2a8, 1.0))),
        ]
    } else {
        vec![(0.0, ca), (1.0, cb)]
    }
}

/// A gradient's colour at `t` (0..1).
fn stop_at(stops: &[(f32, Rgba)], t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        if t <= w[1].0 {
            return w[0].1.mix(w[1].1, (t - w[0].0) / (w[1].0 - w[0].0).max(1e-4));
        }
    }
    stops.last().map_or(Rgba::TRANSPARENT, |s| s.1)
}

fn color(c: Rgba, a: f32) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c.0, c.1, c.2, (c.3 * a).clamp(0.0, 1.0)).unwrap_or(tiny_skia::Color::BLACK)
}

/// The sampling thread. Dropping it stops the thread at its next wake-up
/// (it is unparked immediately, so that is now).
struct Worker {
    stop: Arc<AtomicBool>,
    thread: std::thread::Thread,
}

impl Worker {
    fn spawn(id: u64, tx: calloop::channel::Sender<Fact>, scale: Arc<AtomicU32>, painter: Arc<Mutex<Painter>>) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let h = std::thread::Builder::new().name("nimbus-monitor".into()).spawn(move || run(id, tx, st, scale, painter)).ok()?;
        Some(Self { stop, thread: h.thread().clone() })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

fn run(id: u64, tx: calloop::channel::Sender<Fact>, stop: Arc<AtomicBool>, scale: Arc<AtomicU32>, painter: Arc<Mutex<Painter>>) {
    let mut s = Sampler::new();
    let mut icons = IconFinder::default();
    // Prime the deltas (and resolve the icons of what is running), then
    // deliver the first real sample quickly.
    let mut next = Instant::now() + Duration::from_millis(280);
    let first = s.sample(id);
    let mut pending = icons.resolve(first.procs.iter().map(|p| &p.name), icon_px(f32::from_bits(scale.load(Ordering::Relaxed))));
    loop {
        loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let now = Instant::now();
            if now >= next {
                break;
            }
            std::thread::park_timeout(next - now);
        }
        let mut snap = s.sample(id);
        let sc = f32::from_bits(scale.load(Ordering::Relaxed));
        pending.extend(icons.resolve(snap.procs.iter().map(|p| &p.name), icon_px(sc)));
        snap.icons = std::mem::take(&mut pending);
        let mem = if snap.mem_total > 0 { snap.mem_used as f32 / snap.mem_total as f32 } else { 0.0 };
        // Closed while sampling: this sample must not reach the shared
        // history (a reopened window's worker owns it now) or the window.
        if stop.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut p) = painter.lock() {
            if stop.load(Ordering::Acquire) {
                return;
            }
            p.push(snap.cpu, mem);
            snap.graphs = Some(p.paint_both(sc));
        }
        if tx.send(Fact::Custom(Arc::new(snap))).is_err() {
            return;
        }
        next += Duration::from_secs(1);
        let now = Instant::now();
        if next < now {
            next = now + Duration::from_secs(1);
        }
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

// ── formatting ──────────────────────────────────────────────────────────

fn fmt_bytes(b: u64) -> String {
    const GI: f64 = (1u64 << 30) as f64;
    const MI: f64 = (1u64 << 20) as f64;
    let b = b as f64;
    if b >= 100.0 * GI {
        format!("{:.0} GiB", b / GI)
    } else if b >= GI {
        format!("{:.2} GiB", b / GI)
    } else if b >= MI {
        format!("{:.0} MiB", b / MI)
    } else {
        format!("{:.0} KiB", b / 1024.0)
    }
}

fn fmt_gib(b: u64) -> String {
    format!("{:.1}", b as f64 / (1u64 << 30) as f64)
}

fn fmt_uptime(s: f64) -> String {
    let m = (s / 60.0) as u64;
    let (d, h, m) = (m / 1440, m / 60 % 24, m % 60);
    if d > 0 {
        format!("up {d}d {h}h")
    } else if h > 0 {
        format!("up {h}h {m}m")
    } else {
        format!("up {m}m")
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// ASCII case-insensitive substring test (no allocation).
fn contains_ci(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

// ── window ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sort {
    Name,
    Pid,
    Cpu,
    Mem,
}

const SORTS: [Sort; 4] = [Sort::Name, Sort::Pid, Sort::Cpu, Sort::Mem];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ctl {
    None,
    Body,
    Close,
    Head(Sort),
    Row(u32),
    Search,
    End,
    Kill,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowState {
    Live,
    /// Still running but ranked out of view: slides off the bottom.
    Leaving,
    /// Exited or filtered out: collapses in place.
    Gone,
}

struct Row {
    name: Arc<str>,
    pid_txt: String,
    cpu: f32,
    cpu_txt: String,
    mem_txt: String,
    old_cpu: String,
    old_mem: String,
    idx: Spring,
    appear: Spring,
    cpu_bar: Spring,
    mem_bar: Spring,
    xfade: Spring,
    state: RowState,
    seen: bool,
    /// When a signal was sent (dimmed until it goes, or for ENDED_TIME).
    ended: Option<f32>,
    dim: Spring,
    /// Stacking while moving, fixed for the whole move: 0 still/fading,
    /// 1 falling, 2 climbing (drawn above, so it passes over like a card).
    z: u8,
}

/// A value that cross-fades in place when it changes.
struct Swap {
    cur: String,
    old: String,
    t: Spring,
}

impl Swap {
    fn new() -> Self {
        Self { cur: String::new(), old: String::new(), t: Spring::new(1.0, XFADE) }
    }
    fn set(&mut self, s: String) {
        if s != self.cur {
            self.old = std::mem::replace(&mut self.cur, s);
            self.t.snap(0.0);
            self.t.set(1.0);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Why {
    Snap,
    Sort,
    Filter,
    /// The rank hold ended: apply the ranks it held back.
    Release,
}

pub struct Monitor {
    chrome: Chrome,
    text: TextCache,
    icons: Rc<IconSet>,
    scale: f32,
    scale_bits: Arc<AtomicU32>,
    id: u64,
    tx: calloop::channel::Sender<Fact>,
    worker: Option<Worker>,
    spawn: bool,
    /// Graph history and rasteriser, shared with the worker so a reopen
    /// continues the same minute.
    painter: Arc<Mutex<Painter>>,
    /// Demo mode: never send real signals.
    dry: bool,

    snap: Option<Arc<Snapshot>>,
    /// Per pid: (smoothed CPU, rank key). The key follows the smoothed
    /// value only when that moved clearly, so near-ties keep their order.
    cpu_key: HashMap<u32, (f32, f32)>,
    /// Last position of each listed pid (the tie-break: no reshuffles).
    rank: HashMap<u32, u32>,
    /// Ranks are held while the pointer is over the table (and a moment
    /// after it leaves) or a row is selected; a re-rank waits.
    in_table: bool,
    left_at: Option<f32>,
    rerank: bool,
    prev_pids: HashSet<u32>,
    shown: HashSet<u32>,
    list: Vec<u32>,
    order: Vec<u32>,
    rows: HashMap<u32, Row>,
    matches: usize,
    first: bool,

    // header / graph values
    subtitle: String,
    load_txt: String,
    swap_txt: String,
    summary: String,
    match_txt: String,
    cpu_big: Swap,
    mem_big: Swap,
    mem_of: String,
    big_w: [Spring; 2],
    cores: Vec<Spring>,
    swap_bar: Spring,
    scroll: Spring,
    dots: [(f32, f32); 2],
    /// Ceiling the CPU texture was painted at, and the one shown (the
    /// texture is stretched between the two, so a rescale glides).
    tex_ceil: f32,
    ceil: Spring,
    ceil_txt: Swap,
    tex_keys: [u64; 2],
    tex_live: bool,
    uploads: Vec<(u64, ImageData)>,
    /// App icon textures by process name: (key, pixel size).
    icon_keys: HashMap<Arc<str>, (u64, u32)>,
    icon_base: u64,
    /// Content sections, once the slab has (nearly) its size.
    reveal: Spring,

    // table
    sort: Sort,
    desc: bool,
    /// Left and right edges of the sort pill (the leading edge is quicker).
    sort_l: Spring,
    sort_r: Spring,
    sort_dir: Spring,
    filter: String,
    selected: Option<u32>,
    sel_from: f32,
    sel_last: f32,
    sel_t: Spring,
    sel_a: Spring,

    // footer
    sum_a: Spring,
    act_a: Spring,
    stat_a: Spring,
    armed: Option<f32>,
    arm_t: Spring,
    /// Where the confirm fill grows from (0..1 across the button).
    arm_x: f32,
    status: Option<(String, bool)>,
    status_until: f32,
    sel_name: String,
    sel_pid: String,

    // input
    hover: HashMap<Ctl, Spring>,
    hovered: Ctl,
    pressed: Ctl,
    mouse: Option<(f32, f32)>,
    hits: Vec<(Rect, Ctl)>,
    head_rects: [Rect; 4],
    /// The rows area (surface coords, last draw) for the rank hold.
    table_rect: Rect,
    scratch: Vec<u32>,
}

pub fn open(ctx: &mut Ctx) -> Box<dyn WinScene> {
    Box::new(Monitor::new(ctx, true, false))
}

fn spring0() -> Spring {
    Spring::new(0.0, presets::SNAPPY)
}

impl Monitor {
    fn new(ctx: &mut Ctx, spawn: bool, dry: bool) -> Self {
        let mut text = TextCache::new(ctx.fs, theme::current());
        text.set_scale(ctx.scale);
        let home = Rect::new(M, M, FW, FH);
        let start = Rect::new(SW * 0.5 - 150.0, SH * 0.5 - 34.0, 300.0, 68.0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let scale_bits = Arc::new(AtomicU32::new(ctx.scale.to_bits()));
        let painter = Arc::new(Mutex::new(Painter::default()));
        let worker = if spawn { Worker::spawn(id, ctx.fact_tx.clone(), scale_bits.clone(), painter.clone()) } else { None };
        let base = 0x6d6f_6e00_0000_0000u64 | (id << 8);
        Self {
            chrome: Chrome::new(start, home, OPEN_W, OPEN_H),
            text,
            icons: ctx.icons.clone(),
            scale: ctx.scale,
            scale_bits,
            id,
            tx: ctx.fact_tx.clone(),
            worker,
            spawn,
            painter,
            dry,
            snap: None,
            cpu_key: HashMap::new(),
            rank: HashMap::new(),
            in_table: false,
            left_at: None,
            rerank: false,
            prev_pids: HashSet::new(),
            shown: HashSet::new(),
            list: Vec::new(),
            order: Vec::new(),
            rows: HashMap::new(),
            matches: 0,
            first: true,
            subtitle: String::new(),
            load_txt: String::new(),
            swap_txt: String::new(),
            summary: String::new(),
            match_txt: String::new(),
            cpu_big: Swap::new(),
            mem_big: Swap::new(),
            mem_of: String::new(),
            big_w: [Spring::new(0.0, presets::SMOOTH), Spring::new(0.0, presets::SMOOTH)],
            cores: Vec::new(),
            swap_bar: Spring::new(0.0, BARS),
            scroll: Spring::new(1.0, SCROLL),
            dots: [(0.0, 0.0); 2],
            tex_ceil: 1.0,
            ceil: Spring::new(0.0, CEIL),
            ceil_txt: Swap::new(),
            tex_keys: [base | 1, base | 2],
            tex_live: false,
            uploads: Vec::new(),
            icon_keys: HashMap::new(),
            icon_base: 0x6d69_0000_0000_0000u64 | (id << 24),
            reveal: Spring::new(0.0, REVEAL),
            sort: Sort::Cpu,
            desc: true,
            sort_l: Spring::new(0.0, presets::SNAPPY),
            sort_r: Spring::new(0.0, presets::SNAPPY),
            sort_dir: Spring::new(0.0, presets::SNAPPY),
            filter: String::new(),
            selected: None,
            sel_from: 0.0,
            sel_last: 0.0,
            sel_t: Spring::new(1.0, SEL),
            sel_a: Spring::new(0.0, presets::SMOOTH),
            sum_a: Spring::new(1.0, XFADE),
            act_a: Spring::new(0.0, presets::SNAPPY),
            stat_a: Spring::new(0.0, XFADE),
            armed: None,
            arm_t: Spring::new(0.0, SpringCfg::new(0.42, 0.86)),
            arm_x: 0.5,
            status: None,
            status_until: 0.0,
            sel_name: String::new(),
            sel_pid: String::new(),
            hover: HashMap::new(),
            hovered: Ctl::None,
            pressed: Ctl::None,
            mouse: None,
            hits: Vec::new(),
            head_rects: [Rect::default(); 4],
            table_rect: Rect::default(),
            scratch: Vec::new(),
        }
    }

    // ── data ────────────────────────────────────────────────────────────

    fn take_snapshot(&mut self, s: Arc<Snapshot>) {
        let n = s.cores.len();
        if self.cores.len() != n {
            self.cores = (0..n).map(|_| Spring::new(0.0, BARS)).collect();
        }
        for (sp, v) in self.cores.iter_mut().zip(&s.cores) {
            sp.set(*v);
        }
        self.cpu_big.set(format!("{:.0}%", s.cpu * 100.0));
        self.mem_big.set(fmt_gib(s.mem_used));
        self.mem_of = format!("GiB of {}", fmt_gib(s.mem_total));
        self.subtitle = format!("{} cores  ·  {}", n.max(1), fmt_uptime(s.uptime));
        self.load_txt = format!("{:.2}   {:.2}   {:.2}", s.load[0], s.load[1], s.load[2]);
        self.swap_txt = if s.swap_total > 0 {
            format!("{} of {} GiB", fmt_gib(s.swap_used), fmt_gib(s.swap_total))
        } else {
            "None".into()
        };
        self.swap_bar.set(if s.swap_total > 0 { s.swap_used as f32 / s.swap_total as f32 } else { 0.0 });
        self.summary = format!("{} processes  ·  {} threads", thousands(s.procs.len()), thousands(s.threads as usize));
        if let Some(g) = &s.graphs {
            for (k, g) in g.iter().enumerate() {
                self.uploads.retain(|(key, _)| *key != self.tex_keys[k]);
                self.uploads.push((self.tex_keys[k], g.img.clone()));
                self.dots[k] = (g.prev, g.last);
                if k == 0 {
                    self.tex_ceil = g.ceil;
                    self.ceil.set(g.ceil);
                    if self.ceil.get() <= 0.0 {
                        self.ceil.snap(g.ceil);
                    }
                    self.ceil_txt.set(format!("{:.0}%", g.ceil * 100.0));
                }
            }
            // Continue the scroll from where it is: one sample further.
            self.scroll.pos -= 1.0;
            self.scroll.target = 1.0;
            if self.scroll.pos < -0.5 {
                self.scroll.pos = 0.0;
            }
        }
        for (name, img) in &s.icons {
            // A reopened window's new worker sends its icons again: upload
            // only what is new (or now drawn at another size).
            let n = self.icon_keys.len() as u64;
            let e = self.icon_keys.entry(name.clone()).or_insert((self.icon_base | n, 0));
            if e.1 != img.w {
                e.1 = img.w;
                self.uploads.push((e.0, img.clone()));
            }
        }
        // Smoothed CPU per pid, and the sticky rank key on top of it.
        let keys = &mut self.cpu_key;
        for p in &s.procs {
            let e = keys.entry(p.pid).or_insert((p.cpu, p.cpu));
            e.0 += (p.cpu - e.0) * EMA;
            if (e.0 - e.1).abs() > (0.15 * e.1).max(1.0) {
                e.1 = e.0;
            }
        }
        if keys.len() > s.procs.len() {
            let alive: HashSet<u32> = s.procs.iter().map(|p| p.pid).collect();
            keys.retain(|pid, _| alive.contains(pid));
        }
        self.snap = Some(s);
        self.relayout(Why::Snap);
        if let Some(s) = &self.snap {
            self.prev_pids.clear();
            self.prev_pids.extend(s.procs.iter().map(|p| p.pid));
        }
        self.first = false;
    }

    fn matches(&self, p: &Proc) -> bool {
        let f = self.filter.as_str();
        f.is_empty() || contains_ci(&p.name, f) || (f.bytes().all(|b| b.is_ascii_digit()) && p.pid.to_string().starts_with(f))
    }

    /// Ranks hold still while the pointer is over the table (and a moment
    /// after it leaves) or a row is selected, so nothing slides away under
    /// a click.
    fn holding(&self) -> bool {
        self.in_table || self.left_at.is_some() || self.selected.is_some()
    }

    fn relayout(&mut self, why: Why) {
        let Some(snap) = self.snap.clone() else { return };
        let procs = &snap.procs;
        let mut list = std::mem::take(&mut self.list);
        list.clear();
        for (i, p) in procs.iter().enumerate() {
            if self.matches(p) {
                list.push(i as u32);
            }
        }
        let (sort, desc) = (self.sort, self.desc);
        let (keys, rank) = (&self.cpu_key, &self.rank);
        let ck = |p: &Proc| keys.get(&p.pid).map_or(p.cpu, |k| k.1);
        let rk = |p: &Proc| rank.get(&p.pid).copied().unwrap_or(u32::MAX);
        list.sort_unstable_by(|&a, &b| {
            let (a, b) = (&procs[a as usize], &procs[b as usize]);
            let o = match sort {
                Sort::Cpu => ck(a).total_cmp(&ck(b)),
                Sort::Mem => a.rss.cmp(&b.rss),
                Sort::Pid => a.pid.cmp(&b.pid),
                Sort::Name => {
                    let (x, y) = (a.name.bytes().map(|c| c.to_ascii_lowercase()), b.name.bytes().map(|c| c.to_ascii_lowercase()));
                    x.cmp(y)
                }
            };
            // Equal keys keep their previous order.
            (if desc { o.reverse() } else { o }).then(rk(a).cmp(&rk(b))).then(a.pid.cmp(&b.pid))
        });
        self.matches = list.len();
        self.match_txt = if self.filter.is_empty() { String::new() } else { format!("{} of {}", self.matches, procs.len()) };
        self.shown.clear();
        self.shown.extend(list.iter().map(|&i| procs[i as usize].pid));
        let at: HashMap<u32, u32> = list.iter().map(|&i| (procs[i as usize].pid, i)).collect();

        // Which pids get the visible slots, in order.
        let mut vis = std::mem::take(&mut self.scratch);
        vis.clear();
        let hold = why == Why::Snap && self.holding();
        if hold {
            // Same order as on screen; exited rows leave gaps that the next
            // ranked processes fill.
            vis.extend(self.order.iter().copied().filter(|p| at.contains_key(p)));
        }
        for &i in &list {
            if vis.len() >= ROWS {
                break;
            }
            let pid = procs[i as usize].pid;
            if !hold || !vis.contains(&pid) {
                vis.push(pid);
            }
        }
        // The selection never scrolls out of view: it keeps the last slot.
        if let Some(sel) = self.selected
            && at.contains_key(&sel)
            && !vis.contains(&sel)
        {
            vis.truncate(ROWS - 1);
            vis.push(sel);
        }
        self.rerank = hold && list.iter().take(vis.len()).map(|&i| procs[i as usize].pid).ne(vis.iter().copied());
        self.rank.clear();
        for (n, &i) in list.iter().enumerate() {
            self.rank.insert(procs[i as usize].pid, ROWS as u32 + n as u32);
        }
        for (n, pid) in vis.iter().enumerate() {
            self.rank.insert(*pid, n as u32);
        }

        let max_rss = list.iter().map(|&i| procs[i as usize].rss).max().unwrap_or(1).max(1) as f32;
        for r in self.rows.values_mut() {
            r.seen = false;
        }
        self.order.clear();
        let first = self.first;
        for (i, &pid) in vis.iter().enumerate() {
            let p = &procs[at[&pid] as usize];
            let fi = i as f32;
            self.order.push(pid);
            // Shown and ranked by the smoothed value (a steady number).
            let cpu = self.cpu_key.get(&pid).map_or(p.cpu, |k| k.0);
            let cpu_v = (cpu / 100.0).clamp(0.0, 1.0);
            let mem_v = p.rss as f32 / max_rss;
            let cpu_txt = format!("{cpu:.1}%");
            let mem_txt = fmt_bytes(p.rss);
            match self.rows.get_mut(&pid) {
                Some(r) => {
                    if r.state == RowState::Gone {
                        r.appear.set(1.0);
                    }
                    r.state = RowState::Live;
                    r.seen = true;
                    if (r.idx.target - fi).abs() > 0.01 {
                        // Decided once per move: climbers pass over fallers.
                        r.z = if fi < r.idx.get() { 2 } else { 1 };
                    }
                    r.idx.set(fi);
                    r.cpu = cpu;
                    r.cpu_bar.set(cpu_v);
                    r.mem_bar.set(mem_v);
                    if r.cpu_txt != cpu_txt || r.mem_txt != mem_txt {
                        r.old_cpu = std::mem::replace(&mut r.cpu_txt, cpu_txt);
                        r.old_mem = std::mem::replace(&mut r.mem_txt, mem_txt);
                        r.xfade.snap(0.0);
                        r.xfade.set(1.0);
                    }
                    if *r.name != *p.name {
                        r.name = p.name.clone();
                    }
                }
                None => {
                    let from_below = match why {
                        Why::Sort => true,
                        Why::Snap | Why::Release => !first && self.prev_pids.contains(&pid),
                        Why::Filter => false,
                    };
                    let mut r = Row {
                        name: p.name.clone(),
                        pid_txt: pid.to_string(),
                        cpu,
                        cpu_txt,
                        mem_txt,
                        old_cpu: String::new(),
                        old_mem: String::new(),
                        idx: Spring::new(fi, SLIDE),
                        appear: Spring::new(0.0, APPEAR),
                        cpu_bar: Spring::new(0.0, BARS),
                        mem_bar: Spring::new(0.0, BARS),
                        xfade: Spring::new(1.0, XFADE),
                        state: RowState::Live,
                        seen: true,
                        ended: None,
                        dim: Spring::new(0.0, presets::SMOOTH),
                        z: 0,
                    };
                    if from_below {
                        // Climbing in from below the fold.
                        r.idx.snap(ROWS as f32 + 0.6);
                        r.idx.set(fi);
                        r.z = 2;
                        r.appear.snap(1.0);
                        r.cpu_bar.snap(cpu_v);
                        r.mem_bar.snap(mem_v);
                    } else {
                        // New: fade/scale in where it belongs (staggered
                        // on the first sample, so the table pours in).
                        r.appear.snap(if first { -0.14 * fi } else { 0.0 });
                        r.appear.set(1.0);
                        r.cpu_bar.set(cpu_v);
                        r.mem_bar.set(mem_v);
                    }
                    self.rows.insert(pid, r);
                }
            }
        }
        self.scratch = vis;
        let shown = &self.shown;
        for (pid, r) in self.rows.iter_mut() {
            if !r.seen && r.state == RowState::Live {
                if shown.contains(pid) {
                    r.state = RowState::Leaving;
                    r.z = 1;
                    r.idx.set(ROWS as f32 + 0.6);
                } else {
                    r.state = RowState::Gone;
                    r.z = 0;
                    r.appear.set(0.0);
                }
            }
        }
        self.list = list;
        // Only a process that exited or no longer matches loses the
        // selection (being out of view cannot happen: it is pinned).
        if let Some(sel) = self.selected
            && !self.shown.contains(&sel)
        {
            self.select(None);
        }
    }

    // ── interaction ─────────────────────────────────────────────────────

    fn select(&mut self, pid: Option<u32>) {
        if pid == self.selected {
            return;
        }
        let had = self.selected.is_some() && self.sel_a.get() > 0.3;
        self.armed = None;
        self.selected = pid;
        let Some(p) = pid else { return };
        if let Some(r) = self.rows.get(&p) {
            // The highlight glides from the old row to the new one.
            self.sel_from = if had { self.sel_last } else { r.idx.get() };
            self.sel_name = r.name.to_string();
        } else {
            // Not on screen yet (it gets pinned into view): rise from below.
            self.sel_from = if had { self.sel_last } else { ROWS as f32 };
            self.sel_name = self.snap.as_ref().and_then(|s| s.procs.iter().find(|q| q.pid == p)).map_or_else(String::new, |q| q.name.to_string());
        }
        self.sel_t.snap(0.0);
        self.sel_t.set(1.0);
        self.sel_pid = format!("PID {p}");
    }

    fn set_sort(&mut self, s: Sort) {
        if s == self.sort {
            self.desc = !self.desc;
        } else {
            self.sort = s;
            self.desc = matches!(s, Sort::Cpu | Sort::Mem);
        }
        self.relayout(Why::Sort);
    }

    fn set_status(&mut self, msg: String, ok: bool) {
        self.status = Some((msg, ok));
        // Stamped by the next step (chrome.time is stale between frames).
        self.status_until = PENDING;
    }

    /// Arm the force quit; the confirm fill grows from `x` (0..1 across
    /// the button).
    fn arm(&mut self, x: f32) {
        if self.selected.is_some() {
            self.armed = Some(PENDING);
            self.arm_x = sat(x);
        }
    }

    fn signal(&mut self, kill: bool) {
        let Some(pid) = self.selected else { return };
        let name = self.rows.get(&pid).map_or_else(|| pid.to_string(), |r| r.name.to_string());
        let (sig, label) = if kill { (libc::SIGKILL, "SIGKILL") } else { (libc::SIGTERM, "SIGTERM") };
        let res = if self.dry || pid <= 1 {
            if pid <= 1 { Err(libc::EPERM) } else { Ok(()) }
        } else if unsafe { libc::kill(pid as libc::pid_t, sig) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
        };
        match res {
            Ok(()) => {
                if let Some(r) = self.rows.get_mut(&pid) {
                    r.ended = Some(PENDING);
                }
                self.set_status(format!("Sent {label} to {name}"), true);
            }
            Err(e) if e == libc::EPERM => self.set_status(format!("Not allowed to end {name}"), false),
            Err(e) if e == libc::ESRCH => self.set_status(format!("{name} has already exited"), false),
            Err(e) => self.set_status(format!("Could not signal {name} ({})", std::io::Error::from_raw_os_error(e)), false),
        }
        self.armed = None;
    }

    fn move_sel(&mut self, d: i32) {
        if self.order.is_empty() {
            return;
        }
        let i = self.selected.and_then(|p| self.order.iter().position(|&x| x == p));
        let n = self.order.len() as i32;
        let j = match i {
            Some(i) => (i as i32 + d).clamp(0, n - 1),
            None if d > 0 => 0,
            None => n - 1,
        };
        self.select(Some(self.order[j as usize]));
    }

    fn hit(&self, x: f32, y: f32) -> Ctl {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map_or(Ctl::None, |(_, c)| *c)
    }

    /// Centre of a control's hit area from the last draw (snapshots).
    pub fn hit_center(&self, c: Ctl) -> Option<(f32, f32)> {
        self.hits.iter().rev().find(|(_, h)| *h == c).map(|(r, _)| (r.x + r.w * 0.5, r.y + r.h * 0.5))
    }

    fn hover_of(&self, c: Ctl) -> f32 {
        self.hover.get(&c).map_or(0.0, |s| sat(s.get()))
    }

    // ── drawing helpers ─────────────────────────────────────────────────

    fn px(&self, r: Rect) -> [i32; 4] {
        let s = self.scale;
        [(r.x * s).floor() as i32, (r.y * s).floor() as i32, ((r.x + r.w) * s).ceil() as i32, ((r.y + r.h) * s).ceil() as i32]
    }

    fn put(&self, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect) {
        if c.3 <= 0.003 || clip.w <= 0.5 || clip.h <= 0.5 {
            return;
        }
        let s = self.scale;
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c, clip: self.px(clip) });
    }

    /// Text with its vertical centre at `cy`; returns its width.
    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.put(out, k, x, cy - lh * 0.5, c, clip);
        w
    }

    /// Right-aligned at `rx`.
    #[allow(clippy::too_many_arguments)]
    fn label_r(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, rx: f32, cy: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.put(out, k, rx - w, cy - lh * 0.5, c, clip);
        w
    }

    /// A vertical hairline that fades in from both ends.
    fn vdivider(&self, out: &mut DrawList, x: f32, y: f32, h: f32, a: f32) {
        if a <= 0.002 || h <= 2.0 {
            return;
        }
        let half = h * 0.5;
        let r = |y, h| paint::rect(self.scale, Rect::new(x, y, 1.0, h), 0.0);
        out.shapes.push(r(y, half).gradient(HAIR.fade(0.0), HAIR, 0.0, 1.0).alpha(a).done());
        out.shapes.push(r(y + half, h - half).gradient(HAIR, HAIR.fade(0.0), 0.0, 1.0).alpha(a).done());
    }

    /// Horizontal bar: a thin track plus an iridescent (accent) fill of
    /// `v` (0..1).
    fn hbar(&self, out: &mut DrawList, r: Rect, v: f32, (ca, cb): (Rgba, Rgba), a: f32) {
        let sc = self.scale;
        out.shapes.push(paint::rect(sc, r, r.h * 0.5).fill(TRACK).alpha(a).done());
        let v = sat(v);
        if v > 0.002 {
            let w = (r.w * v).max(r.h);
            let f = paint::rect(sc, Rect::new(r.x, r.y, w, r.h), r.h * 0.5);
            let f = if theme::prism() { f.on_fill(1.0, 0.0, 0.0) } else { f.gradient(ca, ca.mix(cb, v), 1.0, 0.0) };
            out.shapes.push(f.alpha(a).done());
        }
    }

    // ── frame ───────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn draw_header(&mut self, fs: &mut FontSystem, out: &mut DrawList, ox: f32, oy: f32, w: f32, a: f32, clip: Rect) {
        let sc = self.scale;
        let sub = std::mem::take(&mut self.subtitle);
        self.label(fs, out, &sub, F_SUB, ox + 24.0 + w + 14.0, oy + 30.5, MUTED.fade(a), clip);
        self.subtitle = sub;
        let close = Rect::new(ox + FW - 46.0, oy + 15.0, 30.0, 30.0);
        self.hits.push((close, Ctl::Close));
        let ch = self.hover_of(Ctl::Close);
        if ch > 0.003 {
            out.shapes.push(paint::rect(sc, close, 15.0).fill(FAINT.fade(1.2)).alpha(a * ch).done());
        }
        let id = self.icons.id(&["window-close-symbolic"]);
        let c = SOFT.mix(TEXT, ch).fade(a);
        if c.3 > 0.003 {
            out.icons.push(IconDraw { id, x: (close.x + 7.0) * sc, y: (close.y + 7.0) * sc, size: (16.0 * sc).round(), color: c });
        }
    }

    fn draw_graph(&mut self, out: &mut DrawList, k: usize, g: Rect, a: f32) {
        let sc = self.scale;
        for f in [0.25, 0.5, 0.75] {
            let y = (g.y + g.h * f).round();
            out.shapes.push(paint::rect(sc, Rect::new(g.x, y, g.w, 1.0), 0.0).fill(Rgba::hex(0xffffff, 0.035 * a)).done());
        }
        if !self.tex_live || a <= 0.0 {
            return;
        }
        let p = self.scroll.get().clamp(0.0, 1.0);
        let n = HIST as f32 + 1.0;
        // The texture is painted at its own ceiling; shown against the
        // animated one it is scaled about the plot's bottom line (cropped
        // when it grows past the top), so a rescale glides, never jumps.
        let kf = if k == 0 { self.tex_ceil / self.ceil.get().max(0.05) } else { 1.0 };
        let (base, top) = (g.h - 1.0, 5.0);
        let (y0, h) = (g.y + base * (1.0 - kf), g.h * kf);
        let (ry0, ry1) = (y0.max(g.y), (y0 + h).min(g.y + g.h));
        if ry1 > ry0 + 0.1 {
            let mut img = ImageDraw::new(self.tex_keys[k], [g.x * sc, ry0 * sc, g.w * sc, (ry1 - ry0) * sc]);
            img.uv = [p / n, (ry0 - y0) / h, (HIST as f32 + p) / n, (ry1 - y0) / h];
            img.alpha = a;
            img.radius = 6.0 * sc;
            out.images.push(img);
        }
        // Live dot riding the right edge.
        let (pv, lv) = self.dots[k];
        let v = lerp(pv, lv, p) * kf;
        let y = (g.y + base - v * (base - top)).max(g.y + 3.0);
        let (aa, ab) = theme::accent();
        let c = if theme::prism() { stop_at(&graph_stops((aa, ab)), 1.0) } else if k == 0 { ab } else { aa };
        out.shapes.push(paint::circle(sc, g.x + g.w - 1.0, y, 3.4).fill(c).alpha(a).done());
        out.shapes.push(paint::circle(sc, g.x + g.w - 1.0, y, 1.4).fill(Rgba::hex(0xffffff, 0.95)).alpha(a).done());
    }

    #[allow(clippy::too_many_arguments)]
    fn big_value(&mut self, fs: &mut FontSystem, out: &mut DrawList, which: usize, x: f32, cy: f32, a: f32, clip: Rect) -> f32 {
        let sw = if which == 0 { &mut self.cpu_big } else { &mut self.mem_big };
        let (cur, old, t) = (std::mem::take(&mut sw.cur), std::mem::take(&mut sw.old), sat(sw.t.get()));
        let (k, w) = self.text.get(fs, &cur, F_BIG);
        let lh = self.text.line_h(F_BIG);
        self.big_w[which].set(w);
        if self.big_w[which].get() == 0.0 {
            self.big_w[which].snap(w);
        }
        // Cross-fade with a hint of vertical drift (2 px), no motion blur.
        self.put(out, k, x, cy - lh * 0.5 + (1.0 - t) * 2.0, TEXT.fade(a * t), clip);
        if t < 1.0 && !old.is_empty() {
            let (ko, _) = self.text.get(fs, &old, F_BIG);
            self.put(out, ko, x, cy - lh * 0.5 - t * 2.0, TEXT.fade(a * (1.0 - t)), clip);
        }
        let sw = if which == 0 { &mut self.cpu_big } else { &mut self.mem_big };
        (sw.cur, sw.old) = (cur, old);
        self.big_w[which].get()
    }

    fn draw_cpu_card(&mut self, fs: &mut FontSystem, out: &mut DrawList, c: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        self.label(fs, out, "CPU", F_SECTION, c.x + 16.0, c.y + 20.0, MUTED.fade(a), clip);
        if self.snap.is_none() {
            self.draw_graph(out, 0, Rect::new(c.x + GRAPH_IN, c.y + GRAPH_Y, c.w - 2.0 * GRAPH_IN, GRAPH_H), a);
            return;
        }
        let bw = self.big_value(fs, out, 0, c.x + 16.0, c.y + 50.0, a, clip);
        let lx = c.x + 16.0 + bw + 16.0;
        self.label(fs, out, "Load average", F_SMALL, lx, c.y + 41.0, MUTED.fade(a), clip);
        let load = std::mem::take(&mut self.load_txt);
        self.label(fs, out, &load, F_NUM, lx, c.y + 58.0, SOFT.fade(a), clip);
        self.load_txt = load;
        // Per-core bars, bottom-aligned, top right.
        let n = self.cores.len();
        if n > 0 {
            let gap = if n > 24 { 2.0 } else { 3.0 };
            let bw = ((196.0 + gap) / n as f32 - gap).clamp(2.0, 8.0);
            let total = n as f32 * (bw + gap) - gap;
            let (x0, y0, h) = (c.x + c.w - 16.0 - total, c.y + 16.0, 46.0);
            let (aa, ab) = theme::accent();
            for (i, s) in self.cores.iter().enumerate() {
                let x = x0 + i as f32 * (bw + gap);
                out.shapes.push(paint::rect(sc, Rect::new(x, y0, bw, h), bw * 0.4).fill(Rgba::hex(0xffffff, 0.05)).alpha(a).done());
                let v = sat(s.get());
                let fh = (h * v).max(bw.min(3.0));
                // Meters fill bottom-up through the iridescence.
                let f = paint::rect(sc, Rect::new(x, y0 + h - fh, bw, fh), bw * 0.4);
                let f = if theme::prism() { f.on_fill(0.0, -1.0, 0.0) } else { f.gradient(ab, aa, 0.0, 1.0) };
                out.shapes.push(f.alpha(a * (0.45 + 0.55 * v)).done());
            }
        }
        let g = Rect::new(c.x + GRAPH_IN, c.y + GRAPH_Y, c.w - 2.0 * GRAPH_IN, GRAPH_H);
        self.draw_graph(out, 0, g, a);
        // The scale label cross-fades in place when the ceiling changes.
        let (cur, old) = (std::mem::take(&mut self.ceil_txt.cur), std::mem::take(&mut self.ceil_txt.old));
        let t = sat(self.ceil_txt.t.get());
        self.label(fs, out, &cur, F_TINY, g.x + 2.0, g.y + 5.0, DIM.fade(a * t), clip);
        if t < 1.0 && !old.is_empty() {
            self.label(fs, out, &old, F_TINY, g.x + 2.0, g.y + 5.0, DIM.fade(a * (1.0 - t)), clip);
        }
        (self.ceil_txt.cur, self.ceil_txt.old) = (cur, old);
    }

    fn draw_mem_card(&mut self, fs: &mut FontSystem, out: &mut DrawList, c: Rect, a: f32, clip: Rect) {
        self.label(fs, out, "MEMORY", F_SECTION, c.x + 16.0, c.y + 20.0, MUTED.fade(a), clip);
        if self.snap.is_some() {
            let bw = self.big_value(fs, out, 1, c.x + 16.0, c.y + 50.0, a, clip);
            let of = std::mem::take(&mut self.mem_of);
            self.label(fs, out, &of, F_SMALL, c.x + 16.0 + bw + 7.0, c.y + 54.0, MUTED.fade(a), clip);
            self.mem_of = of;
            let rx = c.x + c.w - 16.0;
            self.label_r(fs, out, "SWAP", F_SECTION, rx, c.y + 20.0, MUTED.fade(a), clip);
            let sw = std::mem::take(&mut self.swap_txt);
            self.label_r(fs, out, &sw, F_NUM, rx, c.y + 41.0, SOFT.fade(a), clip);
            self.swap_txt = sw;
            let (aa, ab) = theme::accent();
            self.hbar(out, Rect::new(rx - 84.0, c.y + 56.0, 84.0, 4.0), self.swap_bar.get(), (ab, aa), a);
        }
        self.draw_graph(out, 1, Rect::new(c.x + GRAPH_IN, c.y + GRAPH_Y, c.w - 2.0 * GRAPH_IN, GRAPH_H), a);
    }

    fn draw_table(&mut self, fs: &mut FontSystem, out: &mut DrawList, t: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        let (aa, ab) = theme::accent();
        self.table_rect = Rect::new(t.x, t.y + ROWS_TOP, t.w, ROWS as f32 * ROW_H);
        // Header: the active column wears a pill that slides between
        // columns, with a three-bar glyph that morphs between ↓ and ↑.
        // Labels sit flush with their column's edge; the room for the glyph
        // is on the inner side (right of NAME, left of the numbers).
        let hy = t.y + 20.0;
        let glyph = 10.0;
        let (pl, pr) = (t.x + self.sort_l.get(), t.x + self.sort_r.get());
        for (i, s) in SORTS.iter().enumerate() {
            let (name, right, x) = match s {
                Sort::Name => ("NAME", false, t.x + NAME_X),
                Sort::Pid => ("PID", true, t.x + PID_R),
                Sort::Cpu => ("CPU", true, t.x + CPU_R),
                Sort::Mem => ("MEMORY", true, t.x + MEM_R),
            };
            let (k, w) = self.text.get(fs, name, F_SECTION);
            let gw = w + 6.0 + glyph;
            let (bx, tx) = if right { (x - gw, x - w) } else { (x, x) };
            let r = Rect::new(bx - 9.0, hy - 11.0, gw + 18.0, 22.0);
            self.head_rects[i] = r;
            self.hits.push((r, Ctl::Head(*s)));
            let active = *s == self.sort;
            if active {
                // Kept relative to the table, so only a column change moves it.
                let (l, rr) = (r.x - t.x, r.x + r.w - t.x);
                if self.sort_r.get() == 0.0 {
                    self.sort_l.snap(l);
                    self.sort_r.snap(rr);
                } else if (self.sort_l.target - l).abs() > 0.5 {
                    let right = l > self.sort_l.target;
                    let (lead, trail) = (SpringCfg::new(0.3, 0.74), SpringCfg::new(0.5, 0.86));
                    self.sort_l.cfg = if right { trail } else { lead };
                    self.sort_r.cfg = if right { lead } else { trail };
                }
                self.sort_l.set(l);
                self.sort_r.set(rr);
            }
            let h = self.hover_of(Ctl::Head(*s));
            // Lit by how much of it the pill covers right now.
            let cover = sat((pr.min(tx + w) - pl.max(tx)) / w.max(1.0));
            let col = MUTED.mix(SOFT, h).mix(TEXT, cover);
            let lh = self.text.line_h(F_SECTION);
            self.put(out, k, tx, hy - lh * 0.5, col.fade(a), clip);
            if !active && h > 0.01 {
                out.shapes.push(paint::rect(sc, r, 11.0).fill(Rgba::hex(0xffffff, 0.045 * h)).alpha(a).done());
            }
        }
        let pill = Rect::new(pl, hy - 11.0, pr - pl, 22.0);
        out.shapes.push(paint::rect(sc, pill, 11.0).fill(FAINT).alpha(a).done());
        let d = sat(self.sort_dir.get());
        // The glyph shows once the pill has (nearly) its width again.
        let want = self.sort_r.target - self.sort_l.target;
        let ga = a * sat(1.0 - (pill.w - want).abs() / 24.0);
        let gx = if self.sort == Sort::Name { pill.x + pill.w - 9.0 - glyph } else { pill.x + 9.0 };
        for j in 0..3 {
            let wd = lerp([10.0, 7.0, 4.0][j], [4.0, 7.0, 10.0][j], d);
            let y = hy - 4.5 + j as f32 * 3.6;
            out.shapes.push(paint::rect(sc, Rect::new(gx, y, wd, 1.8), 0.9).on_fill(1.0, 0.0, 0.0).alpha(ga).done());
        }
        paint::divider(&mut out.shapes, sc, t.x + 6.0, t.y + 37.0, t.w - 12.0, a);

        let rows_area = Rect::new(t.x, t.y + ROWS_TOP - 1.0, t.w, ROWS as f32 * ROW_H + 2.0);
        let rclip = Rect::new(clip.x, rows_area.y, clip.w, rows_area.h);
        let top = t.y + ROWS_TOP;

        if self.snap.is_none() || self.matches == 0 {
            let msg = if self.snap.is_none() {
                "Reading processes…".to_string()
            } else {
                format!("No processes match “{}”", self.filter)
            };
            let (k, w) = self.text.get(fs, &msg, F_SUB);
            let lh = self.text.line_h(F_SUB);
            let ea = if self.snap.is_none() { 1.0 } else { 1.0 - self.rows.values().map(|r| sat(r.appear.get())).fold(0.0, f32::max) };
            self.put(out, k, t.x + (t.w - w) * 0.5, top + ROWS as f32 * ROW_H * 0.5 - lh * 0.5 - 8.0, MUTED.fade(a * ea), rclip);
        }

        // Selection highlight: glides between rows, then rides its row.
        if let Some(sel) = self.selected
            && let Some(r) = self.rows.get(&sel)
        {
            self.sel_last = lerp(self.sel_from, r.idx.get(), sat(self.sel_t.get()));
        }
        let sa = sat(self.sel_a.get());
        let pos_fade = |idx: f32| sat((ROWS as f32 - 0.5 - idx) * 2.0);
        // Once settled, the highlight is drawn with its row (so a row
        // sliding over it covers it); while gliding or fading, first.
        let sel_live = self.selected.is_some_and(|p| self.rows.contains_key(&p)) && self.sel_t.get() >= 0.999;
        if sa > 0.003 && !sel_live {
            self.draw_sel(out, t, top, sa * pos_fade(self.sel_last) * a);
        }
        let rows = std::mem::take(&mut self.rows);
        let mut ids = std::mem::take(&mut self.scratch);
        ids.clear();
        ids.extend(rows.keys().copied());
        // Stacking is fixed per move (decided in relayout): collapsing rows
        // at the bottom, then still rows, fallers, and climbers on top.
        ids.sort_unstable_by_key(|p| (rows[p].z, rows[p].state != RowState::Gone, *p));
        for (n, pid) in ids.iter().enumerate() {
            let r = &rows[pid];
            let idx = r.idx.get();
            let app = sat(r.appear.get());
            let ra = a * pos_fade(idx) * app * app;
            if ra <= 0.003 {
                continue;
            }
            let y = top + idx * ROW_H;
            let cy = y + ROW_H * 0.5;
            let row = Rect::new(t.x + 6.0, y + 1.0, t.w - 12.0, ROW_H - 2.0);
            let ctl = Ctl::Row(*pid);
            // Hit-tested at full strength: a dimmed (signalled) row can
            // still be picked again to force it.
            if r.state == RowState::Live && ra > 0.5 {
                self.hits.push((row, ctl));
            }
            let ra = ra * (1.0 - 0.55 * sat(r.dim.get()));
            // Rows stacked above this one hide what they cover of it: text
            // and icons are cut at their edge, like cards passing over.
            let (mut lo, mut hi) = (y, y + ROW_H);
            for o in &ids[n + 1..] {
                let ro = &rows[o];
                if ro.appear.get() < 0.5 {
                    continue;
                }
                let oy = top + ro.idx.get() * ROW_H;
                if oy + ROW_H <= lo || oy >= hi {
                    continue;
                }
                if oy <= y {
                    lo = lo.max(oy + ROW_H);
                } else {
                    hi = hi.min(oy);
                }
            }
            let (lo, hi) = (lo.max(rclip.y), hi.min(rclip.y + rclip.h));
            let tclip = Rect::new(rclip.x, lo, rclip.w, (hi - lo).max(0.0));
            // Enter: slide 10 px in from the left and scale up; collapse:
            // shrink toward the row's centre line.
            let gone = r.state == RowState::Gone;
            let dx = if gone { 0.0 } else { -(1.0 - app) * 10.0 };
            let ky = if gone { app } else { 0.6 + 0.4 * app };
            let h = self.hover_of(ctl);
            let selected = self.selected == Some(*pid);
            let travel = (r.idx.get() - r.idx.target).abs();
            if travel > 0.004 {
                // A moving row carries an opaque backing (and a shadow while
                // it travels far), so whatever it passes over is covered.
                let lift = sat(travel * 1.6);
                out.shapes.push(
                    paint::rect(sc, row, 10.0)
                        .fill(LIFT)
                        .shadow(Rgba::hex(0x000000, 0.55 * lift), 9.0, 3.0)
                        .alpha(ra * sat(travel * 12.0))
                        .done(),
                );
            }
            if selected && sel_live {
                self.draw_sel(out, t, top, sa * ra);
            }
            if h > 0.01 && !selected {
                out.shapes.push(paint::rect(sc, row, 10.0).fill(Rgba::hex(0xffffff, 0.045 * h)).alpha(ra).done());
            }
            // App icon (or a monogram tile when the app has none).
            let ts = ICON * ky;
            let tile = Rect::new(t.x + 16.0 + dx + (ICON - ts) * 0.5, cy - ts * 0.5, ts, ts);
            let sel_k = if selected { sa } else { 0.0 };
            match self.icon_keys.get(&r.name) {
                Some(&(key, _)) if self.tex_live => {
                    if sel_k > 0.01 {
                        out.shapes.push(
                            paint::rect(sc, tile.inflate(2.5), 8.0 * ky)
                                .rim(1.2, aa.fade(0.75), ab.fade(0.75))
                                .alpha(ra * sel_k)
                                .done(),
                        );
                    }
                    let (c0, c1) = (tile.y.max(lo), (tile.y + ts).min(hi));
                    if c1 > c0 + 0.1 {
                        let mut img = ImageDraw::new(key, [tile.x * sc, c0 * sc, ts * sc, (c1 - c0) * sc]);
                        img.uv = [0.0, (c0 - tile.y) / ts, 1.0, (c1 - tile.y) / ts];
                        img.alpha = ra;
                        out.images.push(img);
                    }
                }
                _ => {
                    out.shapes.push(
                        paint::rect(sc, tile, 6.0 * ky)
                            .on_mix(FAINT, sel_k, paint::IRID.0, paint::IRID.1, 0.1)
                            .alpha(ra)
                            .done(),
                    );
                    let ch = r.name.chars().find(|c| c.is_alphanumeric()).unwrap_or('?').to_ascii_uppercase();
                    let mut b = [0u8; 4];
                    let (k, w) = self.text.get(fs, ch.encode_utf8(&mut b), F_MONO);
                    let lh = self.text.line_h(F_MONO);
                    self.put(out, k, tile.x + (ts - w) * 0.5, cy - lh * 0.5, SOFT.mix(theme::on_ink(), sel_k).fade(ra * ky), tclip);
                }
            }
            // Name and pid.
            let (k, _) = self.text.get_fit(fs, &r.name, F_ROW, PID_R - NAME_X - 70.0);
            let lh = self.text.line_h(F_ROW);
            self.put(out, k, t.x + NAME_X + dx, cy - lh * 0.5, TEXT.fade(ra), tclip);
            self.label_r(fs, out, &r.pid_txt, F_NUM, t.x + PID_R + dx, cy, MUTED.fade(ra), tclip);
            // Bars and values; values cross-fade in place.
            let bh = 4.0 * ky;
            self.hbar(out, Rect::new(t.x + CPU_BAR + dx, cy - bh * 0.5, BAR_W, bh), r.cpu_bar.get(), (aa, ab), ra);
            self.hbar(out, Rect::new(t.x + MEM_BAR + dx, cy - bh * 0.5, BAR_W, bh), r.mem_bar.get(), (ab, aa), ra);
            let xf = sat(r.xfade.get());
            let cpu_col = if r.cpu < 0.05 { DIM } else { SOFT };
            self.label_r(fs, out, &r.cpu_txt, F_NUM, t.x + CPU_R + dx, cy, cpu_col.fade(ra * xf), tclip);
            self.label_r(fs, out, &r.mem_txt, F_NUM, t.x + MEM_R + dx, cy, SOFT.fade(ra * xf), tclip);
            if xf < 1.0 {
                self.label_r(fs, out, &r.old_cpu, F_NUM, t.x + CPU_R + dx, cy, cpu_col.fade(ra * (1.0 - xf)), tclip);
                self.label_r(fs, out, &r.old_mem, F_NUM, t.x + MEM_R + dx, cy, SOFT.fade(ra * (1.0 - xf)), tclip);
            }
        }
        self.scratch = ids;
        self.rows = rows;
    }

    fn draw_sel(&self, out: &mut DrawList, t: Rect, top: f32, pa: f32) {
        let sc = self.scale;
        let y = top + self.sel_last * ROW_H;
        let r = Rect::new(t.x + 6.0, y + 1.0, t.w - 12.0, ROW_H - 2.0);
        // Faint white, an iridescent tick at the row's start.
        out.shapes.push(paint::rect(sc, r, 10.0).fill(FAINT).alpha(pa).done());
        out.shapes.push(paint::rect(sc, Rect::new(r.x + 4.0, r.y + r.h * 0.5 - 7.0, 3.0, 14.0), 1.5).on_fill(0.0, 1.0, 0.0).alpha(pa).done());
    }

    fn draw_footer(&mut self, fs: &mut FontSystem, out: &mut DrawList, f: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        let (aa, _) = theme::accent();
        let cy = f.y + f.h * 0.5;
        // Search field.
        let s = Rect::new(f.x, f.y, 300.0, f.h);
        self.hits.push((s, Ctl::Search));
        let hs = self.hover_of(Ctl::Search);
        let active = !self.filter.is_empty();
        // A faint pill; filtering lights the prism hairline around it.
        out.shapes.push(
            paint::rect(sc, s, s.h * 0.5)
                .fill(Rgba::hex(0xffffff, 0.05 + 0.025 * hs))
                .prism(paint::HAIR, if active { 1.6 } else { 0.0 })
                .alpha(a)
                .done(),
        );
        let gc = if active { aa } else { MUTED };
        out.shapes.push(paint::circle(sc, s.x + 17.5, cy - 1.5, 5.0).rim(1.6, gc, gc).alpha(a).done());
        for (o, rad) in [(4.6, 1.1), (6.4, 1.1)] {
            out.shapes.push(paint::circle(sc, s.x + 17.5 + o, cy - 1.5 + o, rad).fill(gc).alpha(a).done());
        }
        let fclip = Rect::new(s.x + 30.0, s.y, s.w - 40.0, s.h);
        if active {
            let filt = std::mem::take(&mut self.filter);
            let w = self.label(fs, out, &filt, F_ROW, s.x + 32.0, cy, TEXT.fade(a), fclip);
            self.filter = filt;
            out.shapes.push(paint::rect(sc, Rect::new(s.x + 33.0 + w, cy - 8.0, 1.6, 16.0), 0.8).on_fill(0.0, 1.0, 0.0).alpha(a).done());
            let mt = std::mem::take(&mut self.match_txt);
            self.label_r(fs, out, &mt, F_SMALL, s.x + s.w - 12.0, cy, MUTED.fade(a), clip);
            self.match_txt = mt;
        } else {
            self.label(fs, out, "Type to filter", F_ROW, s.x + 32.0, cy, DIM.mix(MUTED, hs).fade(a), fclip);
        }

        let right = f.x + f.w;
        // Summary (nothing selected).
        let sum_a = sat((self.sum_a.get() - 0.4) / 0.6) * a;
        if sum_a > 0.003 {
            let sm = std::mem::take(&mut self.summary);
            self.label_r(fs, out, &sm, F_SMALL, right - 4.0, cy, MUTED.fade(sum_a), clip);
            self.summary = sm;
        }
        // Actions for the selection: they slide in from the right.
        let act = sat(self.act_a.get());
        let shift = (1.0 - act) * 18.0;
        let kill = Rect::new(right - 112.0 + shift, f.y, 112.0, f.h);
        let end = Rect::new(kill.x - 8.0 - 72.0, f.y, 72.0, f.h);
        let aa_act = sat((act - 0.35) / 0.65) * a;
        if self.selected.is_some() && act > 0.5 {
            self.hits.push((end, Ctl::End));
            self.hits.push((kill, Ctl::Kill));
        }
        if aa_act > 0.003 {
            let he = self.hover_of(Ctl::End);
            out.shapes.push(
                paint::rect(sc, end, end.h * 0.5)
                    .fill(Rgba::hex(0xffffff, 0.07 + 0.06 * he))
                    .alpha(aa_act)
                    .done(),
            );
            let (k, w) = self.text.get(fs, "End", F_BTN);
            let lh = self.text.line_h(F_BTN);
            self.put(out, k, end.x + (end.w - w) * 0.5, cy - lh * 0.5, TEXT.fade(aa_act), clip);
            let hk = self.hover_of(Ctl::Kill);
            let arm = sat(self.arm_t.get());
            out.shapes.push(
                paint::rect(sc, kill, kill.h * 0.5)
                    .fill(BAD.fade(0.14 + 0.08 * hk))
                    .alpha(aa_act)
                    .done(),
            );
            // Arming grows a solid red fill out of the click point; each
            // label keeps its own contrast and is cut at the fill's edge, so
            // one of them is always fully readable.
            let o = kill.x + kill.w * self.arm_x;
            let (fl, fr) = (lerp(o, kill.x, arm), lerp(o, kill.x + kill.w, arm));
            if fr - fl > 0.5 {
                out.shapes.push(
                    paint::rect(sc, Rect::new(fl, kill.y, fr - fl, kill.h), (kill.h * 0.5).min((fr - fl) * 0.5))
                        .fill(BAD)
                        .alpha(aa_act)
                        .done(),
                );
            }
            let band = |x0: f32, x1: f32| Rect::new(x0, kill.y, (x1 - x0).max(0.0), kill.h);
            let (k, w) = self.text.get(fs, "Force quit", F_BTN);
            let x = kill.x + (kill.w - w) * 0.5;
            self.put(out, k, x, cy - lh * 0.5, BAD.fade(aa_act), band(kill.x, fl));
            self.put(out, k, x, cy - lh * 0.5, BAD.fade(aa_act), band(fr, kill.x + kill.w));
            let (k, w) = self.text.get(fs, "Confirm kill", F_BTN);
            self.put(out, k, kill.x + (kill.w - w) * 0.5, cy - lh * 0.5, INK.fade(aa_act), band(fl, fr));
        }
        // Selected process / status message, right-aligned to the buttons.
        let st = sat(self.stat_a.get());
        let lr = lerp(right - 4.0, end.x - 16.0, act);
        let name_a = aa_act * sat(1.0 - st * 1.6);
        if name_a > 0.003 {
            let pid = std::mem::take(&mut self.sel_pid);
            let pw = self.label_r(fs, out, &pid, F_SMALL, lr, cy, MUTED.fade(name_a), clip);
            self.sel_pid = pid;
            let nm = std::mem::take(&mut self.sel_name);
            let (k, w) = self.text.get_fit(fs, &nm, F_ROW, 180.0);
            self.sel_name = nm;
            let lh = self.text.line_h(F_ROW);
            self.put(out, k, lr - pw - 10.0 - w, cy - lh * 0.5, TEXT.fade(name_a), clip);
        }
        if st > 0.003
            && let Some((msg, ok)) = self.status.take()
        {
            let col = if ok { GOOD } else { BAD };
            let sa = sat((st - 0.3) / 0.7) * a;
            let w = self.label_r(fs, out, &msg, F_SMALL, lr, cy, SOFT.fade(sa), clip);
            out.shapes.push(paint::circle(sc, lr - w - 11.0, cy, 3.0).fill(col).alpha(sa).done());
            self.status = Some((msg, ok));
        }
    }
}

impl Monitor {
    /// Give back every texture this window uploaded (graphs, app icons).
    fn free_textures(&mut self, frees: &mut Vec<u64>) {
        if self.tex_live {
            frees.extend_from_slice(&self.tex_keys);
            frees.extend(self.icon_keys.values().map(|k| k.0));
            self.tex_live = false;
        }
        self.icon_keys.clear();
        self.uploads.clear();
    }
}

impl WinScene for Monitor {
    fn size(&self) -> (f32, f32) {
        (SW, SH)
    }
    fn placement(&self) -> Placement {
        Placement::Center
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }

    fn apply(&mut self, f: &Fact) {
        self.on_fact(f);
    }

    /// Only this window's own snapshots matter (a monitor on another
    /// output has its own id).
    fn on_fact(&mut self, f: &Fact) -> bool {
        if let Fact::Custom(c) = f
            && !self.chrome.closing
            && let Ok(s) = c.clone().downcast::<Snapshot>()
            && s.id == self.id
        {
            self.take_snapshot(s);
            return true;
        }
        false
    }

    fn set_scale(&mut self, s: f32) {
        self.scale = s;
        self.text.set_scale(s);
        self.scale_bits.store(s.to_bits(), Ordering::Relaxed);
    }

    fn step(&mut self, dt: f32) -> bool {
        let mut m = self.chrome.step(dt);
        let now = self.chrome.time;
        let sc = self.scale;
        // Stamp what input asked for since the last frame.
        if self.armed == Some(PENDING) {
            self.armed = Some(now);
        }
        if self.status_until == PENDING {
            self.status_until = now + STATUS_TIME;
        }
        if self.left_at == Some(PENDING) {
            self.left_at = Some(now);
        }
        if let Some(at) = self.armed {
            if now >= at + ARM_TIMEOUT {
                self.armed = None;
            } else {
                self.chrome.wake(at + ARM_TIMEOUT);
            }
        }
        // The rank hold ends a moment after the pointer left the table;
        // then the ranks it held back are applied.
        if let Some(at) = self.left_at {
            if now >= at + HOLD_AFTER {
                self.left_at = None;
            } else {
                self.chrome.wake(at + HOLD_AFTER);
            }
        }
        if self.rerank && !self.holding() {
            self.relayout(Why::Release);
        }
        let stat_on = self.status.is_some() && now < self.status_until;
        if stat_on {
            self.chrome.wake(self.status_until);
        }
        let sel = self.selected.is_some();
        self.stat_a.set(if stat_on { 1.0 } else { 0.0 });
        self.act_a.set(if sel { 1.0 } else { 0.0 });
        self.sel_a.set(if sel { 1.0 } else { 0.0 });
        self.sum_a.set(if sel || stat_on { 0.0 } else { 1.0 });
        self.arm_t.set(if self.armed.is_some() { 1.0 } else { 0.0 });
        self.sort_dir.set(if self.desc { 0.0 } else { 1.0 });
        let closing = self.chrome.closing;
        if closing {
            self.reveal.set(0.0);
        } else if self.chrome.open_w.get() >= 0.92 && self.chrome.open_h.get() >= 0.92 {
            self.reveal.set(1.0);
        }
        if self.hovered != Ctl::None && self.hovered != Ctl::Body {
            self.hover.entry(self.hovered).or_insert(spring0());
        }
        for (c, h) in self.hover.iter_mut() {
            h.set(if self.hovered == *c && !closing { 1.0 } else { 0.0 });
        }
        // Each spring stops once what is left of it is under a quarter of
        // a device pixel (see `settle`), so the window really goes idle
        // between samples.
        let graph_px = (CPU_W - 2.0 * GRAPH_IN) * sc / HIST as f32;
        for (s, ppu) in [
            (&mut self.scroll, graph_px),
            (&mut self.ceil, GRAPH_H * sc * 4.0),
            (&mut self.swap_bar, 84.0 * sc),
            (&mut self.sort_l, sc),
            (&mut self.sort_r, sc),
            (&mut self.sort_dir, 10.0 * sc),
            (&mut self.sel_t, ROW_H * ROWS as f32 * sc),
            (&mut self.sel_a, 255.0),
            (&mut self.sum_a, 255.0),
            (&mut self.act_a, 255.0),
            (&mut self.stat_a, 255.0),
            (&mut self.arm_t, 112.0 * sc),
            (&mut self.reveal, 255.0),
            (&mut self.cpu_big.t, 255.0),
            (&mut self.mem_big.t, 255.0),
            (&mut self.ceil_txt.t, 255.0),
        ] {
            m |= s.step(dt);
            settle(s, ppu);
        }
        for s in self.cores.iter_mut() {
            m |= s.step(dt);
            settle(s, 46.0 * sc);
        }
        for s in self.big_w.iter_mut() {
            m |= s.step(dt);
            settle(s, sc);
        }
        for s in self.hover.values_mut() {
            m |= s.step(dt);
            settle(s, 255.0);
        }
        let hovered = self.hovered;
        self.hover.retain(|c, s| !s.at_rest() || s.target > 0.0 || *c == hovered);
        for r in self.rows.values_mut() {
            if r.ended == Some(PENDING) {
                r.ended = Some(now);
            }
            if let Some(at) = r.ended {
                // Still running long after the signal: it ignored it.
                if now >= at + ENDED_TIME {
                    r.ended = None;
                } else {
                    self.chrome.wake(at + ENDED_TIME);
                }
            }
            r.dim.set(if r.ended.is_some() { 1.0 } else { 0.0 });
            for (s, ppu) in [
                (&mut r.idx, ROW_H * sc),
                (&mut r.appear, 255.0),
                (&mut r.cpu_bar, BAR_W * sc),
                (&mut r.mem_bar, BAR_W * sc),
                (&mut r.xfade, 255.0),
                (&mut r.dim, 255.0),
            ] {
                m |= s.step(dt);
                settle(s, ppu);
            }
            if r.idx.at_rest() && r.state == RowState::Live {
                r.z = 0;
            }
        }
        self.rows.retain(|_, r| match r.state {
            RowState::Live => true,
            RowState::Leaving => !r.idx.at_rest(),
            RowState::Gone => !(r.appear.at_rest() && r.appear.target <= 0.0),
        });
        if !stat_on && self.stat_a.at_rest() && self.stat_a.get() <= 0.0 {
            self.status = None;
        }
        m
    }

    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        let sc = self.scale;
        if self.chrome.done() {
            // Last frame before the surface goes: release the textures.
            self.free_textures(&mut out.frees);
        } else if !self.uploads.is_empty() {
            out.uploads.append(&mut self.uploads);
            self.tex_live = true;
        }
        let r = self.chrome.morph();
        self.chrome.body = r;
        let open = self.chrome.openness();
        let ob = sat(open);
        out.shapes.push(
            paint::ink(sc, r, RADIUS.min(r.h * 0.5))
                .shadow(SHADOW, 18.0 + 12.0 * ob, 6.0 + 8.0 * ob)
                .done(),
        );
        out.glass(r, RADIUS.min(r.h * 0.5), 1.0);
        self.hits.push((r, Ctl::Body));
        let clip = r.inflate(-1.5);
        // Content is laid out at its home size around the body's centre and
        // only shows once the slab (nearly) has that size, so nothing ever
        // reaches past its edges. It arrives in sections (header, graphs,
        // table, footer), each lifting into place.
        let (ox, oy) = (r.x + (r.w - FW) * 0.5, r.y + (r.h - FH) * 0.5);
        let fit = sat((open - 0.955) / 0.035);
        let rv = self.reveal.get();
        let stage = |k: f32| sat((rv - 0.08 * k) / 0.5) * fit;
        let lift = |a: f32| (1.0 - a) * 10.0;

        // The title is the one thing on the folded slab; it travels from
        // the slab's centre to its place in the header (and back on close).
        let (kt, tw) = self.text.get(fs, "Processes", F_TITLE);
        let lh = self.text.line_h(F_TITLE);
        let m = sat((open - 0.3) / 0.62);
        let m = m * m * (3.0 - 2.0 * m);
        let tx = lerp(r.x + (r.w - tw) * 0.5, ox.max(r.x) + 24.0, m);
        let ty = lerp(r.y + r.h * 0.5, oy.max(r.y) + 30.0, m);
        self.put(out, kt, tx, ty - lh * 0.5, TEXT.fade(sat(open * 4.0)), clip);
        let a0 = stage(0.0);
        if a0 > 0.0 {
            self.draw_header(fs, out, ox, oy + lift(a0), tw, a0, clip);
        }
        let a1 = stage(1.0);
        if a1 > 0.0 {
            let y = oy + CARD_Y + lift(a1);
            // Open sections, split by hairlines.
            paint::divider(&mut out.shapes, sc, ox + PAD, y - 6.0, FW - 2.0 * PAD, a1);
            self.vdivider(out, ox + PAD + CPU_W + GAP * 0.5, y + 14.0, CARD_H - 28.0, a1);
            paint::divider(&mut out.shapes, sc, ox + PAD, y + CARD_H + 6.0, FW - 2.0 * PAD, a1);
            self.draw_cpu_card(fs, out, Rect::new(ox + PAD, y, CPU_W, CARD_H), a1, clip);
            self.draw_mem_card(fs, out, Rect::new(ox + PAD + CPU_W + GAP, y, MEM_W, CARD_H), a1, clip);
        }
        let a2 = stage(2.0);
        if a2 > 0.0 {
            self.draw_table(fs, out, Rect::new(ox + PAD, oy + TABLE_Y + lift(a2), FW - 2.0 * PAD, TABLE_H), a2, clip);
        } else {
            self.table_rect = Rect::default();
        }
        let a3 = stage(3.0);
        if a3 > 0.0 {
            paint::divider(&mut out.shapes, sc, ox + PAD, oy + FOOT_Y - 5.0 + lift(a3), FW - 2.0 * PAD, a3);
            self.draw_footer(fs, out, Rect::new(ox + PAD, oy + FOOT_Y + lift(a3), FW - 2.0 * PAD, FOOT_H), a3, clip);
        }
        self.text.end_frame();
        let (dx, dy) = self.chrome.translate(out, sc);
        if dx != 0.0 || dy != 0.0 {
            for (h, _) in &mut self.hits {
                h.x += dx;
                h.y += dy;
            }
            self.table_rect.x += dx;
            self.table_rect.y += dy;
        }
        // Rows move under a still pointer: keep the hover honest.
        if let Some((x, y)) = self.mouse
            && !self.chrome.grabbing
        {
            self.hovered = self.hit(x, y);
        }
    }

    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        self.mouse = Some((x, y));
        if !self.chrome.grabbing {
            self.hovered = self.hit(x, y);
        }
        let inside = self.table_rect.contains(x, y);
        if inside {
            self.left_at = None;
        } else if self.in_table {
            self.left_at = Some(PENDING);
        }
        self.in_table = inside;
        None
    }

    fn pointer_leave(&mut self) {
        self.mouse = None;
        if !self.chrome.grabbing {
            self.hovered = Ctl::None;
        }
        if self.in_table {
            self.in_table = false;
            self.left_at = Some(PENDING);
        }
    }

    fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        self.mouse = Some((x, y));
        self.hovered = self.hit(x, y);
        self.pressed = self.hovered;
        if !self.chrome.closing && self.hovered == Ctl::Body {
            self.chrome.start_grab();
        }
        None
    }

    fn release(&mut self) -> (Option<Action>, bool) {
        if self.chrome.grabbing {
            self.chrome.end_grab();
            self.pressed = Ctl::None;
            return (None, false);
        }
        let c = std::mem::replace(&mut self.pressed, Ctl::None);
        if c != self.hovered || self.chrome.closing {
            return (None, false);
        }
        match c {
            Ctl::Close => return (None, true),
            Ctl::Head(s) => self.set_sort(s),
            Ctl::Row(pid) => self.select(if self.selected == Some(pid) { None } else { Some(pid) }),
            Ctl::End => self.signal(false),
            Ctl::Kill => {
                if self.armed.is_some() {
                    self.signal(true);
                } else {
                    // The confirm fill grows from where the click landed.
                    let at = match (self.mouse, self.hits.iter().rev().find(|(_, h)| *h == Ctl::Kill)) {
                        (Some((x, _)), Some((r, _))) => (x - r.x) / r.w.max(1.0),
                        _ => 0.5,
                    };
                    self.arm(at);
                }
            }
            _ => {}
        }
        (None, false)
    }

    fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        match k.sym {
            keys::ESCAPE => {
                if self.armed.is_some() {
                    self.armed = None;
                } else if !self.filter.is_empty() {
                    self.filter.clear();
                    self.relayout(Why::Filter);
                } else if self.selected.is_some() {
                    self.select(None);
                } else {
                    return (None, true);
                }
            }
            keys::BACKSPACE => {
                if k.ctrl {
                    self.filter.clear();
                } else {
                    self.filter.pop();
                }
                self.relayout(Why::Filter);
            }
            keys::UP => self.move_sel(-1),
            keys::DOWN => self.move_sel(1),
            keys::HOME => self.move_sel(-(ROWS as i32)),
            keys::END => self.move_sel(ROWS as i32),
            keys::DELETE if self.selected.is_some() => {
                if !k.shift {
                    self.signal(false);
                } else if self.armed.is_some() {
                    self.signal(true);
                } else {
                    self.arm(0.5);
                }
            }
            keys::DELETE => {}
            _ => {
                if let Some(t) = &k.text
                    && !k.ctrl
                    && !k.alt
                    && !t.is_empty()
                    && t.chars().all(|c| !c.is_control())
                    && self.filter.chars().count() < 32
                {
                    self.filter.push_str(t);
                    self.relayout(Why::Filter);
                }
            }
        }
        (None, false)
    }

    /// Held keys repeat, except Delete: ending or killing a process (and
    /// confirming a kill) always takes a deliberate press.
    fn key_repeat(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if k.sym == keys::DELETE {
            return (None, false);
        }
        self.key(k)
    }

    fn cursor(&self) -> Cursor {
        if self.chrome.grabbing {
            return Cursor::Grabbing;
        }
        match self.hovered {
            Ctl::Head(_) | Ctl::Row(_) | Ctl::Close | Ctl::End | Ctl::Kill => Cursor::Pointer,
            Ctl::Search => Cursor::Text,
            _ => Cursor::Default,
        }
    }

    fn close(&mut self) {
        self.chrome.close();
        self.armed = None;
        // Stop sampling right away; the window is only folding now.
        self.worker = None;
    }

    fn reopen(&mut self) {
        self.chrome.reopen();
        if self.spawn && self.worker.is_none() {
            self.worker = Worker::spawn(self.id, self.tx.clone(), self.scale_bits.clone(), self.painter.clone());
        }
    }

    fn release_textures(&mut self, frees: &mut Vec<u64>) {
        self.worker = None;
        self.free_textures(frees);
    }
}

// ── demo ────────────────────────────────────────────────────────────────

const DEMO_PROCS: [(u32, &str, f32, f32); 24] = [
    (5188, "rustc", 88.0, 1100.0),
    (5121, "cargo", 34.0, 280.0),
    (2841, "firefox", 18.0, 1850.0),
    (4120, "rust-analyzer", 12.0, 2400.0),
    (3310, "Isolated Web Content", 9.5, 620.0),
    (3600, "helium", 6.4, 880.0),
    (1203, "niri", 4.2, 310.0),
    (2950, "spotify", 3.4, 410.0),
    (3800, "slack", 2.2, 700.0),
    (1980, "kitty", 2.1, 180.0),
    (3005, "Discord", 1.8, 540.0),
    (1022, "pipewire", 1.2, 38.0),
    (2700, "syncthing", 0.9, 140.0),
    (2210, "nimbus", 0.8, 42.0),
    (1544, "Xwayland", 0.6, 96.0),
    (4400, "nvim", 0.5, 120.0),
    (1025, "wireplumber", 0.4, 52.0),
    (911, "dbus-broker", 0.2, 8.0),
    (870, "NetworkManager", 0.1, 24.0),
    (1, "systemd", 0.0, 18.0),
    (2066, "zsh", 0.0, 9.0),
    (1700, "mako", 0.0, 14.0),
    (1300, "gnome-keyring-daemon", 0.0, 11.0),
    (2500, "bluetoothd", 0.0, 6.0),
];

fn noise(a: u32, b: u32) -> f32 {
    ((a as f32 * 12.9898 + b as f32 * 78.233).sin() * 43_758.547).fract().abs()
}

fn fake(id: u64, painter: &Mutex<Painter>, icons: &mut IconFinder, scale: f32, step: u32, procs: &[(u32, &str, f32, f32)]) -> Arc<Snapshot> {
    let ps: Vec<Proc> = procs
        .iter()
        .map(|&(pid, name, cpu, mib)| Proc {
            pid,
            name: Arc::from(name),
            cpu: cpu * (0.88 + 0.24 * noise(pid, step)),
            rss: (mib * 1_048_576.0 * (0.985 + 0.03 * noise(pid + 7, step))) as u64,
        })
        .collect();
    let total = (ps.iter().map(|p| p.cpu).sum::<f32>() / 1600.0).min(1.0);
    let cores = (0..16u32).map(|k| (total * (0.4 + 1.3 * noise(k, step)) + if k == 3 { 0.6 } else { 0.0 }).clamp(0.02, 1.0)).collect();
    let mem_total = 30_u64 << 30;
    let mem_used = (11.2 * (1u64 << 30) as f32 * (0.98 + 0.04 * noise(99, step))) as u64;
    let mut p = painter.lock().unwrap();
    p.push(total, mem_used as f32 / mem_total as f32);
    let graphs = Some(p.paint_both(scale));
    let icons = icons.resolve(ps.iter().map(|p| &p.name), icon_px(scale));
    Arc::new(Snapshot {
        id,
        cpu: total,
        cores,
        mem_total,
        mem_used,
        swap_total: 8 << 30,
        swap_used: (1.3 * (1u64 << 30) as f32) as u64,
        load: [2.31 + 0.1 * noise(1, step), 1.84, 1.22],
        uptime: 3.0 * 3600.0 + 12.0 * 60.0 + step as f64,
        threads: 2318 + step,
        procs: ps,
        graphs,
        icons,
    })
}

/// Headless demo for `nimbus --snapshot` (NIMBUS_SNAPSHOT_ONLY=monitor).
pub fn demo(d: &mut crate::snapshot::Demo) {
    let scale = d.scale;
    let mut w = Monitor::new(&mut d.ctx(), false, true);
    {
        // A minute of history so the graphs are full.
        let mut p = w.painter.lock().unwrap();
        for i in 0..HIST {
            let t = i as f32;
            let cpu = 0.22 + 0.12 * (t * 0.21).sin() + 0.08 * (t * 0.83).sin().max(0.0) + 0.05 * noise(i as u32, 3);
            p.push(cpu, 0.36 + 0.012 * (t * 0.11).sin() + 0.004 * noise(i as u32, 5));
        }
    }
    let id = w.id;
    let painter = w.painter.clone();
    let mut step = 0;
    let mut finder = IconFinder::default();
    // Each sample is shot right away: that frame carries the graph uploads
    // (Demo::tick's own upload path has no text buffers to render with).
    let mut feed = |d: &mut crate::snapshot::Demo, w: &mut Monitor, procs: &[(u32, &str, f32, f32)], name: &str| {
        step += 1;
        let t0 = Instant::now();
        let snap = fake(id, &painter, &mut finder, scale, step, procs);
        if step == 1 {
            eprintln!("monitor: first sample with {} app icons in {:.1} ms", snap.icons.len(), t0.elapsed().as_secs_f64() * 1e3);
        }
        assert!(w.on_fact(&Fact::Custom(snap)));
        d.shoot(w, name);
    };
    let shoot_seq = |d: &mut crate::snapshot::Demo, w: &mut Monitor, name: &str, times: &[f32]| {
        let mut t = 0.0;
        for (i, &at) in times.iter().enumerate() {
            d.tick(w, at - t);
            t = at;
            d.shoot(w, &format!("{name}_{i}"));
        }
    };
    let click = |d: &mut crate::snapshot::Demo, w: &mut Monitor, c: Ctl| {
        d.tick(w, 1.0 / 120.0);
        if let Some((x, y)) = w.hit_center(c) {
            w.pointer_motion(x, y);
            w.press(x, y);
            w.release();
        }
    };

    // Open, with the first sample landing mid-way (rows pour in).
    shoot_seq(d, &mut w, "monitor_open", &[0.05, 0.1, 0.15, 0.2]);
    feed(d, &mut w, &DEMO_PROCS, "monitor_pour_pre");
    shoot_seq(d, &mut w, "monitor_pour", &[0.1, 0.2, 0.35]);
    d.tick(&mut w, 1.0);
    // Next sample: the graphs scroll by one step and settle.
    feed(d, &mut w, &DEMO_PROCS, "monitor_scroll_pre");
    shoot_seq(d, &mut w, "monitor_scroll", &[0.2]);
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_main");
    // At the real cadence (a sample exactly every second, 120 Hz frames)
    // the window settles well before each next sample and then stays still.
    let mut l = DrawList::default();
    let mut least = usize::MAX;
    for i in 0..4 {
        feed(d, &mut w, &DEMO_PROCS, &format!("monitor_cadence_{i}"));
        let mut last_busy = 0;
        for f in 0..120 {
            w.draw(&mut d.gpu.fonts, &mut l);
            w.chrome.wake_at = None;
            if w.step(1.0 / 120.0) {
                last_busy = f + 1;
            }
        }
        assert!(w.chrome.wake_at.is_none());
        least = least.min(120 - last_busy);
    }
    eprintln!("monitor: at 1 Hz samples / 120 Hz frames, at least {least} idle frames before each sample");
    assert!(least >= 24, "monitor animates (almost) all the time between samples: {least} idle frames");
    // Another window's (or module's) payload is not ours: no redraw.
    assert!(!w.on_fact(&Fact::Custom(Arc::new(0u32))));

    // Rank change: firefox spikes, rustc finishes, clang starts, Discord exits.
    let mut ranked = DEMO_PROCS.to_vec();
    ranked.retain(|p| p.1 != "Discord");
    for p in &mut ranked {
        match p.1 {
            "firefox" => p.2 = 64.0,
            "rustc" => p.2 = 3.0,
            "cargo" => p.2 = 1.5,
            "slack" => p.2 = 14.0,
            _ => {}
        }
    }
    ranked.push((6012, "clang-18", 41.0, 760.0));
    feed(d, &mut w, &ranked, "monitor_rank_pre");
    shoot_seq(d, &mut w, "monitor_rank", &[0.04, 0.09, 0.15, 0.24, 0.4]);
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_rank_end");

    // Sort by memory, then name, then back to CPU.
    click(d, &mut w, Ctl::Head(Sort::Mem));
    shoot_seq(d, &mut w, "monitor_sort_mem", &[0.06, 0.14, 0.3]);
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_sort_mem");
    click(d, &mut w, Ctl::Head(Sort::Name));
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_sort_name");
    click(d, &mut w, Ctl::Head(Sort::Name));
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_sort_name_desc");
    click(d, &mut w, Ctl::Head(Sort::Cpu));
    d.tick(&mut w, 1.0);

    // Hover, select, glide the selection to another row.
    let pid_at = |w: &Monitor, i: usize| w.order.get(i).copied().unwrap_or(0);
    let p3 = pid_at(&w, 3);
    if let Some((x, y)) = w.hit_center(Ctl::Row(p3)) {
        w.pointer_motion(x, y);
    }
    d.tick(&mut w, 0.4);
    d.shoot(&mut w, "monitor_hover");
    let p1 = pid_at(&w, 1);
    click(d, &mut w, Ctl::Row(p1));
    shoot_seq(d, &mut w, "monitor_select", &[0.08, 0.2, 0.5]);
    click(d, &mut w, Ctl::Row(p3));
    shoot_seq(d, &mut w, "monitor_glide", &[0.05, 0.1, 0.18, 0.5]);

    // End (SIGTERM, dry run), then arm the force quit.
    click(d, &mut w, Ctl::End);
    d.tick(&mut w, 0.5);
    d.shoot(&mut w, "monitor_end");
    {
        // A signalled (dimmed) row can still be clicked, and a held
        // Delete never repeats into signals.
        let p = w.selected.unwrap();
        let del = KeyInput { sym: keys::DELETE, text: None, ctrl: false, shift: true, alt: false };
        w.key_repeat(&del);
        assert!(w.armed.is_none(), "a repeat armed the kill");
        click(d, &mut w, Ctl::Row(p));
        assert!(w.selected.is_none());
        d.tick(&mut w, 0.3);
        assert!(w.hit_center(Ctl::Row(p)).is_some(), "signalled row is not clickable");
        w.key(&del);
        assert!(w.armed.is_none(), "armed with nothing selected");
        click(d, &mut w, Ctl::Row(p));
        assert_eq!(w.selected, Some(p));
        d.tick(&mut w, 0.3);
    }
    click(d, &mut w, Ctl::Kill);
    shoot_seq(d, &mut w, "monitor_armed", &[0.08, 0.5]);
    // The process exits: its row collapses, the rest close ranks.
    let gone = w.selected;
    ranked.retain(|p| Some(p.0) != gone);
    feed(d, &mut w, &ranked, "monitor_gone_pre");
    shoot_seq(d, &mut w, "monitor_gone", &[0.06, 0.14, 0.25]);
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_gone_end");
    d.tick(&mut w, 3.0);
    w.pointer_leave();

    // The selection stays in view when it ranks out of the top rows: sort
    // by name with the lowest-CPU process selected, then go back.
    let last = w.list.last().map(|&i| w.snap.as_ref().unwrap().procs[i as usize].pid).unwrap();
    w.select(None);
    w.select(Some(last));
    assert!(w.order.contains(&last) || !w.order.is_empty());
    click(d, &mut w, Ctl::Head(Sort::Mem));
    d.tick(&mut w, 0.25);
    d.shoot(&mut w, "monitor_pinned_0");
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_pinned");
    assert_eq!(w.order.last(), Some(&last), "selection scrolled out of view");
    w.key(&KeyInput { sym: keys::ESCAPE, text: None, ctrl: false, shift: false, alt: false });
    click(d, &mut w, Ctl::Head(Sort::Cpu));
    d.tick(&mut w, 1.0);
    w.pointer_leave();
    d.tick(&mut w, 1.5);

    // Type to filter.
    for ch in ["r", "u"] {
        w.key(&KeyInput { sym: ch.as_bytes()[0] as u32, text: Some(ch.into()), ctrl: false, shift: false, alt: false });
    }
    shoot_seq(d, &mut w, "monitor_filter", &[0.08, 0.2]);
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_filter_end");
    w.key(&KeyInput { sym: keys::ESCAPE, text: None, ctrl: false, shift: false, alt: false });
    d.tick(&mut w, 1.0);
    d.shoot(&mut w, "monitor_unfilter");
    w.close();
    shoot_seq(d, &mut w, "monitor_close", &[0.06, 0.12, 0.2]);
    // Settle without drawing; the final frame (shot) frees the graphs.
    for _ in 0..180 {
        w.step(1.0 / 120.0);
    }
    assert!(w.chrome.done(), "monitor did not finish closing");
    d.shoot(&mut w, "monitor_closed");
    assert!(!w.tex_live);

    // The real thing: the worker samples /proc and the window follows.
    let mut r = Monitor::new(&mut d.ctx(), true, true);
    for i in 0..4 {
        d.pump(&mut r, 1.3);
        d.shoot(&mut r, &format!("monitor_real_{i}"));
        d.tick(&mut r, 0.05);
    }
    d.tick(&mut r, 1.0);
    d.shoot(&mut r, "monitor_real");
    let n = r.snap.as_ref().map_or(0, |s| s.procs.len());
    let mut sm = Sampler::new();
    sm.sample(0);
    let t0 = Instant::now();
    let _ = sm.sample(0);
    let t1 = t0.elapsed();
    let mut pt = Painter::default();
    for i in 0..HIST + 2 {
        pt.push(noise(i as u32, 1), 0.4);
    }
    let t2 = Instant::now();
    let _ = pt.paint_both(scale);
    eprintln!("monitor: real sampling saw {n} processes; one /proc pass {:.2} ms, graphs {:.2} ms", t1.as_secs_f64() * 1e3, t2.elapsed().as_secs_f64() * 1e3);
    r.close();
    assert!(r.worker.is_none());
    for _ in 0..180 {
        r.step(1.0 / 120.0);
    }
    d.shoot(&mut r, "monitor_real_closed");
}
