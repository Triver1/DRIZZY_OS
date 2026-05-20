import QtQuick
import Qt.labs.folderlistmodel
import "../../theme"

Item {
  id: root
  property var shell

  GridView {
    id: wpGrid
    anchors { fill: parent; margins: 16; topMargin: 8 }
    cellWidth:  Math.floor(width / 4)
    cellHeight: 118
    clip: true

    FolderListModel {
      id: wpModel
      folder: "file:///home/drizzy/Pictures/Wallpapers"
      nameFilters: ["*.png", "*.jpg", "*.jpeg", "*.webp"]
      showDirs: false
    }

    model: wpModel

    delegate: Item {
      width: wpGrid.cellWidth; height: wpGrid.cellHeight

      Rectangle {
        anchors { fill: parent; margins: 4 }
        radius: 14
        color: "transparent"
        border.color: wpHover.hovered ? Theme.accent : Theme.border
        border.width: wpHover.hovered ? 2 : 1
        clip: true

        Behavior on border.color { ColorAnimation { duration: 150 } }
        Behavior on border.width { NumberAnimation { duration: 150 } }

        Image {
          anchors { fill: parent; margins: 0 }
          source: fileUrl
          fillMode: Image.PreserveAspectCrop
          asynchronous: true; smooth: true; cache: false
          opacity: wpHover.hovered ? 1.0 : 0.85
          Behavior on opacity { NumberAnimation { duration: 150 } }
          layer.enabled: true
        }

        Rectangle {
          anchors { bottom: parent.bottom; left: parent.left; right: parent.right }
          height: 28; radius: 14
          color: "#88000000"
          visible: wpHover.hovered

          Text {
            anchors.centerIn: parent
            text: "Set Wallpaper"
            color: "#ffffff"; font.pixelSize: 10; font.weight: Font.DemiBold
          }
        }
      }

      HoverHandler { id: wpHover; cursorShape: Qt.PointingHandCursor }
      TapHandler {
        onTapped: {
          shell.wallpaperPath = fileUrl
          // onWallpaperPathChanged in ScreenShell handles matugen if theme === 3
        }
      }
    }
  }
}
