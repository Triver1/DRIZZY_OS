import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import "../../theme"

Item {
  id: root

  property int  batteryPct: 100
  property string batteryStatus: "Unknown"
  property string powerProfile: "balanced"
  property var  profiles: []
  property string uptimeStr: ""

  function refresh() {
    batteryProc.running   = true
    profileProc.running   = true
    uptimeProc.running    = true
  }

  onVisibleChanged: if (visible) refresh()
  Component.onCompleted: refresh()

  Timer { interval: 30000; running: true; repeat: true; onTriggered: refresh() }

  Process {
    id: batteryProc; running: false
    command: ["sh", "-c",
      "for b in /sys/class/power_supply/BAT*; do " +
      "[ -f \"$b/capacity\" ] && echo \"$(cat $b/capacity)|$(cat $b/status 2>/dev/null || echo Unknown)|$(cat $b/energy_full 2>/dev/null || echo 0)|$(cat $b/energy_now 2>/dev/null || echo 0)\"; done | head -1"]
    stdout: StdioCollector {
      onStreamFinished: {
        var parts = text.trim().split("|")
        if (parts.length >= 2) {
          root.batteryPct = parseInt(parts[0]) || 0
          root.batteryStatus = parts[1].trim()
        }
      }
    }
  }

  Process {
    id: profileProc; running: false
    command: ["sh", "-c", "powerprofilesctl get 2>/dev/null; powerprofilesctl list 2>/dev/null"]
    stdout: StdioCollector {
      onStreamFinished: {
        var lines = text.trim().split("\n").filter(function(l) { return l.trim() !== "" })
        if (lines.length > 0) {
          root.powerProfile = lines[0].trim()
          var profs = []
          for (var i = 1; i < lines.length; i++) {
            var l = lines[i].trim()
            if (l === "power-saver" || l === "balanced" || l === "performance") profs.push(l)
          }
          if (profs.length > 0) root.profiles = profs
          else root.profiles = ["power-saver", "balanced", "performance"]
        } else {
          root.profiles = []
        }
      }
    }
  }

  Process {
    id: uptimeProc; running: false
    command: ["sh", "-c", "uptime -p 2>/dev/null || uptime"]
    stdout: StdioCollector {
      onStreamFinished: { root.uptimeStr = text.trim().replace(/^up /, "") }
    }
  }

  Process {
    id: cmdProc; running: false
  }
  function exec(cmd) { cmdProc.command = ["sh", "-c", cmd]; cmdProc.running = true }

  Flickable {
    anchors.fill: parent
    contentWidth: width
    contentHeight: col.implicitHeight + 32
    clip: true

    Column {
      id: col
      width: parent.width; spacing: 0

      Item { width: 1; height: 24 }

      // ── Battery ───────────────────────────────────────────────────────────

      SectionHeader { label: "Battery" }

      OutlineCard {
        width: parent.width

        Column {
          width: parent.width; spacing: 20

          // Status header
          RowLayout {
            width: parent.width; spacing: 16

            Rectangle {
              width: 56; height: 56; radius: 16
              color: Qt.rgba(battColor.r, battColor.g, battColor.b, 0.15)

              readonly property color battColor:
                root.batteryPct > 30 ? Theme.accent :
                root.batteryPct > 10 ? Theme.yellow : Theme.red

              Text {
                anchors.centerIn: parent
                text: root.batteryStatus === "Charging" ? "󰂅"
                    : root.batteryPct > 80 ? "󰁹"
                    : root.batteryPct > 60 ? "󰂀"
                    : root.batteryPct > 40 ? "󰁾"
                    : root.batteryPct > 20 ? "󰁼"
                    : "󰁺"
                color: parent.battColor; font.pixelSize: 28
              }
            }

            Column {
              Layout.fillWidth: true; spacing: 4

              Text {
                text: root.batteryPct + "%"
                color: Theme.fg; font.pixelSize: 28; font.weight: Font.DemiBold
              }
              Text {
                text: root.batteryStatus === "Charging"   ? "Charging"
                    : root.batteryStatus === "Full"        ? "Fully charged"
                    : root.batteryStatus === "Discharging" ? "On battery"
                    : root.batteryStatus
                color: Theme.fgMuted; font.pixelSize: 12
              }
            }
          }

          // Progress bar
          Item {
            width: parent.width; height: 10

            Rectangle {
              anchors.fill: parent; radius: 5; color: Theme.bg
              Rectangle {
                width: parent.width * (root.batteryPct / 100)
                height: parent.height; radius: 5
                color: root.batteryPct > 30 ? Theme.accent
                     : root.batteryPct > 10 ? Theme.yellow : Theme.red
                Behavior on width { NumberAnimation { duration: 600; easing.type: Easing.OutCubic } }
              }
            }
          }
        }
      }

      Item { width: 1; height: 24 }

      // ── Power Profile ─────────────────────────────────────────────────────

      SectionHeader { label: "Power Profile" }

      OutlineCard {
        width: parent.width
        visible: root.profiles.length > 0

        Column {
          width: parent.width; spacing: 12

          Text {
            text: "Active: " + root.powerProfile
            color: Theme.fgMuted; font.pixelSize: 11
          }

          Row {
            width: parent.width; spacing: 8

            Repeater {
              model: root.profiles.length > 0 ? root.profiles
                : ["power-saver", "balanced", "performance"]

              Rectangle {
                width: (parent.width - 8 * (root.profiles.length - 1)) / root.profiles.length
                height: 64; radius: 12
                property bool isActive: root.powerProfile === modelData
                color: isActive ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15) : Theme.input
                border.color: isActive ? Theme.accent : Theme.border; border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }

                Column {
                  anchors.centerIn: parent; spacing: 4
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData === "power-saver"  ? "󰌪"
                        : modelData === "balanced"      ? "󰓅"
                        : "󰎓"
                    font.pixelSize: 22
                    color: isActive ? Theme.accent : Theme.fgMuted
                  }
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData === "power-saver"  ? "Saver"
                        : modelData === "balanced"      ? "Balanced"
                        : "Performance"
                    color: isActive ? Theme.fg : Theme.fgMuted; font.pixelSize: 10
                  }
                }

                HoverHandler { cursorShape: Qt.PointingHandCursor }
                TapHandler {
                  onTapped: {
                    root.exec("powerprofilesctl set " + modelData)
                    root.powerProfile = modelData
                  }
                }
              }
            }
          }
        }
      }

      OutlineCard {
        width: parent.width
        visible: root.profiles.length === 0
        height: 48
        Text {
          anchors { fill: parent; leftMargin: 16 }
          text: "power-profiles-daemon not available"
          color: Theme.fgDim; font.pixelSize: 12; verticalAlignment: Text.AlignVCenter
        }
      }

      Item { width: 1; height: 24 }

      // ── Session ────────────────────────────────────────────────────────────

      SectionHeader { label: "Session" }

      Column {
        width: parent.width; spacing: 8

        Row {
          width: parent.width; spacing: 8

          Repeater {
            model: [
              { label: "Lock",     icon: "󰌾", cmd: "hyprlock"                              },
              { label: "Suspend",  icon: "󰤄", cmd: "systemctl suspend"                     },
              { label: "Reboot",   icon: "󰑓", cmd: "systemctl reboot"                      },
              { label: "Shutdown", icon: "󰐥", cmd: "systemctl poweroff"                    },
            ]

            Rectangle {
              width: (parent.width - 8 * 3) / 4; height: 72; radius: 12
              color: Theme.card; border.color: Theme.border; border.width: 1

              Column {
                anchors.centerIn: parent; spacing: 6
                Text {
                  anchors.horizontalCenter: parent.horizontalCenter
                  text: modelData.icon; font.pixelSize: 24
                  color: modelData.label === "Shutdown" || modelData.label === "Reboot"
                    ? Theme.red : Theme.fgMuted
                }
                Text {
                  anchors.horizontalCenter: parent.horizontalCenter
                  text: modelData.label; color: Theme.fgMuted; font.pixelSize: 10
                }
              }

              HoverHandler { cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: root.exec(modelData.cmd) }
            }
          }
        }

        OutlineCard {
          width: parent.width; height: 48
          Row {
            anchors { fill: parent; leftMargin: 0 }
            spacing: 10
            Text { text: "󰔛"; color: Theme.fgMuted; font.pixelSize: 16; anchors.verticalCenter: parent.verticalCenter }
            Text {
              text: "Up " + root.uptimeStr
              color: Theme.fgMuted; font.pixelSize: 12; anchors.verticalCenter: parent.verticalCenter
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
      id: inn; anchors { fill: parent; margins: 16 }
      spacing: 0
    }
  }
}
