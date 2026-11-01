//! Nimbus looks. A `Look` is everything visual that isn't motion: surfaces,
//! type, shape and colour. All colours are straight sRGB.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const fn hex(rgb: u32, a: f32) -> Self {
        Self(
            ((rgb >> 16) & 0xff) as f32 / 255.0,
            ((rgb >> 8) & 0xff) as f32 / 255.0,
            (rgb & 0xff) as f32 / 255.0,
            a,
        )
    }
    pub const TRANSPARENT: Self = Self(0.0, 0.0, 0.0, 0.0);

    pub fn with_a(self, a: f32) -> Self {
        Self(self.0, self.1, self.2, a)
    }
    pub fn fade(self, k: f32) -> Self {
        Self(self.0, self.1, self.2, self.3 * k)
    }
    pub fn mix(self, o: Self, t: f32) -> Self {
        Self(
            self.0 + (o.0 - self.0) * t,
            self.1 + (o.1 - self.1) * t,
            self.2 + (o.2 - self.2) * t,
            self.3 + (o.3 - self.3) * t,
        )
    }
    pub fn arr(self) -> [f32; 4] {
        [self.0, self.1, self.2, self.3]
    }
}

// Glass body: very dark blue-violet smoke, mostly see-through.
pub const GLASS_TOP: Rgba = Rgba::hex(0x1b1d2a, 0.46);
pub const GLASS_BOT: Rgba = Rgba::hex(0x0d0e16, 0.58);
pub const GLASS_HOVER_TOP: Rgba = Rgba::hex(0x262940, 0.54);
pub const GLASS_HOVER_BOT: Rgba = Rgba::hex(0x12131f, 0.64);
// Rim light: bright at the top edge, almost gone at the bottom (lit from above).
pub const RIM_TOP: Rgba = Rgba::hex(0xffffff, 0.30);
pub const RIM_BOT: Rgba = Rgba::hex(0xffffff, 0.05);
pub const SHADOW: Rgba = Rgba::hex(0x000000, 0.38);

pub const TEXT: Rgba = Rgba::hex(0xeef0f7, 1.0);
pub const TEXT_MUTED: Rgba = Rgba::hex(0xa3aabb, 1.0);
pub const TEXT_DIM: Rgba = Rgba::hex(0x6b7285, 1.0);

pub const ACCENT_A: Rgba = Rgba::hex(0x8fb3ff, 1.0); // periwinkle
pub const ACCENT_B: Rgba = Rgba::hex(0xc69bff, 1.0); // orchid
pub const GOOD: Rgba = Rgba::hex(0x7ee2b8, 1.0);
pub const WARN: Rgba = Rgba::hex(0xffd37a, 1.0);
pub const BAD: Rgba = Rgba::hex(0xff7b93, 1.0);

pub const DOT_EMPTY: Rgba = Rgba::hex(0xffffff, 0.16);
pub const DOT_USED: Rgba = Rgba::hex(0xdfe4f2, 0.62);

/// Everything visual about the bar that isn't motion. Pick with
/// `NIMBUS_LOOK=<name>`; all looks share the same springs and layout.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub name: &'static str,
    pub body_top: Rgba,
    pub body_bot: Rgba,
    pub hover_top: Rgba,
    pub hover_bot: Rgba,
    pub rim_top: Rgba,
    pub rim_bot: Rgba,
    pub rim_w: f32,
    /// Edge glow inside the body (glass thickness). 0 = off.
    pub inner: f32,
    /// Cursor-following highlight strength. 0 = off.
    pub specular: f32,
    pub shadow: Rgba,
    pub shadow_sigma: f32,
    pub shadow_dy: f32,
    /// Ask niri to blur behind the bodies.
    pub blur: bool,
    /// Bodies only appear as hover chips.
    pub chips: bool,
    /// Full-width backing strip and its bottom hairline.
    pub strip: Option<(Rgba, Rgba)>,
    /// Corner radius in logical px (15 = capsule).
    pub radius: f32,
    pub text: Rgba,
    pub text_muted: Rgba,
    pub text_shadow: Option<Rgba>,
    pub dot_used: Rgba,
    pub dot_empty: Rgba,
    pub accent_a: Rgba,
    pub accent_b: Rgba,
    pub bloom: f32,
    pub divider: Rgba,
    /// UI typeface (resolved through fontconfig) and its tuning.
    pub font: &'static str,
    pub font_scale: f32,
    pub weight_shift: i16,
    /// Island height in logical px.
    pub island_h: f32,
    /// Islands hang from the top edge like tabs (flat top, rounded bottom).
    pub hang: bool,
    /// Roundness of workspace dots and the indicator, 0 = square, 1 = round.
    pub dot_round: f32,
    pub ind_round: f32,
}

