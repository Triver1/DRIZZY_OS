import QtQuick
import QtQuick.Effects

// QML twin of nimbus/src/bar.rs: three floating glass islands.
//
//   [ spaces ]            [ window title ]            [ status · clock ]
//
// Pure QtQuick (no Quickshell types) so it can also be rendered offscreen.
Item {
    id: bar

    // Bench state index; -1 until the first synthetic snapshot arrives.
    property int n: -1
    property var now: new Date()

    // ── bench workload (services.rs bench_snapshot) ───────────────────────
    readonly property bool have: n >= 0
    readonly property var walk: [0, 1, 2, 3, 4, 3, 2, 1]
    readonly property var wins: [2, 1, 0, 3, 0]
    readonly property var titles: [
        "nimbus — ~/NEWFLAKE/modules/no-nix/nimbus",
        "Helium — Wayland layer shell protocol",
        "yazi",
        "Spotify Premium"
    ]
    readonly property int active: have ? walk[n % 8] : -1
    readonly property string title: have ? titles[n % 4] : ""

    // Static status (no polling in the twin).
    readonly property string volText: "72%"
    readonly property string batText: "43%"

    // ── layout (bar.rs consts) ────────────────────────────────────────────
    readonly property real islandY: 7
    readonly property real islandH: 30
    readonly property real margin: 9
    readonly property real slotW: 17
    readonly property real activeW: 36
    readonly property real spacesPad: 8
    readonly property real dotSize: 6
    readonly property real indH: 8
    readonly property real indInset: 5
    readonly property real iconSize: 16
    readonly property real titleMax: 420

    // ── palette (theme.rs) ────────────────────────────────────────────────
    readonly property color cText: "#eef0f7"
    readonly property color cMuted: "#a3aabb"
    readonly property color accentA: "#8fb3ff"
    readonly property color accentB: "#c69bff"
    readonly property color dotEmpty: Qt.rgba(1, 1, 1, 0.16)
    readonly property color dotUsed: Qt.rgba(0xdf / 255, 0xe4 / 255, 0xf2 / 255, 0.62)

    readonly property font fTitle: Qt.font({ family: "Inter", pixelSize: 13, weight: Font.Medium })
    readonly property font fClock: Qt.font({ family: "Inter", pixelSize: 14, weight: Font.DemiBold })
    readonly property font fDate: Qt.font({ family: "Inter", pixelSize: 13, weight: Font.Medium })
    readonly property font fSmall: Qt.font({ family: "Inter", pixelSize: 12, weight: Font.DemiBold })

    // Geometry of the glass bodies (window coords) for blur / input regions.
    readonly property var islands: [spaces, center, status]

    function mix(a, b, t) {
        return Qt.rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t,
                       a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
    }
    function clamp(v, lo, hi) {
        return Math.max(lo, Math.min(hi, v))
    }

    onActiveChanged: spaces.applyWorkspaces()
    onTitleChanged: center.applyTitle()
    onNowChanged: status.applyClock(true)
    Component.onCompleted: status.applyClock(false)

    // ── workspaces ────────────────────────────────────────────────────────
    Island {
        id: spaces
        delayMs: 0
        x: bar.margin
        y: bar.islandY + dy
        width: bar.spacesPad * 2 + Math.max(row.implicitWidth, bar.slotW)

        // Indicator edges (island-local x). *T are targets; the visible edges
        // follow on springs whose configs swap with the travel direction.
        property real indLT: 0
        property real indRT: 0
        property real indL: indLT
        property real indR: indRT
        property bool movingRight: true
        property bool indInit: false
        property real ia: bar.have ? 1 : 0

        Behavior on indL {
            id: indLB
            Spring {
                // leading edge BOUNCY 0.42/0.58, trailing edge TAIL 0.58/0.82
                response: spaces.movingRight ? 0.58 : 0.42
                zeta: spaces.movingRight ? 0.82 : 0.58
                epsilon: 0.05
            }
        }
        Behavior on indR {
            id: indRB
            Spring {
                response: spaces.movingRight ? 0.42 : 0.58
                zeta: spaces.movingRight ? 0.58 : 0.82
                epsilon: 0.05
            }
        }
        Behavior on ia { Spring { response: 0.46; zeta: 1.0 } }

        readonly property real il: Math.min(indL, indR)
        readonly property real ir: Math.max(indL, indR)

        function applyWorkspaces() {
            if (bar.active < 0)
                return
            // Targets from the *target* layout.
            const l = bar.spacesPad + bar.slotW * bar.active + bar.indInset
            const r = l + bar.activeW - 2 * bar.indInset
            if (!indInit) {
                // First snapshot: snap.
                indInit = true
                indLB.enabled = false
                indRB.enabled = false
                indLT = l
                indRT = r
                indLB.enabled = true
                indRB.enabled = true
                return
            }
            movingRight = (l + r) > (indLT + indRT)
            indLT = l
            indRT = r
        }

        Row {
            id: row
            x: bar.spacesPad
            height: bar.islandH

            Repeater {
                model: bar.have ? 5 : 0

                Item {
                    id: slot
                    required property int index

                    width: index === bar.active ? bar.activeW : bar.slotW
                    height: bar.islandH
                    Behavior on width { Spring { response: 0.46; zeta: 1.0; epsilon: 0.05 } }

                    readonly property real cx: row.x + x + width / 2
                    readonly property bool hov: spaces.hovered
                        && spaces.mouseX >= row.x + x && spaces.mouseX < row.x + x + width
                    property real dotS: hov ? bar.dotSize + 3 : bar.dotSize
                    Behavior on dotS { Spring { response: 0.30; zeta: 0.72; epsilon: 0.01 } }

                    readonly property real solid: bar.wins[index] > 0 ? 1 : 0
                    readonly property color c: solid > 0 ? bar.dotUsed : bar.dotEmpty
                    // Dots the indicator passes over are absorbed into it.
                    readonly property real dist: Math.max(spaces.il - cx, cx - spaces.ir)
                    readonly property real free: 1 - spaces.ia * (1 - bar.clamp((dist + 1) / 7, 0, 1))
                    readonly property real d: dotS * (0.4 + 0.6 * free)
                    readonly property real fa: c.a * free * solid
                    readonly property real ra: c.a * free * (1 - solid * 0.6)

                    Rectangle {
                        visible: slot.free > 0.002
                        width: slot.d
                        height: slot.d
                        radius: slot.d / 2
                        x: (slot.width - slot.d) / 2
                        y: (bar.islandH - slot.d) / 2
                        color: Qt.rgba(slot.c.r, slot.c.g, slot.c.b, slot.fa)
                        border.width: 1.2
                        border.color: Qt.rgba(slot.c.r, slot.c.g, slot.c.b, slot.fa + slot.ra * (1 - slot.fa))
                    }
                }
            }
        }

        // Liquid indicator: squashes vertically as it stretches.
        Rectangle {
            id: ind
            readonly property real rest: bar.activeW - 2 * bar.indInset
            readonly property real stretch: ((spaces.ir - spaces.il) - rest) / rest
            readonly property real h: bar.indH * (1 - 0.32 * bar.clamp(stretch, 0, 1.2))
                                              * (1 + 0.18 * bar.clamp(-stretch, 0, 1))

            visible: spaces.ia > 0.002
            opacity: bar.clamp(spaces.ia, 0, 1)
            x: spaces.il
            width: spaces.ir - spaces.il
            height: h
            y: (bar.islandH - h) / 2
            radius: h / 2
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0.0; color: bar.accentA }
                GradientStop { position: 1.0; color: bar.accentB }
            }

            // Lit rim / inner glow approximation (white 0.55 top → 0.08 bottom).
            Rectangle {
                anchors.fill: parent
                radius: parent.radius
                gradient: Gradient {
                    GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, 0.42) }
                    GradientStop { position: 0.4; color: Qt.rgba(1, 1, 1, 0.08) }
                    GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, 0.06) }
                }
            }

            // Coloured bloom (σ 5 px, accent midpoint @ 0.42).
            layer.enabled: true
            layer.effect: MultiEffect {
                autoPaddingEnabled: true
                blurMax: 32
                shadowEnabled: true
                shadowColor: Qt.rgba((bar.accentA.r + bar.accentB.r) / 2,
                                     (bar.accentA.g + bar.accentB.g) / 2,
                                     (bar.accentA.b + bar.accentB.b) / 2, 0.42)
                shadowBlur: 10 / 32
                shadowHorizontalOffset: 0
                shadowVerticalOffset: 0
            }
        }
    }

    // ── window title ──────────────────────────────────────────────────────
    Island {
        id: center
        delayMs: 70
        y: bar.islandY + dy
        width: Math.max(tw, 32) * (0.9 + 0.1 * bar.clamp(show, 0, 1))
        x: (bar.width - width) / 2

        property string curT: ""
        property string prevT: ""
        property real xf: 1
        property real tw: 0
        Behavior on xf { id: xfB; Spring { response: 0.46; zeta: 1.0 } }
        Behavior on tw { id: twB; Spring { response: 0.46; zeta: 1.0; epsilon: 0.05 } }

        readonly property color tc: bar.mix(bar.cMuted, bar.cText, 0.35 + 0.65 * bar.clamp(lift, 0, 1))

        function applyTitle() {
            const t = bar.title
            if (t === curT)
                return
            const wasNone = curT === ""
            prevT = wasNone ? "" : curT
            curT = t
            xfB.enabled = false
            xf = 0
            xfB.enabled = true
            xf = 1
            show = t !== "" ? 1 : 0
            if (t === "")
                return
            const target = Math.min(curText.implicitWidth, bar.titleMax) + 32
            if (tw < 1) {
                twB.enabled = false
                tw = target * 0.7
                twB.enabled = true
            }
            tw = target
        }

        Item {
            id: titleClip
            x: 2
            y: 2
            width: parent.width - 4
            height: parent.height - 4
            clip: true

            Text {
                visible: center.xf < 1 && center.prevT !== ""
                text: center.prevT
                font: bar.fTitle
                color: center.tc
                elide: Text.ElideRight
                width: Math.min(implicitWidth, bar.titleMax)
                height: bar.islandH
                verticalAlignment: Text.AlignVCenter
                x: (bar.width - width) / 2 - center.x - titleClip.x
                y: -titleClip.y - center.xf * 7
                opacity: 1 - center.xf
            }
            Text {
                id: curText
                text: center.curT
                font: bar.fTitle
                color: center.tc
                elide: Text.ElideRight
                width: Math.min(implicitWidth, bar.titleMax)
                height: bar.islandH
                verticalAlignment: Text.AlignVCenter
                x: (bar.width - width) / 2 - center.x - titleClip.x
                y: -titleClip.y + (1 - center.xf) * 7
                opacity: center.xf
            }
        }
    }

    // ── status · clock ────────────────────────────────────────────────────
    Island {
        id: status
        delayMs: 140
        y: bar.islandY + dy
        width: total
        x: bar.width - bar.margin - total

        FontMetrics { id: fmSmall; font: bar.fSmall }
        FontMetrics { id: fmDate; font: bar.fDate }
        FontMetrics { id: fmClock; font: bar.fClock }

        readonly property real lifted: bar.clamp(lift, 0, 1)
        readonly property color iconC: bar.mix(bar.cMuted, bar.cText, lifted)

        readonly property string dateText: {
            const wd = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
            const mo = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
            const d = bar.now
            return wd[d.getDay()] + " " + d.getDate() + " " + mo[d.getMonth()]
        }
        readonly property real digitW: {
            let w = 0
            for (let i = 0; i < 10; i++)
                w = Math.max(w, fmClock.advanceWidth(String(i)))
            return w
        }
        readonly property real colonW: fmClock.advanceWidth(":")
        readonly property real volTW: fmSmall.advanceWidth(bar.volText)
        readonly property real batTW: fmSmall.advanceWidth(bar.batText)
        readonly property real dateTW: fmDate.advanceWidth(dateText)

        // Hover sub-regions (nimbus Hit::Volume / Hit::Clock).
        readonly property bool volHover: hovered && mouseX >= xVol - 6 && mouseX < xVol + volW + 6
        readonly property bool clockHover: hovered && mouseX >= xDate - 6
        property real vr: volHover ? 1 : 0
        property real dr: clockHover ? 1 : 0
        Behavior on vr { Spring { response: 0.46; zeta: 1.0 } }
        Behavior on dr { Spring { response: 0.46; zeta: 1.0 } }
        readonly property real vrc: bar.clamp(vr, 0, 1.2)
        readonly property real drc: bar.clamp(dr, 0, 1.2)

        readonly property real pl: 13
        readonly property real pr: 15
        readonly property real gap: 12
        readonly property real volW: bar.iconSize + (5 + volTW) * vrc
        readonly property real batW: bar.iconSize + 4 + batTW
        readonly property real dateW: (dateTW + 9) * drc
        readonly property real clockW: digitW * 4 + colonW + 2
        readonly property real xVol: pl
        readonly property real xNet: xVol + volW + gap
        readonly property real xBat: xNet + bar.iconSize + gap
        readonly property real xDiv: xBat + batW + gap
        readonly property real xDate: xDiv + 1 + gap
        readonly property real xClock: xDate + dateW
        readonly property real total: xClock + clockW + pr

        function applyClock(animate) {
            const d = bar.now
            const s = String(d.getHours()).padStart(2, "0") + String(d.getMinutes()).padStart(2, "0")
            const ds = [d0, d1, d2, d3]
            for (let i = 0; i < 4; i++)
                ds[i].set(s[i], animate)
        }

        // Volume (+ level revealed on hover).
        Icon {
            x: status.xVol
            y: (bar.islandH - size) / 2
            size: bar.iconSize
            name: "audio-volume-high-symbolic"
            tint: status.iconC
        }
        Item {
            visible: status.vrc > 0.002
            x: status.xVol + bar.iconSize
            width: (5 + status.volTW) * status.vrc
            height: bar.islandH
            clip: true
            Text {
                x: 5 - (1 - Math.min(status.vrc, 1)) * 6
                height: bar.islandH
                verticalAlignment: Text.AlignVCenter
                text: bar.volText
                font: bar.fSmall
                color: bar.cText
                opacity: Math.min(status.vrc, 1)
            }
        }

        // Network.
        Icon {
            x: status.xNet
            y: (bar.islandH - size) / 2
            size: bar.iconSize
            name: "network-wireless-signal-excellent-symbolic"
            tint: status.iconC
        }

        // Battery.
        Icon {
            x: status.xBat
            y: (bar.islandH - size) / 2
            size: bar.iconSize
            name: "battery-level-40-symbolic"
            tint: status.iconC
        }
        Text {
            x: status.xBat + bar.iconSize + 4
            height: bar.islandH
            verticalAlignment: Text.AlignVCenter
            text: bar.batText
            font: bar.fSmall
            color: status.iconC
        }

        // Hairline divider with a soft vertical fade.
        Rectangle {
            x: status.xDiv
            y: bar.islandH / 2 - 7
            width: 1
            height: 14
            radius: 0.5
            gradient: Gradient {
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, 0.04) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, 0.22) }
            }
        }

        // Date slides out from behind the clock on hover.
        Item {
            visible: status.drc > 0.002
            x: status.xDate
            width: status.dateW
            height: bar.islandH
            clip: true
            Text {
                x: status.dateW - status.dateTW - 9 + (1 - Math.min(status.drc, 1)) * 10
                height: bar.islandH
                verticalAlignment: Text.AlignVCenter
                text: status.dateText
                font: bar.fDate
                color: bar.cMuted
                opacity: Math.min(status.drc, 1)
            }
        }

        // Odometer clock: digits roll inside a window just taller than caps.
        Item {
            id: clockClip
            x: status.xClock - 2
            y: bar.islandH / 2 - 8.5
            width: status.clockW + 4
            height: 17
            clip: true

            // Glyph line box (18 px) centred on the island's midline.
            readonly property real ty: bar.islandH / 2 - 9 - y

            Digit {
                id: d0
                x: 2
                width: status.digitW
                height: 18
                ty: clockClip.ty
                delayMs: 180
                font: bar.fClock
                color: bar.cText
            }
            Digit {
                id: d1
                x: 2 + status.digitW
                width: status.digitW
                height: 18
                ty: clockClip.ty
                delayMs: 120
                font: bar.fClock
                color: bar.cText
            }
            Text {
                x: 2 + status.digitW * 2 + 1
                y: clockClip.ty - 1
                height: 18
                verticalAlignment: Text.AlignVCenter
                text: ":"
                font: bar.fClock
                color: bar.mix(bar.cMuted, bar.cText, 0.5)
            }
            Digit {
                id: d2
                x: 2 + status.digitW * 2 + status.colonW + 2
                width: status.digitW
                height: 18
                ty: clockClip.ty
                delayMs: 60
                font: bar.fClock
                color: bar.cText
            }
            Digit {
                id: d3
                x: 2 + status.digitW * 3 + status.colonW + 2
                width: status.digitW
                height: 18
                ty: clockClip.ty
                delayMs: 0
                font: bar.fClock
                color: bar.cText
            }
        }
    }
}
