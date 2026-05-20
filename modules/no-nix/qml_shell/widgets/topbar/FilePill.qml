import QtQuick
import "../../theme"

Item {
  id: root
  property var shell
  property bool dragHovered: false

  width: 30
  height: 24

  Rectangle {
    anchors.fill: parent
    radius: 12
    color: root.shell && root.shell.fileBufferOpen ? "#e0383438" : "#e0302d31"
    border.color: root.shell && root.shell.fileBufferOpen ? Theme.accent : Theme.border
    border.width: 1

    Behavior on color { ColorAnimation { duration: 160 } }
    Behavior on border.color { ColorAnimation { duration: 160 } }

    DropArea {
      id: iconDrop
      anchors.fill: parent
      onEntered: function(drag) {
        if (!root.shell) return
        if (drag.hasUrls || drag.hasText) {
          root.dragHovered = true
          root.shell.fileBufferIconHovered = true
          root.shell.showFileBuffer()
          drag.acceptProposedAction()
        }
      }
      onExited: {
        root.dragHovered = false
        if (!root.shell) return
        root.shell.fileBufferIconHovered = false
        root.shell.hideFileBufferSoon()
      }
      onDropped: function(drop) {
        root.dragHovered = false
        if (!root.shell) return
        root.shell.fileBufferIconHovered = false
        root.shell.submitFileBufferDrop(drop.hasUrls ? drop.urls : [], drop.hasText ? drop.text : "")
        drop.acceptProposedAction()
      }
    }

    Text {
      anchors.centerIn: parent
      text: "󰉋"
      color: root.dragHovered || (root.shell && root.shell.fileBufferOpen) ? Theme.accent : Theme.blue
      font.pixelSize: 13

      Behavior on color { ColorAnimation { duration: 150 } }
    }

    Rectangle {
      id: countBadge
      visible: root.shell && root.shell.fileBufferCount > 0
      width: Math.max(12, countText.implicitWidth + 6)
      height: 12
      radius: 6
      anchors { right: parent.right; top: parent.top; topMargin: -5 }
      color: Theme.accent

      Text {
        id: countText
        anchors.centerIn: parent
        text: root.shell ? Math.min(root.shell.fileBufferCount, 9) : ""
        color: Theme.black
        font.pixelSize: 8
        font.weight: Font.Bold
      }
    }

    HoverHandler {
      cursorShape: Qt.PointingHandCursor
      onHoveredChanged: {
        if (!root.shell) return
        root.shell.fileBufferIconHovered = hovered
        if (hovered) root.shell.showFileBuffer()
        else root.shell.hideFileBufferSoon()
      }
    }

    TapHandler {
      onTapped: {
        if (!root.shell) return
        if (root.shell.fileBufferOpen) {
          root.shell.fileBufferIconHovered = false
          root.shell.fileBufferPanelHovered = false
          root.shell.fileBufferDragging = false
          root.shell.fileBufferOpen = false
        } else {
          root.shell.showFileBuffer()
        }
      }
    }
  }
}
