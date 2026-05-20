import QtQuick
import QtQuick.Layouts
import Quickshell.Services.Pipewire
import "../../theme"

Item {
  id: root

  PwObjectTracker {
    objects: [ Pipewire.defaultAudioSink, Pipewire.defaultAudioSource ]
  }

  readonly property var sink:   Pipewire.defaultAudioSink
  readonly property var source: Pipewire.defaultAudioSource

  Flickable {
    anchors.fill: parent
    contentWidth: width
    contentHeight: col.implicitHeight + 32
    clip: true

    Column {
      id: col
      width: parent.width
      spacing: 0

      Item { width: 1; height: 24 }

      // ── Output ───────────────────────────────────────────────────────────────

      SectionHeader { label: "Output" }

      OutlineCard {
        width: parent.width
        visible: root.sink !== null

        Column {
          width: parent.width; spacing: 20

          // Device name + mute toggle
          RowLayout {
            width: parent.width; spacing: 12

            Rectangle {
              width: 44; height: 44; radius: 12
              color: Qt.rgba(Theme.purple.r, Theme.purple.g, Theme.purple.b, 0.12)
              Text { anchors.centerIn: parent; text: "󰕾"; color: Theme.purple; font.pixelSize: 22 }
            }

            Column {
              Layout.fillWidth: true; spacing: 2
              Text {
                text: root.sink ? (root.sink.description || root.sink.name || "Audio Output") : "No Output"
                color: Theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold
                elide: Text.ElideRight; width: parent.width
              }
              Text {
                text: root.sink && root.sink.audio && root.sink.audio.muted ? "Muted" : "Active"
                color: root.sink && root.sink.audio && root.sink.audio.muted ? Theme.red : Theme.fgMuted
                font.pixelSize: 11
              }
            }

            MuteButton {
              muted: root.sink && root.sink.audio ? root.sink.audio.muted : false
              onToggle: { if (root.sink && root.sink.audio) root.sink.audio.muted = !root.sink.audio.muted }
            }
          }

          // Volume slider
          Column {
            width: parent.width; spacing: 8

            RowLayout {
              width: parent.width
              Text { text: "Volume"; color: Theme.fgMuted; font.pixelSize: 11 }
              Item { Layout.fillWidth: true }
              Text {
                text: root.sink && root.sink.audio
                  ? Math.round(root.sink.audio.volume * 100) + "%" : "—"
                color: Theme.purple; font.pixelSize: 12; font.weight: Font.DemiBold
              }
            }

            AudioSlider {
              width: parent.width
              value: root.sink && root.sink.audio ? root.sink.audio.volume : 0
              sliderColor: Theme.purple
              onMoved: function(v) { if (root.sink && root.sink.audio) root.sink.audio.volume = v }
            }
          }
        }
      }

      OutlineCard {
        width: parent.width
        visible: root.sink === null
        height: 56

        Text {
          anchors.centerIn: parent
          text: "No audio output found"
          color: Theme.fgDim; font.pixelSize: 12
        }
      }

      Item { width: 1; height: 24 }

      // ── Input ─────────────────────────────────────────────────────────────

      SectionHeader { label: "Input" }

      OutlineCard {
        width: parent.width
        visible: root.source !== null

        Column {
          width: parent.width; spacing: 20

          RowLayout {
            width: parent.width; spacing: 12

            Rectangle {
              width: 44; height: 44; radius: 12
              color: Qt.rgba(Theme.blue.r, Theme.blue.g, Theme.blue.b, 0.12)
              Text { anchors.centerIn: parent; text: "󰍬"; color: Theme.blue; font.pixelSize: 22 }
            }

            Column {
              Layout.fillWidth: true; spacing: 2
              Text {
                text: root.source ? (root.source.description || root.source.name || "Microphone") : "No Input"
                color: Theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold
                elide: Text.ElideRight; width: parent.width
              }
              Text {
                text: root.source && root.source.audio && root.source.audio.muted ? "Muted" : "Active"
                color: root.source && root.source.audio && root.source.audio.muted ? Theme.red : Theme.fgMuted
                font.pixelSize: 11
              }
            }

            MuteButton {
              muted: root.source && root.source.audio ? root.source.audio.muted : false
              onToggle: { if (root.source && root.source.audio) root.source.audio.muted = !root.source.audio.muted }
            }
          }

          Column {
            width: parent.width; spacing: 8

            RowLayout {
              width: parent.width
              Text { text: "Gain"; color: Theme.fgMuted; font.pixelSize: 11 }
              Item { Layout.fillWidth: true }
              Text {
                text: root.source && root.source.audio
                  ? Math.round(root.source.audio.volume * 100) + "%" : "—"
                color: Theme.blue; font.pixelSize: 12; font.weight: Font.DemiBold
              }
            }

            AudioSlider {
              width: parent.width
              value: root.source && root.source.audio ? root.source.audio.volume : 0
              sliderColor: Theme.blue
              onMoved: function(v) { if (root.source && root.source.audio) root.source.audio.volume = v }
            }
          }
        }
      }

      Item { width: 1; height: 24 }

      // ── Quick controls ─────────────────────────────────────────────────────

      SectionHeader { label: "Quick Controls" }

      Row {
        width: parent.width; spacing: 8

        Repeater {
          model: [
            { label: "Mute Output",  icon: "󰝟", action: "mute-sink"   },
            { label: "Mute Input",   icon: "󰍭", action: "mute-source" },
          ]

          Rectangle {
            width: (parent.width - 8) / 2; height: 64; radius: 14
            color: Theme.card; border.color: Theme.border; border.width: 1
            property bool active: {
              if (modelData.action === "mute-sink")
                return root.sink && root.sink.audio && root.sink.audio.muted
              return root.source && root.source.audio && root.source.audio.muted
            }

            Column {
              anchors.centerIn: parent; spacing: 4
              Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: modelData.icon; font.pixelSize: 22
                color: parent.parent.active ? Theme.red : Theme.fgMuted
                Behavior on color { ColorAnimation { duration: 150 } }
              }
              Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: modelData.label; color: Theme.fgMuted; font.pixelSize: 10
              }
            }

            HoverHandler { cursorShape: Qt.PointingHandCursor }
            TapHandler {
              onTapped: {
                if (modelData.action === "mute-sink" && root.sink && root.sink.audio)
                  root.sink.audio.muted = !root.sink.audio.muted
                else if (root.source && root.source.audio)
                  root.source.audio.muted = !root.source.audio.muted
              }
            }
          }
        }
      }

      Item { width: 1; height: 24 }
    }
  }

  component SectionHeader: Item {
    property string label: ""
    width: parent.width; height: 32
    Text {
      x: 0; anchors.verticalCenter: parent.verticalCenter
      text: label.toUpperCase()
      color: Theme.fgDim; font.pixelSize: 10; font.weight: Font.Bold; font.letterSpacing: 1.5
    }
  }

  component OutlineCard: Rectangle {
    default property alias content: inn.data
    width: parent.width
    height: inn.implicitHeight + 32
    radius: 14
    color: Theme.card; border.color: Theme.border; border.width: 1
    Column {
      id: inn
      anchors { fill: parent; margins: 16 }
      spacing: 0
    }
  }

  component MuteButton: Rectangle {
    property bool muted: false
    signal toggle()
    width: 36; height: 36; radius: 10
    color: muted ? Qt.rgba(Theme.red.r, Theme.red.g, Theme.red.b, 0.15) : Theme.input
    border.color: muted ? Theme.red : Theme.border; border.width: 1
    Behavior on color { ColorAnimation { duration: 150 } }

    Text { anchors.centerIn: parent; text: muted ? "󰝟" : "󰕾"; color: muted ? Theme.red : Theme.fgMuted; font.pixelSize: 16 }
    HoverHandler { cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: parent.toggle() }
  }

  component AudioSlider: Item {
    property real value: 0.5
    property color sliderColor: Theme.accent
    signal moved(real v)
    height: 20

    Rectangle {
      anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter }
      height: 6; radius: 3; color: Theme.bg

      Rectangle {
        width: parent.width * Math.max(0, Math.min(1.5, parent.parent.value))
        height: 6; radius: 3; color: parent.parent.sliderColor
        Behavior on width { NumberAnimation { duration: 40 } }

        Rectangle {
          anchors { right: parent.right; verticalCenter: parent.verticalCenter }
          width: 16; height: 16; radius: 8; color: Theme.fg
          border.color: parent.parent.color; border.width: 2
        }
      }
    }

    MouseArea {
      anchors { fill: parent; margins: -8 }
      cursorShape: Qt.PointingHandCursor
      function update(mx) {
        var v = Math.max(0, Math.min(1.5, mx / parent.width))
        parent.value = v; parent.moved(v)
      }
      onPressed: function(e) { update(e.x) }
      onPositionChanged: function(e) { if (pressed) update(e.x) }
    }
  }
}
