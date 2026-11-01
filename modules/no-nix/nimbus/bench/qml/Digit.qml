import QtQuick

// One odometer digit: the old glyph rolls up and out, the new one rolls in
// from below (nimbus `Digit`, DIGIT spring 0.38 / 0.74, roll 13 px).
Item {
    id: root

    property string cur: " "
    property string prev: " "
    property real p: 1
    property real delayMs: 0
    property font font
    property color color: "white"
    property real ty: 0

    readonly property real roll: 13

    function set(c, animate) {
        if (c === cur)
            return
        prev = cur
        cur = c
        if (animate) {
            pB.enabled = false
            p = 0
            pB.enabled = true
            if (delayMs <= 0)
                p = 1
            else
                delay.restart()
        } else {
            pB.enabled = false
            p = 1
            pB.enabled = true
        }
    }

    Behavior on p {
        id: pB
        Spring { response: 0.38; zeta: 0.74 }
    }

    Timer {
        id: delay
        interval: root.delayMs
        onTriggered: root.p = 1
    }

    Text {
        visible: root.p < 1 && root.prev !== " "
        width: root.width
        height: root.height
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignHCenter
        y: root.ty - root.p * root.roll
        text: root.prev
        font: root.font
        color: root.color
        opacity: Math.max(0, Math.min(1, 1 - root.p * 1.7))
    }

    Text {
        width: root.width
        height: root.height
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignHCenter
        y: root.ty + (1 - root.p) * root.roll
        text: root.cur
        font: root.font
        color: root.color
        opacity: Math.max(0, Math.min(1, (root.p - 0.2) / 0.8))
    }
}
