import QtQuick
import Quickshell.Services.Mpris
import "../../theme"

Item {
  id: root

  property int currentIndex: 0
  readonly property int playerCount: Mpris.players.values.length
  readonly property var activePlayer: playerCount > 0 ? Mpris.players.values[currentIndex] : null

  onPlayerCountChanged: {
    if (currentIndex >= playerCount) currentIndex = Math.max(0, playerCount - 1)
  }

  Timer {
    id: posTimer
    interval: 200; repeat: true
    running: root.activePlayer && root.activePlayer.isPlaying
    onTriggered: { if (root.activePlayer) root.activePlayer.positionChanged() }
  }

  Column {
    anchors.centerIn: parent
    spacing: 10

    // Player picker
    Row {
      anchors.horizontalCenter: parent.horizontalCenter
      spacing: 6

      Text {
        text: "󰁅"; font.pixelSize: 13
        color: playerCount > 1 ? Theme.fgMuted : Theme.border
        anchors.verticalCenter: parent.verticalCenter
        HoverHandler { cursorShape: playerCount > 1 ? Qt.PointingHandCursor : Qt.ArrowCursor }
        TapHandler {
          onTapped: {
            if (playerCount > 1) currentIndex = (currentIndex - 1 + playerCount) % playerCount
          }
        }
      }

      Text {
        text: root.activePlayer ? root.activePlayer.identity : "No players"
        color: Theme.fgDim; font.pixelSize: 10
        anchors.verticalCenter: parent.verticalCenter
        elide: Text.ElideRight; width: 110
        horizontalAlignment: Text.AlignHCenter
      }

      Text {
        text: "󰁝"; font.pixelSize: 13
        color: playerCount > 1 ? Theme.fgMuted : Theme.border
        anchors.verticalCenter: parent.verticalCenter
        HoverHandler { cursorShape: playerCount > 1 ? Qt.PointingHandCursor : Qt.ArrowCursor }
        TapHandler {
          onTapped: {
            if (playerCount > 1) currentIndex = (currentIndex + 1) % playerCount
          }
        }
      }
    }

    // Album art disc
    Rectangle {
      width: 104; height: 104; radius: 52
      anchors.horizontalCenter: parent.horizontalCenter
      color: Theme.card
      border.color: Theme.accent; border.width: 2

      Image {
        anchors { fill: parent; margins: 4 }
        source: root.activePlayer && root.activePlayer.trackArtUrl
          ? root.activePlayer.trackArtUrl
          : ""
        fillMode: Image.PreserveAspectCrop
        layer.enabled: true
        layer.effect: null
        visible: source !== ""
      }

      Text {
        anchors.centerIn: parent
        visible: !root.activePlayer || !root.activePlayer.trackArtUrl
        text: "󰝚"; color: Theme.fgDim; font.pixelSize: 32
      }

      Rectangle {
        anchors.centerIn: parent
        width: 22; height: 22; radius: 11
        color: Theme.card
        border.color: Theme.accent; border.width: 1
      }

      RotationAnimation on rotation {
        from: 0; to: 360; duration: 14000
        loops: Animation.Infinite
        running: root.activePlayer && root.activePlayer.isPlaying
      }
    }

    // Player dots
    Row {
      spacing: 5
      anchors.horizontalCenter: parent.horizontalCenter
      Repeater {
        model: playerCount
        Rectangle {
          width: 6; height: 6; radius: 3
          color: index === currentIndex ? Theme.accent : Theme.border
          Behavior on color { ColorAnimation { duration: 150 } }
        }
      }
    }

    // Track info
    Column {
      spacing: 2
      anchors.horizontalCenter: parent.horizontalCenter
      width: 160

      Text {
        width: parent.width
        text: root.activePlayer ? (root.activePlayer.trackTitle || "Unknown Title") : "Not Playing"
        color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold
        elide: Text.ElideRight; horizontalAlignment: Text.AlignHCenter
      }
      Text {
        width: parent.width
        text: root.activePlayer ? (root.activePlayer.trackArtist || "") : "No media playing"
        color: Theme.fgMuted; font.pixelSize: 10
        elide: Text.ElideRight; horizontalAlignment: Text.AlignHCenter
      }
    }

    // Progress bar (visualizer bars)
    Item {
      width: 160; height: 28
      anchors.horizontalCenter: parent.horizontalCenter

      Row {
        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter }
        spacing: 2

        Text {
          text: root.activePlayer ? formatTime(root.activePlayer.position) : "0:00"
          color: Theme.fgDim; font.pixelSize: 9; width: 28
          anchors.verticalCenter: parent.verticalCenter
          horizontalAlignment: Text.AlignRight
        }

        Item {
          width: 96; height: 28
          anchors.verticalCenter: parent.verticalCenter

          TapHandler {
            onTapped: function(eventPoint) {
              if (!root.activePlayer || !root.activePlayer.length) return
              var frac = Math.max(0, Math.min(1, eventPoint.position.x / 96))
              root.activePlayer.position = frac * root.activePlayer.length
            }
          }
          HoverHandler { cursorShape: Qt.PointingHandCursor }

          Row {
            anchors.fill: parent
            spacing: 2

            Repeater {
              model: 14

              Rectangle {
                width: (96 - 13 * 2) / 14
                anchors.verticalCenter: parent.verticalCenter
                radius: 1
                color: Theme.accent

                readonly property real progress: root.activePlayer
                  ? Math.min(1, Math.max(0, root.activePlayer.position / (root.activePlayer.length || 1)))
                  : 0
                readonly property real barFrac: (index + 1) / 14
                readonly property real baseH: barFrac <= progress ? parent.height : 3

                height: {
                  if (!root.activePlayer || !root.activePlayer.isPlaying) return baseH
                  if (barFrac > progress) return 3
                  var seed = index * 0.7 + root.activePlayer.position * 0.3
                  return Math.max(3, 4 + Math.abs(Math.sin(seed)) * (parent.height - 4))
                }

                Behavior on height { NumberAnimation { duration: 130; easing.type: Easing.OutQuad } }
              }
            }
          }
        }

        Text {
          text: root.activePlayer ? formatTime(root.activePlayer.length) : "0:00"
          color: Theme.fgDim; font.pixelSize: 9; width: 28
          anchors.verticalCenter: parent.verticalCenter
        }
      }
    }

    // Controls
    Row {
      spacing: 18
      anchors.horizontalCenter: parent.horizontalCenter

      Text {
        text: "󰒮"; color: Theme.fgMuted; font.pixelSize: 16
        anchors.verticalCenter: parent.verticalCenter
        HoverHandler { cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: { if (root.activePlayer) root.activePlayer.previous() } }
      }

      Rectangle {
        width: 36; height: 36; radius: 18
        color: Theme.accent
        anchors.verticalCenter: parent.verticalCenter
        Text {
          anchors.centerIn: parent
          text: root.activePlayer && root.activePlayer.isPlaying ? "󰏤" : "󰐊"
          color: Theme.bg; font.pixelSize: 18
        }
        HoverHandler { cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: { if (root.activePlayer) root.activePlayer.togglePlaying() } }
      }

      Text {
        text: "󰒭"; color: Theme.fgMuted; font.pixelSize: 16
        anchors.verticalCenter: parent.verticalCenter
        HoverHandler { cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: { if (root.activePlayer) root.activePlayer.next() } }
      }
    }
  }

  function formatTime(secs) {
    if (!secs || secs <= 0) return "0:00"
    var m = Math.floor(secs / 60)
    var s = Math.floor(secs % 60)
    return m + ":" + (s < 10 ? "0" : "") + s
  }
}
