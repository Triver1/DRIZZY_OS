import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import "../../theme"

Item {
  id: root

  property var outputs: []
  property real brightness: 0.6
  property string applyStatus: ""

  function refresh() {
    outputProc.running = true
    briProc.running = true
  }

  onVisibleChanged: if (visible) refresh()
  Component.onCompleted: refresh()

  Process {
    id: outputProc
    running: false
    command: ["niri", "msg", "--json", "outputs"]
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          var raw = JSON.parse(text.trim())
          var list = []
          var keys = Object.keys(raw)
          for (var i = 0; i < keys.length; i++) {
            var o = raw[keys[i]]
            // resolve current_mode index to object
            var cm = (typeof o.current_mode === "number" && o.modes && o.modes.length > o.current_mode)
              ? o.modes[o.current_mode] : null
            list.push({ name: o.name, make: o.make || "", model: o.model || "",
              modes: o.modes || [], currentMode: cm,
              logical: o.logical || null, vrrSupported: o.vrr_supported || false,
              vrrEnabled: o.vrr_enabled || false })
          }
          root.outputs = list
        } catch(e) { root.outputs = [] }
      }
    }
  }

  Process {
    id: briProc
    running: false
    command: ["sh", "-c", "brightnessctl -m get && brightnessctl -m max"]
    stdout: StdioCollector {
      onStreamFinished: {
        var lines = text.trim().split("\n")
        if (lines.length >= 2) {
          var cur = parseInt(lines[0]); var max = parseInt(lines[1])
          if (max > 0) root.brightness = cur / max
        }
      }
    }
  }

  Process {
    id: cmdProc
    running: false
  }

  function exec(cmd) {
    cmdProc.command = ["sh", "-c", cmd]
    cmdProc.running = true
  }

  function applyMode(outputName, mode) {
    var hz = Math.round(mode.refresh_rate / 1000)
    exec("niri msg output \"" + outputName + "\" mode " + mode.width + "x" + mode.height + "@" + hz)
    root.applyStatus = "Applied — takes effect next login to persist."
    statusTimer.restart()
  }

  function applyScale(outputName, scale) {
    exec("niri msg output \"" + outputName + "\" scale " + scale.toFixed(2))
    root.applyStatus = "Scale changed to " + scale.toFixed(2) + "×."
    statusTimer.restart()
  }

  Timer {
    id: statusTimer
    interval: 4000
    onTriggered: root.applyStatus = ""
  }

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

      // Status message
      Rectangle {
        width: parent.width; height: 36; radius: 10
        color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12)
        border.color: Theme.accent; border.width: 1
        visible: root.applyStatus !== ""
        Text {
          anchors { fill: parent; leftMargin: 14 }
          text: root.applyStatus; color: Theme.accent
          font.pixelSize: 11; verticalAlignment: Text.AlignVCenter
        }
      }
      Item { width: 1; height: root.applyStatus !== "" ? 12 : 0 }

      // Brightness
      SectionHeader { label: "Brightness" }
      OutlineCard {
        width: parent.width

        Column {
          width: parent.width; spacing: 14

          RowLayout {
            width: parent.width
            Text { text: "󰃠"; color: Theme.yellow; font.pixelSize: 18 }
            Text { text: "Screen Brightness"; Layout.fillWidth: true; color: Theme.fg; font.pixelSize: 13 }
            Text {
              text: Math.round(root.brightness * 100) + "%"
              color: Theme.accent; font.pixelSize: 12; font.weight: Font.DemiBold
            }
          }

          SliderControl {
            width: parent.width; value: root.brightness; color: Theme.yellow
            onMoved: function(v) {
              root.brightness = v
              root.exec("brightnessctl set " + Math.round(v * 100) + "%")
            }
          }
        }
      }

      Item { width: 1; height: 24 }

      // Per-monitor sections
      SectionHeader { label: "Monitors" }

      Repeater {
        model: root.outputs

        Column {
          width: parent.width; spacing: 8

          OutlineCard {
            width: parent.width
            property var out: modelData || {}

            Column {
              width: parent.width; spacing: 16

              // Header row
              RowLayout {
                width: parent.width; spacing: 12

                Rectangle {
                  width: 44; height: 44; radius: 12
                  color: Qt.rgba(Theme.blue.r, Theme.blue.g, Theme.blue.b, 0.12)
                  Text { anchors.centerIn: parent; text: "󰍹"; color: Theme.blue; font.pixelSize: 22 }
                }

                Column {
                  Layout.fillWidth: true; spacing: 2
                  Text {
                    text: (out.make && out.make !== "Unknown" ? out.make + " " : "") + (out.model || out.name || "Monitor")
                    color: Theme.fg; font.pixelSize: 14; font.weight: Font.DemiBold
                  }
                  Text {
                    text: out.name || ""
                    color: Theme.fgMuted; font.pixelSize: 11
                  }
                }

                // Current mode badge
                Rectangle {
                  height: 28; radius: 8
                  width: modeLabel.implicitWidth + 20
                  color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12)
                  border.color: Theme.accent; border.width: 1
                  Text {
                    id: modeLabel
                    anchors.centerIn: parent
                    text: out.currentMode
                      ? out.currentMode.width + "×" + out.currentMode.height + " @" + Math.round(out.currentMode.refresh_rate / 1000) + "Hz"
                      : "Off"
                    color: Theme.accent; font.pixelSize: 10; font.weight: Font.DemiBold
                  }
                }
              }

              // Stats row
              RowLayout {
                width: parent.width; spacing: 8

                StatChip {
                  icon: "󰊿"
                  label: out.logical ? out.logical.scale.toFixed(2) + "×" : "—"
                  sublabel: "Scale"
                }
                StatChip {
                  icon: "󰉇"
                  label: out.logical
                    ? out.logical.width + "×" + out.logical.height
                    : "—"
                  sublabel: "Logical"
                }
                StatChip {
                  icon: "󰑓"
                  label: out.vrrEnabled ? "On" : "Off"
                  sublabel: "VRR"
                  enabled2: out.vrrSupported
                }
              }

              // Scale control
              Column {
                width: parent.width; spacing: 8

                RowLayout {
                  width: parent.width
                  Text { text: "Scale"; color: Theme.fgMuted; font.pixelSize: 11 }
                  Item { Layout.fillWidth: true }
                  Text {
                    text: out.logical ? out.logical.scale.toFixed(2) + "×" : "—"
                    color: Theme.fgDim; font.pixelSize: 11
                  }
                }

                Row {
                  spacing: 6

                  Repeater {
                    model: ["1.00", "1.25", "1.50", "1.75", "2.00", "2.25", "2.50"]

                    Rectangle {
                      height: 30; radius: 8
                      width: scaleLabel.implicitWidth + 16
                      property real scaleVal: parseFloat(modelData)
                      property bool isCurrent: out.logical && Math.abs(out.logical.scale - scaleVal) < 0.01
                      color: isCurrent ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : Theme.input
                      border.color: isCurrent ? Theme.accent : Theme.border; border.width: 1
                      Behavior on color { ColorAnimation { duration: 120 } }

                      Text {
                        id: scaleLabel
                        anchors.centerIn: parent; text: modelData
                        color: isCurrent ? Theme.accent : Theme.fgMuted; font.pixelSize: 11
                      }
                      HoverHandler { cursorShape: Qt.PointingHandCursor }
                      TapHandler {
                        onTapped: root.applyScale(out.name, scaleVal)
                      }
                    }
                  }
                }
              }

              // Mode selector — group by resolution, show Hz variants
              Column {
                width: parent.width; spacing: 8

                Text { text: "Resolution & Refresh Rate"; color: Theme.fgMuted; font.pixelSize: 11 }

                Flow {
                  width: parent.width; spacing: 6

                  Repeater {
                    model: out.modes || []

                    Rectangle {
                      height: 30; radius: 8
                      width: mLabel.implicitWidth + 16
                      property bool isCurrent: out.currentMode
                        && out.currentMode.width === modelData.width
                        && out.currentMode.height === modelData.height
                        && out.currentMode.refresh_rate === modelData.refresh_rate
                      color: isCurrent ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.18) : Theme.input
                      border.color: isCurrent ? Theme.accent : Theme.border; border.width: 1
                      Behavior on color { ColorAnimation { duration: 120 } }

                      Text {
                        id: mLabel
                        anchors.centerIn: parent
                        text: modelData.width + "×" + modelData.height + " @" + Math.round(modelData.refresh_rate / 1000) + "Hz"
                        color: isCurrent ? Theme.accent : Theme.fgMuted; font.pixelSize: 10
                      }
                      HoverHandler { cursorShape: Qt.PointingHandCursor }
                      TapHandler { onTapped: root.applyMode(out.name, modelData) }
                    }
                  }
                }
              }
            }
          }

          Item { width: 1; height: 8 }
        }
      }

      Item { width: 1; height: 8 }
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
    default property alias content: inner.data
    width: parent.width
    height: inner.implicitHeight + 32
    radius: 14; clip: true
    color: Theme.card; border.color: Theme.border; border.width: 1

    Column {
      id: inner
      anchors { fill: parent; margins: 16 }
      spacing: 0
    }
  }

  component StatChip: Rectangle {
    property string icon: ""
    property string label: ""
    property string sublabel: ""
    property bool enabled2: true

    Layout.fillWidth: true
    height: 56; radius: 10
    color: Theme.input; border.color: Theme.border; border.width: 1

    Column {
      anchors.centerIn: parent; spacing: 2
      Text { anchors.horizontalCenter: parent.horizontalCenter; text: icon; color: Theme.fgMuted; font.pixelSize: 14 }
      Text { anchors.horizontalCenter: parent.horizontalCenter; text: label; color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold }
      Text { anchors.horizontalCenter: parent.horizontalCenter; text: sublabel; color: Theme.fgDim; font.pixelSize: 9 }
    }
  }

  component SliderControl: Item {
    property real value: 0.5
    property color color: Theme.accent
    signal moved(real v)
    height: 20

    Rectangle {
      anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter }
      height: 6; radius: 3; color: Theme.bg

      Rectangle {
        width: parent.width * Math.max(0, Math.min(1, parent.parent.value))
        height: 6; radius: 3; color: parent.parent.color
        Behavior on width { NumberAnimation { duration: 40 } }

        Rectangle {
          anchors { right: parent.right; verticalCenter: parent.verticalCenter }
          width: 16; height: 16; radius: 8
          color: Theme.fg; border.color: parent.parent.color; border.width: 2
        }
      }
    }

    MouseArea {
      anchors { fill: parent; margins: -8 }
      cursorShape: Qt.PointingHandCursor
      function update(mx) {
        var v = Math.max(0, Math.min(1, mx / parent.width))
        parent.value = v; parent.moved(v)
      }
      onPressed: function(e) { update(e.x) }
      onPositionChanged: function(e) { if (pressed) update(e.x) }
    }
  }
}
