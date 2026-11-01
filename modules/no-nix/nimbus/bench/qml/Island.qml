import QtQuick

// A floating glass island (nimbus `Island`): staggered spring entrance from
// 12 px above, hover lift, press, and a cursor-tracking specular.
Item {
    id: root

    default property alias content: contentItem.data

    property int delayMs: 0
    property real enter: 0
    property real show: 1

    readonly property bool hovered: ma.containsMouse
    readonly property real mouseX: ma.mouseX
    readonly property real mouseY: ma.mouseY

    property real lift: hovered ? 1 : 0
    property real press: ma.pressed ? 1 : 0
    property real spotX: 0
    property real spotY: 0

    readonly property real alpha: Math.max(0, Math.min(1, enter)) * Math.max(0, Math.min(1, show))
    readonly property real dy: -(1 - enter) * 12
    // Glass body as drawn (inflated on hover), for blur / input regions.
    readonly property real grow: glass.grow

    height: 30
    opacity: alpha

    // ENTER spring (0.62 / 0.68): slight overshoot as it lands.
    Behavior on enter { Spring { response: 0.62; zeta: 0.68 } }
    Behavior on show { Spring { response: 0.46; zeta: 1.0 } }
    Behavior on lift { Spring { response: 0.30; zeta: 0.72 } }
    Behavior on press { Spring { response: 0.30; zeta: 0.72 } }
    // nimbus POINTER is 0.14 / 1.0; Qt's fixed-step integrator turns that into
    // per-step ringing, so use a slightly slower critically damped follow.
    Behavior on spotX { id: sxB; Spring { response: 0.2; zeta: 1.0; epsilon: 0.05 } }
    Behavior on spotY { id: syB; Spring { response: 0.2; zeta: 1.0; epsilon: 0.05 } }

    Timer {
        id: delay
        interval: root.delayMs
        onTriggered: root.enter = 1
    }

    Component.onCompleted: {
        if (delayMs <= 0)
            enter = 1
        else
            delay.start()
    }

    MouseArea {
        id: ma
        anchors.fill: parent
        hoverEnabled: true
        onEntered: {
            sxB.enabled = false
            syB.enabled = false
            root.spotX = mouseX
            root.spotY = mouseY
            sxB.enabled = true
            syB.enabled = true
        }
        onPositionChanged: mouse => {
            root.spotX = mouse.x
            root.spotY = mouse.y
        }
    }

    Glass {
        id: glass
        anchors.fill: parent
        lift: root.lift
        press: root.press
        spotX: root.spotX
        spotY: root.spotY
    }

    Item {
        id: contentItem
        anchors.fill: parent
    }
}
