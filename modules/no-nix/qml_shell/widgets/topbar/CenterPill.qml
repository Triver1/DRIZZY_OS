import QtQuick
import Quickshell
import "../../theme"

Item {
  id: root
  property var shell
  property var dateClock

  width: pill.width
  height: 24

  Rectangle {
    id: pill
    width: pillRow.implicitWidth + 28
    height: 24; radius: 12
    color: root.shell.dockOpen ? "#e0383438" : "#e0302d31"
    border.color: root.shell.dockOpen ? Theme.accent : Theme.border
    border.width: 1

    Behavior on color { ColorAnimation { duration: 220 } }
    Behavior on border.color { ColorAnimation { duration: 220 } }

    Row {
      id: pillRow
      anchors.centerIn: parent; spacing: 10

      Text {
        text: Qt.formatDateTime(root.dateClock.date, "ddd d")
        color: Theme.fgMuted; font.pixelSize: 11; font.weight: Font.Medium
        anchors.verticalCenter: parent.verticalCenter
      }

      Rectangle {
        width: 1; height: 12; color: Theme.border
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        text: Qt.formatDateTime(root.dateClock.date, "HH:mm")
        color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold
        anchors.verticalCenter: parent.verticalCenter
      }
    }

    HoverHandler {
      cursorShape: Qt.PointingHandCursor
      onHoveredChanged: if (hovered) root.shell.dockOpen = true
    }
  }
}
