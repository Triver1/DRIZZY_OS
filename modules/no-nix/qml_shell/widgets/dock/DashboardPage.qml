import QtQuick
import QtQuick.Layouts
import "../../theme"

Item {
  id: root
  property var shell
  property var stopwatchTimer
  property var notifModel

  RowLayout {
    anchors { fill: parent; margins: 14; topMargin: 8 }
    spacing: 14

    // Left column: media player
    MediaPlayer {
      Layout.preferredWidth: 200
      Layout.fillHeight: true
    }

    // Separator
    Rectangle {
      Layout.preferredWidth: 1
      Layout.fillHeight: true
      Layout.topMargin: 12
      Layout.bottomMargin: 12
      color: Theme.border
    }

    // Center column: quick settings
    QuickSettingsGrid {
      Layout.fillWidth: true
      Layout.fillHeight: true
      shell: root.shell
    }

    // Separator
    Rectangle {
      Layout.preferredWidth: 1
      Layout.fillHeight: true
      Layout.topMargin: 12
      Layout.bottomMargin: 12
      color: Theme.border
    }

    // Right column: activity / notifications
    Column {
      Layout.preferredWidth: 186
      Layout.fillHeight: true
      spacing: 8

      Text {
        text: "Activity"
        color: Theme.fgMuted
        font.pixelSize: 10
        font.weight: Font.Bold
        font.letterSpacing: 1.0
      }

      ActivityList {
        width: parent.width
        height: parent.height - 20
        notifModel: root.notifModel
      }
    }
  }
}
