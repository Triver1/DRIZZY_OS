//! Headless demo for `nimbus --snapshot` (`NIMBUS_SNAPSHOT_ONLY=files`):
//! a made-up home in the snapshot folder, never the real one.

use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

use super::*;
use crate::{snapshot::Demo, window::KeyInput};

/// Textures the window handed back, summed over frames.
static FREED: AtomicU64 = AtomicU64::new(0);

fn write(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    std::fs::write(p, data).unwrap();
}

/// A soft procedural landscape, so previews have something to show.
fn picture(w: u32, h: u32, seed: f32) -> image::RgbImage {
    image::RgbImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
        let ridge = 0.55 + 0.12 * (u * 7.0 + seed).sin() + 0.05 * (u * 19.0 + seed * 2.0).sin();
        let land = v > ridge;
        let sky = [0.16 + 0.5 * v, 0.2 + 0.32 * v, 0.42 - 0.1 * v];
        let ground = [0.05 + 0.1 * u, 0.1 + 0.05 * v, 0.14];
        let c = if land { ground } else { sky };
        let sun = (-(((u - 0.68).powi(2) + (v - 0.34).powi(2)) * 90.0)).exp() * if land { 0.0 } else { 0.8 };
        image::Rgb([((c[0] + sun) * 255.0).min(255.0) as u8, ((c[1] + sun * 0.8) * 255.0).min(255.0) as u8, ((c[2] + sun * 0.5) * 255.0).min(255.0) as u8])
    })
}