pub const GLASS: Look = Look {
    name: "glass",
    body_top: GLASS_TOP,
    body_bot: GLASS_BOT,
    hover_top: GLASS_HOVER_TOP,
    hover_bot: GLASS_HOVER_BOT,
    rim_top: RIM_TOP,
    rim_bot: RIM_BOT,
    rim_w: 1.0,
    inner: 0.55,
    specular: 1.0,
    shadow: SHADOW,
    shadow_sigma: 7.0,
    shadow_dy: 2.5,
    blur: true,
    chips: false,
    strip: None,
    radius: 15.0,
    text: TEXT,
    text_muted: TEXT_MUTED,
    text_shadow: None,
    dot_used: DOT_USED,
    dot_empty: DOT_EMPTY,
    accent_a: ACCENT_A,
    accent_b: ACCENT_B,
    bloom: 0.42,
    divider: Rgba::hex(0xffffff, 0.22),
    ..MATTE
};

/// Solid graphite pills: opaque, a hairline edge, a real shadow. No gloss.
pub const MATTE: Look = Look {
    name: "matte",
    body_top: Rgba::hex(0x16171d, 0.97),
    body_bot: Rgba::hex(0x131419, 0.97),
    hover_top: Rgba::hex(0x1e1f27, 0.98),
    hover_bot: Rgba::hex(0x1a1b22, 0.98),
    rim_top: Rgba::hex(0xffffff, 0.08),
    rim_bot: Rgba::hex(0xffffff, 0.05),
    rim_w: 1.0,
    inner: 0.0,
    specular: 0.0,
    shadow: Rgba::hex(0x000000, 0.42),
    shadow_sigma: 9.0,
    shadow_dy: 3.0,
    blur: false,
    chips: false,
    strip: None,
    radius: 15.0,
    text: Rgba::hex(0xececf1, 1.0),
    text_muted: Rgba::hex(0x8e94a4, 1.0),
    text_shadow: None,
    dot_used: Rgba::hex(0xc9cdd8, 0.75),
    dot_empty: Rgba::hex(0xffffff, 0.18),
    accent_a: ACCENT_A,
    accent_b: ACCENT_B,
    bloom: 0.22,
    divider: Rgba::hex(0xffffff, 0.12),
    font: "Inter",
    font_scale: 1.0,
    weight_shift: 0,
    island_h: 30.0,
    hang: false,
    dot_round: 1.0,
    ind_round: 1.0,
};

/// Classic edge-to-edge strip, near-black; hovered items get a soft chip.
pub const STRIP: Look = Look {
    name: "strip",
    body_top: Rgba::hex(0xffffff, 0.075),
    body_bot: Rgba::hex(0xffffff, 0.06),
    hover_top: Rgba::hex(0xffffff, 0.075),
    hover_bot: Rgba::hex(0xffffff, 0.06),
    rim_top: Rgba::hex(0xffffff, 0.0),
    rim_bot: Rgba::hex(0xffffff, 0.0),
    rim_w: 0.0,
    inner: 0.0,
    specular: 0.0,
    shadow: Rgba::hex(0x000000, 0.0),
    shadow_sigma: 0.0,
    shadow_dy: 0.0,
    blur: false,
    chips: true,
    strip: Some((Rgba::hex(0x0e0f13, 0.985), Rgba::hex(0xffffff, 0.06))),
    radius: 9.0,
    text: Rgba::hex(0xe9eaef, 1.0),
    text_muted: Rgba::hex(0x868b99, 1.0),
    text_shadow: None,
    dot_used: Rgba::hex(0xc9cdd8, 0.72),
    dot_empty: Rgba::hex(0xffffff, 0.18),
    accent_a: ACCENT_A,
    accent_b: ACCENT_B,
    bloom: 0.3,
    divider: Rgba::hex(0xffffff, 0.10),
    ..MATTE
};

