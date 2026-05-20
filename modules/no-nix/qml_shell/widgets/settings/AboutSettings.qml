import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import "../../theme"

Item {
  id: root

  property string hostname:    "..."
  property string kernel:      "..."
  property string nixosVer:    "..."
  property string niriVer:     "..."
  property string desktopEnv:  "Niri"
  property string shell:       "Zsh"
  property string uptimeStr:   "..."
  property string cpuModel:    "..."
  property string gpuModel:    "..."
  property int    ramGB:       0

  function refresh() {
    infoProc.running = true
  }

  onVisibleChanged: if (visible) refresh()
  Component.onCompleted: refresh()

  Process {
    id: infoProc; running: false
    command: ["sh", "-c", [
      "echo HOSTNAME=$(hostname)",
      "echo KERNEL=$(uname -r)",
      "echo NIXOS=$(nixos-version 2>/dev/null || echo N/A)",
      "echo NIRI=$(niri --version 2>/dev/null || echo N/A)",
      "echo UPTIME=$(uptime -p 2>/dev/null || uptime)",
      "echo CPU=$(grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2 | sed 's/^ //')",
      "echo RAM=$(grep MemTotal /proc/meminfo | awk '{printf \"%.0f\", $2/1024/1024}')",
      "echo GPU=$(lspci 2>/dev/null | grep -i 'vga\\|3d\\|display' | head -1 | sed 's/.*: //' | cut -c1-60 || echo N/A)"
    ].join("; ")]
    stdout: StdioCollector {
      onStreamFinished: {
        var lines = text.trim().split("\n")
        for (var i = 0; i < lines.length; i++) {
          var eq = lines[i].indexOf("=")
          if (eq < 0) continue
          var key = lines[i].substring(0, eq)
          var val = lines[i].substring(eq + 1).trim()
          if (key === "HOSTNAME") root.hostname = val
          else if (key === "KERNEL")   root.kernel   = val
          else if (key === "NIXOS")    root.nixosVer = val
          else if (key === "NIRI")     root.niriVer  = val
          else if (key === "UPTIME")   root.uptimeStr = val.replace(/^up /, "")
          else if (key === "CPU")      root.cpuModel = val
          else if (key === "RAM")      root.ramGB = parseInt(val) || 0
          else if (key === "GPU")      root.gpuModel = val
        }
      }
    }
  }

  Flickable {
    anchors.fill: parent
    contentWidth: width
    contentHeight: col.implicitHeight + 32
    clip: true

    Column {
      id: col
      width: parent.width; spacing: 0

      Item { width: 1; height: 24 }

      // Avatar / distro card
      OutlineCard {
        width: parent.width

        RowLayout {
          width: parent.width; spacing: 20

          Rectangle {
            width: 72; height: 72; radius: 20
            color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12)
            border.color: Theme.accent; border.width: 1

            Text {
              anchors.centerIn: parent; text: "󱄅"
              color: Theme.accent; font.pixelSize: 38
            }
          }

          Column {
            Layout.fillWidth: true; spacing: 4

            Text {
              text: root.hostname
              color: Theme.fg; font.pixelSize: 20; font.weight: Font.DemiBold
            }
            Text {
              text: "NixOS  ·  " + root.desktopEnv
              color: Theme.fgMuted; font.pixelSize: 12
            }
            Text {
              text: root.nixosVer
              color: Theme.fgDim; font.pixelSize: 11
            }
          }
        }
      }

      Item { width: 1; height: 24 }

      SectionHeader { label: "Hardware" }

      OutlineCard {
        width: parent.width

        Column {
          width: parent.width; spacing: 0

          InfoRow { label: "CPU";    value: root.cpuModel; iconStr: "󰻠" }
          Divider {}
          InfoRow { label: "RAM";    value: root.ramGB + " GB";           iconStr: "󰍛" }
          Divider {}
          InfoRow { label: "GPU";    value: root.gpuModel;                iconStr: "󰦚" }
        }
      }

      Item { width: 1; height: 24 }

      SectionHeader { label: "Software" }

      OutlineCard {
        width: parent.width

        Column {
          width: parent.width; spacing: 0

          InfoRow { label: "Kernel";  value: root.kernel;    iconStr: "󰌽" }
          Divider {}
          InfoRow { label: "NixOS";   value: root.nixosVer;  iconStr: "󱄅" }
          Divider {}
          InfoRow { label: "Niri";    value: root.niriVer;   iconStr: "󰖭" }
          Divider {}
          InfoRow { label: "Shell";   value: root.shell;     iconStr: "" }
          Divider {}
          InfoRow { label: "Uptime";  value: root.uptimeStr; iconStr: "󰔛" }
        }
      }

      Item { width: 1; height: 24 }

      SectionHeader { label: "About This Shell" }

      OutlineCard {
        width: parent.width
        height: innerAbout.implicitHeight + 32

        Column {
          id: innerAbout
          anchors { fill: parent; margins: 16 }
          spacing: 12

          Text {
            text: "Quickshell · QML Wayland Shell"
            color: Theme.fg; font.pixelSize: 14; font.weight: Font.DemiBold
          }
          Text {
            width: parent.width; wrapMode: Text.Wrap
            text: "Ponokai Shusia theme. Built with Quickshell 0.3.0 and Qt6 Quick. " +
                  "Config lives at ~/NEWFLAKE/modules/no-nix/qml_shell/."
            color: Theme.fgMuted; font.pixelSize: 11; lineHeight: 1.5
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
    radius: 14; color: Theme.card; border.color: Theme.border; border.width: 1
    Column { id: inn; anchors { fill: parent; margins: 16 }
      spacing: 0 }
  }

  component Divider: Rectangle {
    width: parent.width; height: 1; color: Theme.border
  }

  component InfoRow: RowLayout {
    property string label:   ""
    property string value:   ""
    property string iconStr: ""
    width: parent.width; height: 44

    Text { text: iconStr; color: Theme.fgMuted; font.pixelSize: 16; Layout.preferredWidth: 24 }
    Text { text: label;  color: Theme.fgMuted; font.pixelSize: 12; Layout.preferredWidth: 80 }
    Text {
      text: value; color: Theme.fg; font.pixelSize: 12
      Layout.fillWidth: true; elide: Text.ElideRight
    }
  }
}