fn fixture(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
    for d in ["Desktop", "Music/Albums", "Videos", "Downloads", "Documents/Taxes/2024", "Documents/Notes", "Pictures/Trips/Lisbon", "orbit-tools/.git", "orbit-tools/src", "garden-site/src"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    write(root, "orbit-tools/flake.nix", b"{ }\n");
    write(root, "garden-site/package.json", b"{}\n");
    write(root, ".profile", b"export EDITOR=vi\n");
    write(root, ".config/tool/settings.toml", b"a = 1\n");
    for (i, n) in ["harbour-dusk", "ridge-morning", "tram-28", "miradouro"].iter().enumerate() {
        let p = root.join(format!("Pictures/Trips/Lisbon/{n}.jpg"));
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        picture(2400, 1600, i as f32 * 1.7).save(&p).unwrap();
    }
    picture(1200, 2000, 3.1).save(root.join("Pictures/portrait-study.png")).unwrap();
    picture(3200, 800, 0.4).save(root.join("Pictures/panorama-coast.webp")).unwrap();
    write(root, "Pictures/broken-scan.jpg", &[0xff, 0xd8, 0xff, 0xe0, 0, 16, 74, 70, 73, 70, 1, 2, 3]);
    write(root, "Documents/files-to-sort.md", b"# Files to sort\n\nThe attic scans, then the tax folder.\n");
    write(root, "Documents/report-q3.pdf", b"%PDF-1.7\n\x01\x02\x03\xff\xfe binary body");
    write(root, "Documents/Report draft.docx", b"PK\x03\x04\x00\x00binary");
    write(root, "Documents/Taxes/2024/invoice-atelier.pdf", b"%PDF-1.7\n\x00");
    write(root, "Documents/Taxes/2024/receipts.csv", b"date,amount\n2024-01-02,12.40\n");
    write(root, "Documents/Notes/reading-list.md", b"# Reading list\n\n- The Dispossessed\n- Piranesi\n- The Overstory\n\nNotes on *structure* and pacing, to return to after the summer.\n\n## Next\n\n1. Finish the chapter on tides\n2. Lend the atlas back\n");
    write(root, "Downloads/orbit-tools-0.4.2.tar.gz", b"\x1f\x8b\x08\x00compressed\x00");
    write(root, "Downloads/setup.bin", &[0u8; 4096]);
    write(root, "Music/Albums/tide-tables.flac", b"fLaC\x00\x00");
    write(root, "Videos/ferry-crossing.mp4", b"\x00\x00\x00\x18ftypmp42");
    write(root, "orbit-tools/src/main.rs", br#"//! Orbit propagation for the small tools.

use std::collections::HashMap;

/* Two-body only: good enough for planning
   a pass, not for flying one. */
const MU: f64 = 398_600.4418;

#[derive(Clone, Debug)]
pub struct Orbit {
    pub a: f64,
    pub e: f64,
    pub name: String,
}

impl Orbit {
    pub fn period(&self) -> f64 {
        // Kepler's third law.
        2.0 * std::f64::consts::PI * (self.a.powi(3) / MU).sqrt()
    }
}

fn main() {
    let mut fleet: HashMap<&str, Orbit> = HashMap::new();
    fleet.insert("iss", Orbit { a: 6_780.0, e: 0.0006, name: "ISS".to_string() });
    for (id, o) in &fleet {
        println!("{id}: {} takes {:.1} min", o.name, o.period() / 60.0);
    }
    if fleet.is_empty() {
        return;
    }
}
"#);
    write(root, "orbit-tools/README.md", b"# orbit-tools\n\nSmall pass planners.\n");
    for i in 0..14 {
        write(root, &format!("Documents/Notes/meeting-{:02}.md", i + 1), b"# Notes\n");
    }
    // Awkward names: newline, bidi override, spaces, not UTF-8, a pipe.
    let weird = root.join("Downloads/Odd names");
    std::fs::create_dir_all(&weird).unwrap();
    std::fs::write(weird.join("two\nlines.txt"), "x").unwrap();
    std::fs::write(weird.join("photo\u{202e}gpj.exe"), "x").unwrap();
    std::fs::write(weird.join("  spaced   out  .txt"), "x").unwrap();
    std::fs::write(weird.join(OsStr::from_bytes(b"caf\xe9 latin1.txt")), "x").unwrap();
    std::fs::write(weird.join("日本語のメモ.txt"), "こんにちは\n").unwrap();
    let c = std::ffi::CString::new(weird.join("a-pipe").as_os_str().as_bytes()).unwrap();
    // SAFETY: a valid C string.
    unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    let _ = std::os::unix::fs::symlink("/nonexistent/target", weird.join("dangling-link"));
    let _ = std::os::unix::fs::symlink(root.join("Documents/Notes"), weird.join("link-to-notes"));
    // Huge and empty folders.
    let huge = root.join("Pictures/Scans-archive");
    std::fs::create_dir_all(&huge).unwrap();
    for i in 0..12_480 {
        std::fs::File::create(huge.join(format!("scan-{i:05}.tif"))).unwrap();
    }
    std::fs::create_dir_all(root.join("Documents/Empty for now")).unwrap();
}

fn tick(d: &mut Demo, b: &mut Browser, secs: f32) {
    const DT: f32 = 1.0 / 120.0;
    let mut list = DrawList::default();
    for _ in 0..(secs / DT).round() as u32 {
        b.draw(&mut d.gpu.fonts, &mut list);
        if !list.uploads.is_empty() || !list.frees.is_empty() {
            FREED.fetch_add(list.frees.len() as u64, Ordering::Relaxed);
            let only = DrawList { uploads: std::mem::take(&mut list.uploads), frees: std::mem::take(&mut list.frees), ..Default::default() };
            d.render(&only, b.text_buffers(), (4.0, 4.0), "_files_scratch");
        }
        b.step(DT);
    }
}

fn settle(d: &mut Demo, b: &mut Browser, secs: f32) {
    for _ in 0..(secs / 0.05).round() as u32 {
        d.pump(b, 0.01);
        tick(d, b, 0.05);
    }
}

fn press(b: &mut Browser, sym: u32, text: Option<&str>) {
    b.key(&KeyInput { sym, text: text.map(String::from), ctrl: false, shift: false, alt: false });
}

fn typed(d: &mut Demo, b: &mut Browser, s: &str) {
    for c in s.chars() {
        press(b, c as u32, Some(&c.to_string()));
        tick(d, b, 0.02);
    }
}

fn select_named(b: &mut Browser, name: &str) {
    if let Some(i) = b.view.iter().position(|&e| &*b.entries[e as usize].shown == name) {
        b.select(i);
    }
}

/// Nothing may animate at rest: after a generous wait, not one step moves
/// and nothing asks for a frame, over a further second of steps.
fn rest(d: &mut Demo, b: &mut Browser, tag: &str) {
    settle(d, b, 2.5);
    let (mut moved, mut woke) = (0, 0);
    for _ in 0..120 {
        moved += b.step(1.0 / 120.0) as u32;
        woke += b.chrome.wake_at.is_some() as u32;
    }
    eprintln!("files demo: {tag}: steps-moving={moved} wakes={woke} of 120");
}

fn state(b: &mut Browser, tag: &str) {
    let moving = b.step(1.0 / 120.0);
    eprintln!("files demo: {tag}: moving={moving} wake_in={:?} rows={}", b.chrome.wake_at.map(|t| t - b.chrome.time), b.view.len());
}

pub fn demo(d: &mut Demo) {
    let root = d.dir.canonicalize().unwrap_or(d.dir.to_path_buf()).join("fixture-home");
    fixture(&root);
    // SAFETY: the demo runs on the only thread that reads these so far;
    // from here on nothing can reach the real home.
    unsafe {
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_CACHE_HOME", root.join(".cache"));
        std::env::set_var("XDG_CONFIG_HOME", root.join(".config"));
        std::env::set_var("XDG_DATA_HOME", root.join(".local/share"));
    }
    let ix = index::Index::build(&root, index::LIMITS);
    eprintln!("files demo: fixture index entries={} capped={} walk={:?}", ix.stats.entries, ix.stats.capped, ix.stats.elapsed);
    index::install(ix);

    // The opening morph.
    let mut b = Browser::new(&mut d.ctx(), None, true);
    b.chrome.time = 500.0;
    for i in 0..6 {
        d.pump(&mut b, 0.005);
        d.shoot(&mut b, &format!("fb_open_{i}"));
        tick(d, &mut b, 0.05);
    }
    settle(d, &mut b, 1.0);
    d.shoot(&mut b, "fb_home");
    rest(d, &mut b, "home at rest");

    // Pictures: a large image, with metadata under it.
    b.go(root.join("Pictures/Trips/Lisbon"), None, 1.0);
    settle(d, &mut b, 0.8);
    d.shoot(&mut b, "fb_image");
    state(&mut b, "image just after settling");
    rest(d, &mut b, "image at rest");
    press(&mut b, keys::DOWN, None);
    for i in 0..5 {
        d.pump(&mut b, 0.005);
        d.shoot(&mut b, &format!("fb_flip_{i}"));
        tick(d, &mut b, 0.03);
    }
    settle(d, &mut b, 0.6);
    press(&mut b, keys::UP, None);
    d.pump(&mut b, 0.0);
    d.shoot(&mut b, "fb_flip_back");
    eprintln!("files demo: flip back from cache ready={}", b.pv.as_ref().is_some_and(|p| p.ready));
    // The guard itself: answers for an older generation are dropped, and
    // change nothing on screen.
    b.stale = 0;
    let before = b.pv.as_ref().map(|p| p.name.clone());
    let old = Shown { owner: b.owner, generation: b.pv_gen.wrapping_sub(1), content: Mutex::new(Some(Content::Nothing { note: None })), link: None };
    let used = b.on_fact(&Fact::Custom(Arc::new(old)));
    let old = Listing { owner: b.owner, generation: b.list_gen.wrapping_sub(1), dir: "/".into(), select: None, entries: Mutex::new(vec![]), more: false, error: None };
    let used2 = b.on_fact(&Fact::Custom(Arc::new(old)));
    let other = Shown { owner: b.owner + 1000, generation: b.pv_gen, content: Mutex::new(Some(Content::Nothing { note: None })), link: None };
    let used3 = b.on_fact(&Fact::Custom(Arc::new(other)));
    eprintln!("files demo: stale answers used={used},{used2} other-window used={used3} dropped={} pane-kept={}", b.stale, before == b.pv.as_ref().map(|p| p.name.clone()));
    // Fast scrolling: the last one lands, and it is the selected one.
    let mut sent = 0u64;
    let g0 = b.pv_gen;
    for i in 0..30 {
        press(&mut b, if i % 2 == 0 { keys::DOWN } else { keys::UP }, None);
        press(&mut b, keys::DOWN, None);
        tick(d, &mut b, 1.0 / 60.0);
        d.pump(&mut b, 0.0);
        sent = b.pv_gen - g0;
    }
    settle(d, &mut b, 1.2);
    let shown = b.pv.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let sel = b.selected().map(|e| e.shown.to_string()).unwrap_or_default();
    eprintln!("files demo: fast scroll selection-changes={sent} preview-matches-selection={}", shown == sel);

    // Portrait, panorama, corrupt.
    b.go(root.join("Pictures"), Some("portrait-study.png".into()), -1.0);
    settle(d, &mut b, 0.8);
    d.shoot(&mut b, "fb_portrait");
    select_named(&mut b, "panorama-coast.webp");
    settle(d, &mut b, 0.8);
    d.shoot(&mut b, "fb_panorama");
    select_named(&mut b, "broken-scan.jpg");
    settle(d, &mut b, 0.8);
    d.shoot(&mut b, "fb_corrupt");

    // Source code and plain text.
    b.go(root.join("orbit-tools/src"), Some("main.rs".into()), 1.0);
    settle(d, &mut b, 0.6);
    d.shoot(&mut b, "fb_code");
    b.go(root.join("Documents/Notes"), Some("reading-list.md".into()), 1.0);
    settle(d, &mut b, 0.6);
    d.shoot(&mut b, "fb_text");

    // A folder's preview, and a filter (typed, highlighted, then Esc).
    b.go(root.join("Documents"), Some("Notes".into()), 1.0);
    settle(d, &mut b, 0.6);
    d.shoot(&mut b, "fb_folder");
    press(&mut b, keys::RETURN, None);
    settle(d, &mut b, 0.6);
    typed(d, &mut b, "mt1");
    settle(d, &mut b, 0.5);
    d.shoot(&mut b, "fb_filter");
    state(&mut b, "filter (caret blinks on wake-ups)");
    settle(d, &mut b, 11.0);
    state(&mut b, "filter after the blink");
    rest(d, &mut b, "filter at rest");
    press(&mut b, keys::ESCAPE, None);
    settle(d, &mut b, 0.5);
    typed(d, &mut b, "qqqq");
    settle(d, &mut b, 0.5);
    d.shoot(&mut b, "fb_filter_none");
    let (_, closes) = b.key(&KeyInput { sym: keys::ESCAPE, text: None, ctrl: false, shift: false, alt: false });
    eprintln!("files demo: esc with filter closes={closes} filter={:?}", b.filter);
    settle(d, &mut b, 0.4);

    // Huge folder: go to the end and back, timing frames.
    let rss0 = rss_kb();
    b.go(root.join("Pictures/Scans-archive"), None, 1.0);
    let t0 = Instant::now();
    let want = root.join("Pictures/Scans-archive");
    while b.dir != want && t0.elapsed().as_secs_f32() < 5.0 {
        d.pump(&mut b, 0.001);
    }
    eprintln!("files demo: huge listing arrived in {:?} ({} entries)", t0.elapsed(), b.entries.len());
    settle(d, &mut b, 0.6);
    d.shoot(&mut b, "fb_huge");
    // Scene work only (step + draw, including shaping new rows), not the
    // GPU. Every other frame moves a row; one frame jumps to the far end.
    let mut list = DrawList::default();
    let (mut worst, mut sum, mut frames, mut peak) = (0f32, 0f32, 0u32, 0usize);
    let sel0 = b.sel;
    let shaped0 = b.text.bufs.len();
    for i in 0..600 {
        if i % 2 == 0 {
            press(&mut b, keys::DOWN, None);
        }
        if i == 300 {
            press(&mut b, keys::END, None);
        }
        let t = Instant::now();
        b.step(1.0 / 120.0);
        b.draw(&mut d.gpu.fonts, &mut list);
        let ms = t.elapsed().as_secs_f32() * 1000.0;
        worst = worst.max(ms);
        sum += ms;
        frames += 1;
        peak = peak.max(list.texts.len());
    }
    eprintln!(
        "files demo: huge scene per frame avg={:.3}ms worst={:.3}ms; sel {sel0}->{} scroll={:.0}/{:.0} peak texts={peak} (buffers {shaped0}->{}) over {} rows",
        sum / frames as f32,
        worst,
        b.sel,
        b.scroll.get(),
        b.max_scroll(),
        b.text.bufs.len(),
        b.view.len()
    );
    settle(d, &mut b, 0.8);
    d.shoot(&mut b, "fb_huge_end");
    let t = Instant::now();
    typed(d, &mut b, "1234");
    eprintln!("files demo: huge filter 4 keys {:?} -> {} rows", t.elapsed(), b.view.len());
    settle(d, &mut b, 0.4);
    d.shoot(&mut b, "fb_huge_filter");
    press(&mut b, keys::ESCAPE, None);

    // Empty folder.
    b.go(root.join("Documents/Empty for now"), None, 1.0);
    settle(d, &mut b, 0.6);
    d.shoot(&mut b, "fb_empty");
    press(&mut b, keys::BACKSPACE, None);
    settle(d, &mut b, 0.6);
    eprintln!("files demo: backspace went up, selected={:?}", b.selected().map(|e| e.shown.to_string()));

    // Awkward names; hidden files; going up past home; the rail.
    b.go(root.join("Downloads/Odd names"), None, 1.0);
    settle(d, &mut b, 0.6);
    d.shoot(&mut b, "fb_odd");
    select_named(&mut b, "a-pipe");
    settle(d, &mut b, 0.4);
    d.shoot(&mut b, "fb_pipe");
    press(&mut b, keys::RETURN, None);
    eprintln!("files demo: enter on a fifo opened={} closing={}", b.opened.len(), b.chrome.closing);
    select_named(&mut b, "link-to-notes");
    settle(d, &mut b, 0.4);
    d.shoot(&mut b, "fb_link");
    b.go(root.clone(), None, -1.0);
    settle(d, &mut b, 0.6);
    press(&mut b, 0x2e, Some("."));
    settle(d, &mut b, 0.4);
    d.shoot(&mut b, "fb_hidden");
    press(&mut b, 0x2e, Some("."));
    settle(d, &mut b, 0.4);
    // Revealing something hidden turns hidden files on.
    b.on_fact(&deliver(Request { path: root.join(".profile"), from: None }));
    settle(d, &mut b, 0.5);
    eprintln!("files demo: reveal hidden show_hidden={} selected={:?}", b.show_hidden, b.selected().map(|e| e.shown.to_string()));
    // The keys that share a letter with the filter.
    b.go(root.join("Documents/Notes"), Some("meeting-03.md".into()), 1.0);
    settle(d, &mut b, 0.4);
    let s0 = b.sel;
    press(&mut b, 0x6a, Some("j"));
    let down = b.sel;
    press(&mut b, 0x6b, Some("k"));
    let back = b.sel;
    b.key(&KeyInput { sym: 0x66, text: None, ctrl: true, shift: false, alt: false });
    press(&mut b, 0x6a, Some("j"));
    let (typed_j, at) = (b.filter.clone(), b.sel);
    b.key(&KeyInput { sym: 0x6a, text: None, ctrl: true, shift: false, alt: false });
    let ctrl_j = b.sel;
    press(&mut b, keys::ESCAPE, None);
    settle(d, &mut b, 0.3);
    eprintln!("files demo: keys j {s0}->{down}, k ->{back}; after ctrl+f j types {typed_j:?} (sel {at}), ctrl+j ->{ctrl_j}; esc filter={:?}", b.filter);
    // The mouse: glyph or bar puts the caret in the field, typing filters,
    // the clear control ends it.
    tick(d, &mut b, 0.05);
    let at = |b: &Browser, h: Hit| b.hits.iter().rev().find(|(_, x)| *x == h).map(|(r, _)| *r);
    if let Some(r) = at(&b, Hit::Bar) {
        let (x, y) = (r.x + 23.0, r.y + r.h * 0.5);
        b.pointer_motion(x, y);
        b.press(x, y);
        b.release();
        settle(d, &mut b, 0.4);
        d.shoot(&mut b, "fb_bar_focus");
        eprintln!("files demo: click on the glyph focused={} cursor={:?}", b.filtering, b.cursor());
        typed(d, &mut b, "reading");
        settle(d, &mut b, 0.4);
        d.shoot(&mut b, "fb_bar_typed");
        let n = b.view.len();
        tick(d, &mut b, 0.05);
        if let Some(c) = at(&b, Hit::Clear) {
            let (x, y) = (c.x + c.w * 0.5, c.y + c.h * 0.5);
            b.pointer_motion(x, y);
            tick(d, &mut b, 0.02);
            b.press(x, y);
            b.release();
            settle(d, &mut b, 0.4);
            eprintln!("files demo: clear control {n} rows -> {} rows, filter={:?} focused={}", b.view.len(), b.filter, b.filtering);
        }
        b.pointer_leave();
    }
    // A breadcrumb segment still navigates; the bar around it does not.
    tick(d, &mut b, 0.05);
    if let Some(r) = at(&b, Hit::Crumb(1)) {
        b.pointer_motion(r.x + 6.0, r.y + 10.0);
        b.press(r.x + 6.0, r.y + 10.0);
        b.release();
        settle(d, &mut b, 0.4);
        eprintln!("files demo: crumb click -> {:?} filtering={}", b.crumbs.last().map(|c| c.label.clone()), b.filtering);
        b.pointer_leave();
    }
    // A typed path: Tab completes, Enter goes.
    typed(d, &mut b, "~/Pic");
    settle(d, &mut b, 0.3);
    press(&mut b, keys::TAB, None);
    settle(d, &mut b, 0.3);
    let completed = b.filter.clone();
    typed(d, &mut b, "Tr");
    press(&mut b, keys::TAB, None);
    settle(d, &mut b, 0.3);
    d.shoot(&mut b, "fb_path");
    let done = b.filter.clone();
    press(&mut b, keys::RETURN, None);
    settle(d, &mut b, 0.4);
    eprintln!("files demo: path {completed:?} then {done:?} -> {:?}, bar={:?}", b.crumbs.last().map(|c| c.label.clone()), b.filter);
    typed(d, &mut b, "~/Documents/report-q3.pdf");
    press(&mut b, keys::RETURN, None);
    settle(d, &mut b, 0.5);
    eprintln!("files demo: path to a file reveals {:?} in {:?}", b.selected().map(|e| e.shown.to_string()), b.crumbs.last().map(|c| c.label.clone()));
    // Right on a file does nothing; on a folder it opens.
    b.go(root.join("Documents"), Some("files-to-sort.md".into()), 1.0);
    settle(d, &mut b, 0.4);
    press(&mut b, keys::RIGHT, None);
    eprintln!("files demo: right on a file opened={} closing={}", b.opened.len(), b.chrome.closing);
    select_named(&mut b, "Notes");
    press(&mut b, keys::RIGHT, None);
    settle(d, &mut b, 0.4);
    eprintln!("files demo: right on a folder -> {:?}", b.crumbs.last().map(|c| c.label.clone()));
    press(&mut b, keys::LEFT, None);
    settle(d, &mut b, 0.4);
    eprintln!("files demo: left -> {:?}, selected {:?}", b.crumbs.last().map(|c| c.label.clone()), b.selected().map(|e| e.shown.to_string()));
    // Double click opens a folder; a click in a rail place navigates.
    tick(d, &mut b, 0.05);
    if let Some(r) = b.hits.iter().find(|(_, h)| *h == Hit::Row(b.sel)).map(|(r, _)| *r) {
        let (x, y) = (r.x + 60.0, r.y + 10.0);
        b.pointer_motion(x, y);
        for _ in 0..2 {
            b.press(x, y);
            b.release();
        }
        settle(d, &mut b, 0.4);
        eprintln!("files demo: double click -> {:?}", b.crumbs.last().map(|c| c.label.clone()));
        b.pointer_leave();
    }
    if let Some(r) = b.hits.iter().find(|(_, h)| matches!(h, Hit::Place(0))).map(|(r, _)| *r) {
        let (x, y) = (r.x + 30.0, r.y + 10.0);
        b.pointer_motion(x, y);
        b.press(x, y);
        b.release();
        settle(d, &mut b, 0.4);
        eprintln!("files demo: rail Home -> {:?}", b.crumbs.last().map(|c| c.label.clone()));
        b.pointer_leave();
    }
    // A scale change while an image shows: asked for again, at the new size.
    b.go(root.join("Pictures/Trips/Lisbon"), None, 1.0);
    settle(d, &mut b, 0.8);
    let w0 = b.pv.as_ref().and_then(|p| p.tex).map(|t| t.w);
    let s = b.scale;
    b.set_scale(s * 0.5);
    settle(d, &mut b, 0.8);
    let w1 = b.pv.as_ref().and_then(|p| p.tex).map(|t| t.w);
    b.set_scale(s);
    settle(d, &mut b, 0.8);
    eprintln!("files demo: scale change image width {w0:?} -> {w1:?} -> {:?}", b.pv.as_ref().and_then(|p| p.tex).map(|t| t.w));
    // Hover and clicks.
    b.go(root.clone(), None, -1.0);
    settle(d, &mut b, 0.5);
    tick(d, &mut b, 0.05);
    let row = b.hits.iter().find(|(_, h)| matches!(h, Hit::Row(3))).map(|(r, _)| *r);
    if let Some(r) = row {
        let (x, y) = (r.x + 60.0, r.y + 10.0);
        b.pointer_motion(x, y);
        b.pointer_motion(x + 6.0, y + 1.0);
        tick(d, &mut b, 0.05);
        d.shoot(&mut b, "fb_hover");
        b.press(x, y);
        b.release();
        settle(d, &mut b, 0.3);
        eprintln!("files demo: click selected row={} (want 3)", b.sel);
        b.pointer_leave();
    }
    // Opening a file folds the window away (dry run: nothing is spawned).
    b.go(root.join("Documents/Notes"), Some("reading-list.md".into()), 1.0);
    settle(d, &mut b, 0.5);
    let tex_before = b.cache.len();
    FREED.store(0, Ordering::Relaxed);
    press(&mut b, keys::RETURN, None);
    for i in 0..5 {
        d.shoot(&mut b, &format!("fb_close_{i}"));
        tick(d, &mut b, 0.05);
    }
    settle(d, &mut b, 0.6);
    eprintln!(
        "files demo: opened={:?} done={} textures: held {tex_before}, freed {}, left {}",
        b.opened.iter().map(|p| kind::shown(p.file_name().unwrap())).collect::<Vec<_>>(),
        b.chrome.done(),
        FREED.load(Ordering::Relaxed),
        b.cache.len()
    );
    let rss1 = rss_kb();
    drop(b);
    settle_threads();
    eprintln!("files demo: rss before huge folder {rss0} kB, window open {rss1} kB, closed {} kB", rss_kb());

    // Dropped mid-work: textures go to the orphan list, workers leave.
    let mut b = Browser::new(&mut d.ctx(), Some(Request { path: root.join("Pictures/Trips/Lisbon/ridge-morning.jpg"), from: None }), true);
    settle(d, &mut b, 0.8);
    eprintln!("files demo: revealed {:?} tex={}", b.selected().map(|e| e.shown.to_string()), b.cache.len());
    press(&mut b, keys::DOWN, None);
    drop(b);
    settle_threads();

    crate::launcher::demo_files(d);
}

fn rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines().find_map(|l| l.strip_prefix("VmRSS:")?.split_whitespace().next()?.parse().ok()).unwrap_or(0)
}

fn settle_threads() {
    std::thread::sleep(std::time::Duration::from_millis(200));
    let n = std::fs::read_dir("/proc/self/task").map_or(0, |d| d.flatten().filter(|t| std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.starts_with("nimbus-files"))).count());
    eprintln!("files demo: worker threads alive after close: {n}");
}