/// No containers at all: content floats on the wallpaper.
pub const FLOAT: Look = Look {
    name: "float",
    body_top: Rgba::hex(0x000000, 0.30),
    body_bot: Rgba::hex(0x000000, 0.36),
    hover_top: Rgba::hex(0x000000, 0.30),
    hover_bot: Rgba::hex(0x000000, 0.36),
    rim_top: Rgba::hex(0xffffff, 0.0),
    rim_bot: Rgba::hex(0xffffff, 0.0),
    rim_w: 0.0,
    inner: 0.0,
    specular: 0.0,
    shadow: Rgba::hex(0x000000, 0.0),
    shadow_sigma: 0.0,
    shadow_dy: 0.0,
    blur: false,
    chips: true,
    strip: None,
    radius: 15.0,
    text: Rgba::hex(0xffffff, 1.0),
    text_muted: Rgba::hex(0xe4e6ee, 0.86),
    text_shadow: Some(Rgba::hex(0x000000, 0.55)),
    dot_used: Rgba::hex(0xffffff, 0.85),
    dot_empty: Rgba::hex(0xffffff, 0.45),
    accent_a: Rgba::hex(0xffffff, 1.0),
    accent_b: Rgba::hex(0xdfe6ff, 1.0),
    bloom: 0.5,
    divider: Rgba::hex(0xffffff, 0.35),
    ..MATTE
};

/// Light: warm porcelain pills with ink text.
pub const PORCELAIN: Look = Look {
    name: "porcelain",
    body_top: Rgba::hex(0xf7f5f1, 0.97),
    body_bot: Rgba::hex(0xeceae5, 0.97),
    hover_top: Rgba::hex(0xffffff, 0.99),
    hover_bot: Rgba::hex(0xf3f1ed, 0.99),
    rim_top: Rgba::hex(0xffffff, 0.9),
    rim_bot: Rgba::hex(0x000000, 0.07),
    rim_w: 1.0,
    inner: 0.0,
    specular: 0.0,
    shadow: Rgba::hex(0x000000, 0.20),
    shadow_sigma: 10.0,
    shadow_dy: 3.0,
    blur: false,
    chips: false,
    strip: None,
    radius: 15.0,
    text: Rgba::hex(0x1b1c21, 1.0),
    text_muted: Rgba::hex(0x5c606e, 1.0),
    text_shadow: None,
    dot_used: Rgba::hex(0x2a2c33, 0.72),
    dot_empty: Rgba::hex(0x000000, 0.22),
    accent_a: Rgba::hex(0x5b7cfa, 1.0),
    accent_b: Rgba::hex(0x9b6cf5, 1.0),
    bloom: 0.28,
    divider: Rgba::hex(0x000000, 0.14),
    ..MATTE
};


// ── matte family ────────────────────────────────────────────────────────

/// Geist tabs hanging from the top edge, cool cyan accent.
pub const TAB: Look = Look {
    name: "tab",
    body_top: Rgba::hex(0x15161b, 0.98),
    body_bot: Rgba::hex(0x121317, 0.98),
    hover_top: Rgba::hex(0x1c1d24, 0.99),
    hover_bot: Rgba::hex(0x18191f, 0.99),
    rim_top: Rgba::hex(0xffffff, 0.0),
    rim_bot: Rgba::hex(0xffffff, 0.07),
    radius: 13.0,
    shadow: Rgba::hex(0x000000, 0.45),
    shadow_sigma: 10.0,
    shadow_dy: 4.0,
    accent_a: Rgba::hex(0x6fd3ff, 1.0),
    accent_b: Rgba::hex(0x8fa8ff, 1.0),
    font: "Geist",
    island_h: 31.0,
    hang: true,
    ..MATTE
};

