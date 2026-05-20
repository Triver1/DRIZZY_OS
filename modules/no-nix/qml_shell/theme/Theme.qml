pragma Singleton
import QtQuick

QtObject {
  id: root

  property int currentTheme: 0

  // ── Background scale ──────────────────────────────────────────────────────
  property color black:   "#181a1c"
  property color bgDim:   "#252630"
  property color bg:      "#2b2d3a"
  property color surface: "#333648"
  property color card:    "#363a4e"
  property color input:   "#393e53"
  property color raised:  "#3f445b"

  // ── Borders (computed from surface so they follow theme automatically) ────
  property color border:      "#585d74"
  property color borderFocus: green

  // ── Text ─────────────────────────────────────────────────────────────────
  property color fg:      "#f2f2f3"
  property color fgMuted: "#c4c6cf"
  property color fgDim:   "#9da1af"

  // ── Accent palette ────────────────────────────────────────────────────────
  property color red:    "#ff8c9a"
  property color orange: "#f3bb9a"
  property color yellow: "#f8e7b0"
  property color green:  "#b4e49a"
  property color blue:   "#98d4e7"
  property color purple: "#bdb2ff"
  property color grey:   "#c4c6cf"

  // ── Semantic aliases (bindings, auto-follow the base colors) ──────────────
  property color accent:  green
  property color warning: yellow
  property color danger:  red
  property color info:    blue

  // ── Overlays (computed bindings) ──────────────────────────────────────────
  property color panelBg:    Qt.rgba(surface.r, surface.g, surface.b, 0.94)
  property color overlayDim: "#cc000000"
  property color shadow:     Qt.rgba(black.r, black.g, black.b, 0.33)

  // ── Helpers ───────────────────────────────────────────────────────────────
  function alpha(c, a) { return Qt.rgba(c.r, c.g, c.b, a) }

  // Adjust lightness of a hex color by delta (-1..1) while keeping hue/sat.
  function lightenHex(hex, delta) {
    var r = parseInt(hex.slice(1,3),16)/255
    var g = parseInt(hex.slice(3,5),16)/255
    var b = parseInt(hex.slice(5,7),16)/255
    var max=Math.max(r,g,b), min=Math.min(r,g,b)
    var l=(max+min)/2
    var s=max===min?0:(l<0.5?(max-min)/(max+min):(max-min)/(2-max-min))
    var h=0
    if(max!==min){if(max===r)h=((g-b)/(max-min)+(g<b?6:0))/6;else if(max===g)h=((b-r)/(max-min)+2)/6;else h=((r-g)/(max-min)+4)/6}
    l=Math.max(0,Math.min(1,l+delta))
    function hue2rgb(p,q,t){if(t<0)t+=1;if(t>1)t-=1;if(t<1/6)return p+(q-p)*6*t;if(t<.5)return q;if(t<2/3)return p+(q-p)*(2/3-t)*6;return p}
    var q2=l<0.5?l*(1+s):l+s-l*s, p2=2*l-q2
    var r2=s===0?l:hue2rgb(p2,q2,h+1/3)
    var g2=s===0?l:hue2rgb(p2,q2,h)
    var b2=s===0?l:hue2rgb(p2,q2,h-1/3)
    function toHex(x){var v=Math.round(x*255).toString(16);return v.length===1?"0"+v:v}
    return "#"+toHex(r2)+toHex(g2)+toHex(b2)
  }

  // Apply a Material You dark-scheme color object from matugen output.
  // Handles both matugen v1 (flat) and v2 (nested dark/light) JSON shapes.
  function applyMatugen(raw) {
    var c = (raw.dark || raw.colors && raw.colors.dark) ? (raw.dark || raw.colors.dark) : raw
    var bgBase = c.background || c.bg || "#1c1b1f"
    // Derive background scale
    black   = lightenHex(bgBase, -0.04)
    bgDim   = lightenHex(bgBase, -0.02)
    bg      = bgBase
    surface = lightenHex(bgBase, 0.035)
    card    = lightenHex(bgBase, 0.07)
    input   = lightenHex(bgBase, 0.095)
    raised  = lightenHex(bgBase, 0.13)
    border  = lightenHex(bgBase, 0.20)
    // Text
    fg      = c.on_background || c.on_surface || "#e6e1e9"
    fgMuted = c.on_surface_variant || lightenHex(bgBase, 0.50)
    fgDim   = c.outline || lightenHex(bgBase, 0.35)
    // Accents — map Material You roles to our semantic slots
    green   = c.primary           || "#b4e49a"
    blue    = c.tertiary          || "#98d4e7"
    purple  = c.secondary         || "#bdb2ff"
    red     = c.error             || "#ff8c9a"
    orange  = c.primary_container || c.secondary_container || lightenHex(c.primary || bgBase, 0.15)
    yellow  = c.secondary_container || c.tertiary_container || lightenHex(c.tertiary || bgBase, 0.15)
    grey    = c.on_surface_variant || lightenHex(bgBase, 0.50)
    currentTheme = 3
  }

  // ── Theme definitions ─────────────────────────────────────────────────────
  readonly property var themes: [
    {
      name: "Ponokai",
      description: "Blue-dark, cool pastels",
      black:   "#181a1c", bgDim: "#252630",
      bg:      "#2b2d3a", surface: "#333648",
      card:    "#363a4e", input:   "#393e53", raised: "#3f445b",
      border:  "#585d74",
      fg:      "#f2f2f3", fgMuted: "#c4c6cf", fgDim: "#9da1af",
      red:     "#ff8c9a", orange:  "#f3bb9a", yellow: "#f8e7b0",
      green:   "#b4e49a", blue:    "#98d4e7", purple: "#bdb2ff",
      grey:    "#c4c6cf"
    },
    {
      name: "Shusia",
      description: "Purple-dark, warm pastels",
      black:   "#191619", bgDim: "#232025",
      bg:      "#2d2a2e", surface: "#302d31",
      card:    "#383438", input:   "#3b383c", raised: "#413e42",
      border:  "#4b4850",
      fg:      "#e3e3e3", fgMuted: "#848089", fgDim: "#69676c",
      red:     "#f85e84", orange:  "#ef9062", yellow: "#e5c463",
      green:   "#9ecd6f", blue:    "#7accd7", purple: "#ab9df2",
      grey:    "#848089"
    },
    {
      name: "Warm",
      description: "Amber-dark, cozy tones",
      black:   "#1c1a14", bgDim: "#232015",
      bg:      "#2a2619", surface: "#332f21",
      card:    "#3a3628", input:   "#3f3a2c", raised: "#474230",
      border:  "#565040",
      fg:      "#f0e8d5", fgMuted: "#b8aa90", fgDim: "#857a62",
      red:     "#e88a78", orange:  "#e8a060", yellow: "#f0cc78",
      green:   "#b8d882", blue:    "#88c4bc", purple: "#c8a8e0",
      grey:    "#b8aa90"
    }
  ]

  function applyTheme(idx) {
    if (idx < 0 || idx >= themes.length) return
    currentTheme = idx
    var t = themes[idx]
    black   = t.black;   bgDim   = t.bgDim
    bg      = t.bg;      surface = t.surface
    card    = t.card;    input   = t.input;   raised = t.raised
    border  = t.border
    fg      = t.fg;      fgMuted = t.fgMuted; fgDim  = t.fgDim
    red     = t.red;     orange  = t.orange;  yellow = t.yellow
    green   = t.green;   blue    = t.blue;    purple = t.purple
    grey    = t.grey
  }
}
