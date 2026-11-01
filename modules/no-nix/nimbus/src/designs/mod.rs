//! Designs: whole-bar concepts drawn from the shared `Bar` state.
//!
//! A design module provides:
//! - `State` (its own springs) with `Default`
//! - `FONTS`: families it needs loaded
//! - `step(&Bar, &mut State, dt) -> bool` (true while animating)
//! - `draw(&mut Bar, &mut State, &mut FontSystem, &mut DrawList)`
//!
//! Select with `NIMBUS_DESIGN=<name>`; `islands` is the built-in default
//! drawn by `bar.rs` (and styled by `NIMBUS_LOOK`).

use glyphon::FontSystem;

use crate::{bar::Bar, gfx::DrawList};

pub mod braun;
pub mod deck;
pub mod led;
pub mod monolith;
pub mod swiss;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Design {
    Islands,
    Monolith,
    Tabs,
    Braun,
    Led,
    Swiss,
    Deck,
}

impl Design {
    pub const ALL: [Design; 7] = [Self::Deck, Self::Tabs, Self::Monolith, Self::Islands, Self::Braun, Self::Led, Self::Swiss];

    pub fn name(self) -> &'static str {
        match self {
            Self::Islands => "islands",
            Self::Monolith => "monolith",
            Self::Tabs => "tabs",
            Self::Braun => "braun",
            Self::Led => "led",
            Self::Swiss => "swiss",
            Self::Deck => "deck",
        }
    }

    pub fn from_name(n: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == n)
    }

    /// NIMBUS_DESIGN, else the saved choice, else islands.
    pub fn current() -> Self {
        let saved = std::env::var("NIMBUS_DESIGN").ok().or_else(|| crate::config::get("design"));
        saved.as_deref().and_then(Self::from_name).unwrap_or(Self::Islands)
    }

    pub fn fonts(self) -> &'static [&'static str] {
        match self {
            Self::Islands => &[],
            Self::Monolith | Self::Tabs => monolith::FONTS,
            Self::Braun => braun::FONTS,
            Self::Led => led::FONTS,
            Self::Swiss => swiss::FONTS,
            Self::Deck => deck::FONTS,
        }
    }
}

#[derive(Default)]
pub struct States {
    pub monolith: monolith::State,
    pub braun: braun::State,
    pub led: led::State,
    pub swiss: swiss::State,
    pub deck: deck::State,
}

macro_rules! with_state {
    ($bar:expr, $field:ident, |$st:ident| $body:expr) => {{
        let mut $st = std::mem::take(&mut $bar.ds.$field);
        let r = $body;
        $bar.ds.$field = $st;
        r
    }};
}

pub fn step(bar: &mut Bar, dt: f32) -> bool {
    match bar.design {
        Design::Islands => false,
        Design::Monolith | Design::Tabs => {
            let m = with_state!(bar, monolith, |st| monolith::step(bar, &mut st, dt));
            if let Some(t) = bar.ds.monolith.wake {
                bar.wake(t);
            }
            m
        }
        Design::Braun => with_state!(bar, braun, |st| braun::step(bar, &mut st, dt)),
        Design::Led => with_state!(bar, led, |st| led::step(bar, &mut st, dt)),
        Design::Swiss => with_state!(bar, swiss, |st| swiss::step(bar, &mut st, dt)),
        Design::Deck => {
            let m = with_state!(bar, deck, |st| deck::step(bar, &mut st, dt));
            if let Some(t) = bar.ds.deck.wake {
                bar.wake(t);
            }
            m
        }
    }
}

/// Returns false for the built-in islands design.
pub fn draw(bar: &mut Bar, fs: &mut FontSystem, out: &mut DrawList) -> bool {
    match bar.design {
        Design::Islands => return false,
        Design::Monolith | Design::Tabs => with_state!(bar, monolith, |st| monolith::draw(bar, &mut st, fs, out)),
        Design::Braun => with_state!(bar, braun, |st| braun::draw(bar, &mut st, fs, out)),
        Design::Led => with_state!(bar, led, |st| led::draw(bar, &mut st, fs, out)),
        Design::Swiss => with_state!(bar, swiss, |st| swiss::draw(bar, &mut st, fs, out)),
        Design::Deck => with_state!(bar, deck, |st| deck::draw(bar, &mut st, fs, out)),
    }
    true
}