/// JetBrains Mono, squared-off corners, square pixels for dots, amber.
pub const MONO: Look = Look {
    name: "mono",
    body_top: Rgba::hex(0x141414, 0.97),
    body_bot: Rgba::hex(0x111111, 0.97),
    hover_top: Rgba::hex(0x1c1c1c, 0.98),
    hover_bot: Rgba::hex(0x181818, 0.98),
    rim_top: Rgba::hex(0xffffff, 0.09),
    rim_bot: Rgba::hex(0xffffff, 0.05),
    radius: 7.0,
    text: Rgba::hex(0xe8e6e3, 1.0),
    text_muted: Rgba::hex(0x8f8a84, 1.0),
    dot_used: Rgba::hex(0xd9d4cc, 0.75),
    dot_empty: Rgba::hex(0xffffff, 0.16),
    accent_a: Rgba::hex(0xffc069, 1.0),
    accent_b: Rgba::hex(0xff9348, 1.0),
    bloom: 0.25,
    font: "JetBrains Mono",
    font_scale: 0.94,
    dot_round: 0.3,
    ind_round: 0.3,
    ..MATTE
};

/// Space Grotesk, sharp slabs, heavier type, mint accent.
pub const SLAB: Look = Look {
    name: "slab",
    body_top: Rgba::hex(0x17181a, 0.98),
    body_bot: Rgba::hex(0x17181a, 0.98),
    hover_top: Rgba::hex(0x1f2023, 0.99),
    hover_bot: Rgba::hex(0x1f2023, 0.99),
    rim_top: Rgba::hex(0xffffff, 0.06),
    rim_bot: Rgba::hex(0xffffff, 0.06),
    radius: 3.0,
    shadow: Rgba::hex(0x000000, 0.5),
    shadow_sigma: 4.0,
    shadow_dy: 4.0,
    text: Rgba::hex(0xf1f2ef, 1.0),
    text_muted: Rgba::hex(0x9a9e98, 1.0),
    accent_a: Rgba::hex(0x86efc0, 1.0),
    accent_b: Rgba::hex(0x55d6c2, 1.0),
    bloom: 0.2,
    font: "Space Grotesk",
    font_scale: 1.02,
    weight_shift: 100,
    dot_round: 0.0,
    ind_round: 0.0,
    ..MATTE
};

/// Manrope on warm graphite, soft squircle-ish corners, coral accent.
pub const SOFT: Look = Look {
    name: "soft",
    body_top: Rgba::hex(0x1d1b19, 0.97),
    body_bot: Rgba::hex(0x191715, 0.97),
    hover_top: Rgba::hex(0x25221f, 0.98),
    hover_bot: Rgba::hex(0x201d1a, 0.98),
    rim_top: Rgba::hex(0xfff4e8, 0.08),
    rim_bot: Rgba::hex(0xfff4e8, 0.04),
    radius: 11.0,
    island_h: 32.0,
    shadow: Rgba::hex(0x000000, 0.38),
    shadow_sigma: 12.0,
    shadow_dy: 4.0,
    text: Rgba::hex(0xf2ebe3, 1.0),
    text_muted: Rgba::hex(0xa39a90, 1.0),
    dot_used: Rgba::hex(0xe6ddd2, 0.7),
    dot_empty: Rgba::hex(0xfff4e8, 0.17),
    accent_a: Rgba::hex(0xff9a76, 1.0),
    accent_b: Rgba::hex(0xff6f91, 1.0),
    bloom: 0.25,
    divider: Rgba::hex(0xfff4e8, 0.12),
    font: "Manrope",
    weight_shift: 100,
    ..MATTE
};

/// Instrument Sans, pure ink black, monochrome: the accent is white.
pub const INK: Look = Look {
    name: "ink",
    body_top: Rgba::hex(0x0a0a0b, 0.98),
    body_bot: Rgba::hex(0x0a0a0b, 0.98),
    hover_top: Rgba::hex(0x141416, 0.99),
    hover_bot: Rgba::hex(0x141416, 0.99),
    rim_top: Rgba::hex(0xffffff, 0.10),
    rim_bot: Rgba::hex(0xffffff, 0.04),
    shadow: Rgba::hex(0x000000, 0.35),
    text: Rgba::hex(0xffffff, 1.0),
    text_muted: Rgba::hex(0x9b9ba1, 1.0),
    dot_used: Rgba::hex(0xffffff, 0.6),
    dot_empty: Rgba::hex(0xffffff, 0.2),
    accent_a: Rgba::hex(0xffffff, 1.0),
    accent_b: Rgba::hex(0xdedee3, 1.0),
    bloom: 0.18,
    font: "Instrument Sans",
    font_scale: 1.04,
    ..MATTE
};

