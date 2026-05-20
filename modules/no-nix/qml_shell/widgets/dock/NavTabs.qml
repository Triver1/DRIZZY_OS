import QtQuick
import "../../theme"

Item {
  id: root
  property var dockWin

  width: navRow.width + 8
  height: 40

  Row {
    id: navRow
    anchors.centerIn: parent
    spacing: 6

    // Settings gear icon
    Rectangle {
      width: 30; height: 30; radius: 15
      color: gearHov.hovered
        ? Qt.rgba(Theme.fgMuted.r, Theme.fgMuted.g, Theme.fgMuted.b, 0.12) : "transparent"
      Behavior on color { ColorAnimation { duration: 160 } }

      Text {
        anchors.centerIn: parent; text: "󰒓"
        color: Theme.fgMuted; font.pixelSize: 15
      }
      HoverHandler { id: gearHov; cursorShape: Qt.PointingHandCursor }
      TapHandler { onTapped: dockWin.shell.settingsOpen = true }
    }

    Repeater {
      model: ["Dashboard", "Wallpapers", "Timer", "Kanban"]

      Rectangle {
        id: tab
        property bool active: dockWin.currentPage === modelData.toLowerCase()

        width: tabLabel.implicitWidth + 28; height: 30; radius: 15
        color: "transparent"
        border.color: active ? Theme.accent : Theme.border
        border.width: 1

        Behavior on color { ColorAnimation { duration: 160 } }
        Behavior on border.color { ColorAnimation { duration: 160 } }

        Text {
          id: tabLabel
          anchors.centerIn: parent
          text: modelData
          color: tab.active ? Theme.accent : Theme.fgMuted
          font.pixelSize: 12
          font.weight: tab.active ? Font.DemiBold : Font.Medium
          Behavior on color { ColorAnimation { duration: 160 } }
        }

        HoverHandler { cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: dockWin.shell.dockPage = modelData.toLowerCase() }
      }
    }
  }
}
