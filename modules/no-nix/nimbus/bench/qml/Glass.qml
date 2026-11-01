import QtQuick
import QtQuick.Effects
import QtQuick.Shapes

// One glass capsule body: drop shadow, vertical smoke gradient, top sheen
// (nimbus' inner rim glow), cursor-following specular and a 1 px rim that is
// lit from above. Content is NOT a child of this item; it sits on top.
Item {
    id: root

    // 0..1 hover lift, 0..1 press, specular centre in local coords.
    property real lift: 0
    property real press: 0
    property real spotX: 0
    property real spotY: 0

    // Inflate by lift, shrink on press (nimbus: r.inflate(l*1 - press*1.5)).
    readonly property real grow: lift * 1.0 - press * 1.5
    readonly property real r: body.height / 2
    readonly property real l: Math.max(0, Math.min(1.2, lift))

    function mix(a, b, t) {
        return Qt.rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t,
                       a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
    }

    // theme.rs
    readonly property color glassTop: Qt.rgba(0x1b / 255, 0x1d / 255, 0x2a / 255, 0.46)
    readonly property color glassBot: Qt.rgba(0x0d / 255, 0x0e / 255, 0x16 / 255, 0.58)
    readonly property color hoverTop: Qt.rgba(0x26 / 255, 0x29 / 255, 0x40 / 255, 0.54)
    readonly property color hoverBot: Qt.rgba(0x12 / 255, 0x13 / 255, 0x1f / 255, 0.64)

    // Opaque caster, only used as the shadow source and its own cut-out mask.
    Rectangle {
        id: caster
        anchors.fill: body
        radius: root.r
        color: "black"
        visible: false
        layer.enabled: true
    }

    // Drop shadow, clipped to outside the body so the glass stays clear
    // (inverted mask of the caster itself).
    MultiEffect {
        anchors.fill: caster
        source: caster
        autoPaddingEnabled: true
        blurMax: 32
        shadowEnabled: true
        shadowColor: Qt.rgba(0, 0, 0, 0.38 * (0.75 + 0.25 * root.l))
        // nimbus sigma 7 (+5 on hover); MultiEffect blur radius ≈ 2σ of blurMax.
        shadowBlur: (7 + 5 * root.l) * 2 / 32
        shadowVerticalOffset: 2.5 + 2 * root.l
        shadowHorizontalOffset: 0
        maskEnabled: true
        maskInverted: true
        maskSource: caster
    }

    Rectangle {
        id: body
        anchors.fill: parent
        anchors.margins: -root.grow
        radius: root.r
        gradient: Gradient {
            GradientStop { position: 0.0; color: root.mix(root.glassTop, root.hoverTop, root.l) }
            GradientStop { position: 1.0; color: root.mix(root.glassBot, root.hoverBot, root.l) }
        }
    }

    // Inner glow hugging the (lit) top edge: light refracting through the glass.
    Rectangle {
        anchors.fill: body
        anchors.margins: 1
        radius: root.r - 1
        gradient: Gradient {
            GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, 0.15) }
            GradientStop { position: 0.38; color: Qt.rgba(1, 1, 1, 0.03) }
            GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, 0.012) }
        }
    }

    // Specular highlight following the cursor (radius 70, k² falloff), only
    // while hovered. Clipped to the capsule because it is the capsule's fill.
    Shape {
        anchors.fill: body
        visible: root.l > 0.002
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: -1
            strokeColor: "transparent"
            fillGradient: RadialGradient {
                centerX: root.spotX + root.grow
                centerY: root.spotY + root.grow
                focalX: centerX
                focalY: centerY
                centerRadius: 70
                focalRadius: 0
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, 0.10 * root.l) }
                GradientStop { position: 0.25; color: Qt.rgba(1, 1, 1, 0.071 * root.l) }
                GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, 0.025 * root.l) }
                GradientStop { position: 0.75; color: Qt.rgba(1, 1, 1, 0.0024 * root.l) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, 0) }
            }
            PathRectangle {
                x: 0; y: 0
                width: body.width; height: body.height
                radius: root.r
            }
        }
    }

    // 1 px rim: white 0.30 at the top → 0.05 at the bottom (brighter on hover).
    Shape {
        anchors.fill: body
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: -1
            strokeColor: "transparent"
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0
                x2: 0; y2: body.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, Math.min(1, 0.30 * (1 + 0.35 * root.l))) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, 0.05) }
            }
            PathRectangle {
                x: 0; y: 0
                width: body.width; height: body.height
                radius: root.r
            }
            PathRectangle {
                x: 1; y: 1
                width: body.width - 2; height: body.height - 2
                radius: root.r - 1
            }
        }
    }
}
