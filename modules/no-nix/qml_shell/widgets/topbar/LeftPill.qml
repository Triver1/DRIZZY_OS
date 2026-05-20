import QtQuick
import Niri 0.1
import "../../theme"

Item {
  id: root
  property var shell
  property var niri

  width: pill.width
  height: 24

  Rectangle {
    id: pill
    height: 24
    width: pillRow.implicitWidth + 20
    radius: 12
    color: "#e0302d31"
    border.color: Theme.accent; border.width: 1

    Behavior on width { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }

    Row {
      id: pillRow
      anchors { left: parent.left; leftMargin: 10; verticalCenter: parent.verticalCenter }
      spacing: 10

      // App launcher / menu toggle
      Text {
        id: menuBtn
        property bool isLauncher: root.shell.launcherOpen
        text: isLauncher ? "✕" : "⊞"
        color: isLauncher ? Theme.red : Theme.accent
        font.pixelSize: 13
        anchors.verticalCenter: parent.verticalCenter

        Behavior on color { ColorAnimation { duration: 150 } }

        HoverHandler { cursorShape: Qt.PointingHandCursor }
        TapHandler {
          onTapped: {
            if (root.shell.launcherOpen) {
              root.shell.launcherOpen = false
            } else {
              root.shell.launcherOpen = true
              root.shell.dockOpen = false
            }
          }
        }
      }

      // Workspace dots
      Row {
        spacing: 5
        anchors.verticalCenter: parent.verticalCenter

        Repeater {
          model: root.niri.workspaces

          Rectangle {
            width: model.isFocused ? 18 : 8
            height: 8; radius: 4
            color: model.isFocused
              ? Theme.accent
              : (model.isActive ? Theme.blue : Theme.fgMuted)

            Behavior on width { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
            Behavior on color { ColorAnimation { duration: 160 } }

            HoverHandler { cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: root.niri.focusWorkspaceById(model.id) }
          }
        }
      }
    }
  }
}