pub const LOOKS: [&Look; 10] = [&MATTE, &TAB, &MONO, &SLAB, &SOFT, &INK, &GLASS, &STRIP, &FLOAT, &PORCELAIN];

pub fn look(name: &str) -> &'static Look {
    LOOKS.iter().copied().find(|l| l.name == name).unwrap_or(&MATTE)
}

/// The look selected for this process (`NIMBUS_LOOK`, default matte).
pub fn current() -> &'static Look {
    look(&std::env::var("NIMBUS_LOOK").unwrap_or_default())
}

/// Accent pairs for the matte designs (gradient start, end), selectable in
/// the settings window. Prism (the default) is the iridescent Prism Ink
/// fill: its pair is only a stand-in where a single colour is needed.
pub const ACCENTS: [(&str, Rgba, Rgba); 7] = [
    ("Prism", Rgba::hex(0x9fdcff, 1.0), Rgba::hex(0xffb3e4, 1.0)),
    ("Iris", Rgba::hex(0x8fb3ff, 1.0), Rgba::hex(0xc69bff, 1.0)),
    ("Mint", Rgba::hex(0x7ee2b8, 1.0), Rgba::hex(0x5fd4d9, 1.0)),
    ("Coral", Rgba::hex(0xff9a76, 1.0), Rgba::hex(0xff6f91, 1.0)),
    ("Amber", Rgba::hex(0xffc069, 1.0), Rgba::hex(0xff9348, 1.0)),
    ("Ice", Rgba::hex(0xd9e6ff, 1.0), Rgba::hex(0x9fc2ff, 1.0)),
    ("Rose", Rgba::hex(0xffa3c7, 1.0), Rgba::hex(0xd59bff, 1.0)),
];

/// Dark glyph on an iridescent (or accent) fill.
pub const ON_INK: Rgba = Rgba::hex(0x1a1426, 1.0);

static ACCENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// Saved accent: stored by name. Older configs stored an index into the
/// list before Prism existed; their default (0) becomes Prism.
fn parse_accent(v: &str) -> Option<usize> {
    if let Some(i) = ACCENTS.iter().position(|a| a.0.eq_ignore_ascii_case(v)) {
        return Some(i);
    }
    let i: usize = v.parse().ok()?;
    let i = if i == 0 { 0 } else { i + 1 };
    (i < ACCENTS.len()).then_some(i)
}

pub fn accent_index() -> usize {
    use std::sync::atomic::Ordering;
    let i = ACCENT.load(Ordering::Relaxed);
    if i != usize::MAX {
        return i;
    }
    let i = crate::config::get("accent").and_then(|v| parse_accent(&v)).unwrap_or(0);
    ACCENT.store(i, Ordering::Relaxed);
    i
}

pub fn set_accent(i: usize) {
    let i = i.min(ACCENTS.len() - 1);
    ACCENT.store(i, std::sync::atomic::Ordering::Relaxed);
    crate::config::set("accent", ACCENTS[i].0);
}

/// Current accent gradient.
pub fn accent() -> (Rgba, Rgba) {
    let a = ACCENTS[accent_index()];
    (a.1, a.2)
}

/// Whether "on" things are iridescent (the Prism accent).
pub fn prism() -> bool {
    accent_index() == 0
}

/// Glyph colour on top of an "on" fill.
pub fn on_ink() -> Rgba {
    if prism() { ON_INK } else { Rgba::hex(0x0b0b0e, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_names_and_legacy_indices() {
        assert_eq!(parse_accent("Prism"), Some(0));
        assert_eq!(parse_accent("coral"), Some(3));
        assert_eq!(parse_accent("0"), Some(0));
        assert_eq!(parse_accent("2"), Some(3)); // legacy Coral
        assert_eq!(parse_accent("5"), Some(6)); // legacy Rose
        assert_eq!(parse_accent("6"), None);
        assert_eq!(parse_accent("nope"), None);
    }
}
