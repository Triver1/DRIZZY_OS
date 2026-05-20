import Quickshell
import Quickshell.Wayland
import QtQuick

PanelWindow {
  id: launcherWin
  property var shell

  screen: shell.modelData
  anchors { top: true; left: true }
  margins { top: 42; left: 10 }
  implicitWidth: shell.launcherOpen ? 560 : 0
  implicitHeight: shell.launcherOpen ? 420 : 0
  visible: shell.launcherOpen
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"

  WlrLayershell.namespace: "quickshell-launcher"

  Behavior on implicitWidth { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }
  Behavior on implicitHeight { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }

  Rectangle {
    anchors.fill: parent; radius: 16
    color: "#D92B2D3A"; border.color: "#1AF2F2F3"; border.width: 1

    GridView {
      id: appGrid
      anchors { fill: parent; margins: 16; topMargin: 40 }
      cellWidth: 80; cellHeight: 80
      model: DesktopEntries.applications
      clip: true

      delegate: Rectangle {
        width: 72; height: 72; radius: 14
        color: appHover.hovered ? "#22333648" : "transparent"
        Behavior on color { ColorAnimation { duration: 150 } }

        Column {
          anchors.centerIn: parent; spacing: 4
          Image {
            anchors.horizontalCenter: parent.horizontalCenter
            width: 32; height: 32
            source: model.icon ? Quickshell.iconPath(model.icon) : ""
            sourceSize.width: 32; sourceSize.height: 32
            asynchronous: true; fillMode: Image.PreserveAspectFit; smooth: true
          }
          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: model.name || ""; color: "#F2F2F3"
            font.pixelSize: 9; font.weight: Font.Medium
            elide: Text.ElideRight; width: 68
            horizontalAlignment: Text.AlignHCenter; maximumLineCount: 2; wrapMode: Text.WordWrap
          }
        }

        HoverHandler { id: appHover; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: shell.launchApp(model) }
      }
    }

    TextInput {
      id: searchBox
      anchors { top: parent.top; topMargin: 8; left: parent.left; leftMargin: 16; right: parent.right; rightMargin: 16 }
      height: 28; color: "#F2F2F3"; font.pixelSize: 12
      verticalAlignment: Text.AlignVCenter; leftPadding: 10

      Text {
        anchors.fill: parent; text: "Search apps..."
        color: "#9DA1AF"; font.pixelSize: 12
        verticalAlignment: Text.AlignVCenter; leftPadding: 10
        visible: !searchBox.text && !searchBox.activeFocus
      }

      Rectangle {
        anchors.fill: parent; z: -1; radius: 8
        color: "#B2333648"; border.color: "#1AF2F2F3"; border.width: 1
      }
    }
  }
}
