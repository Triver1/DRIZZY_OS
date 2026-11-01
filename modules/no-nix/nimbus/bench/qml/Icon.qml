import QtQuick
import QtQuick.Effects

// Adwaita symbolic icon, tinted with MultiEffect colorization.
Item {
    id: root

    property string name: ""
    property color tint: "white"
    property real size: 16

    width: size
    height: size

    Image {
        id: img
        anchors.fill: parent
        source: "file:///run/current-system/sw/share/icons/Adwaita/symbolic/status/" + root.name + ".svg"
        sourceSize.width: root.size * 2
        sourceSize.height: root.size * 2
        smooth: true
        fillMode: Image.PreserveAspectFit
        visible: false
    }

    MultiEffect {
        anchors.fill: parent
        source: img
        colorization: 1.0
        colorizationColor: root.tint
    }
}
