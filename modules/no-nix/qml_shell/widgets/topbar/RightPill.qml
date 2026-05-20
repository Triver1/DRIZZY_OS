import QtQuick
import "../../theme"

Item {
  id: root
  property var shell
  property var notifOverlay

  width: pill.width
  height: 24

  function batteryIcon(pct, charging) {
    if (charging) return "󰂅"
    if (pct >= 95) return "󰂃"
    if (pct >= 85) return "󰂂"
    if (pct >= 75) return "󰂁"
    if (pct >= 65) return "󰂀"
    if (pct >= 55) return "󰁿"
    if (pct >= 45) return "󰁾"
    if (pct >= 35) return "󰁽"
    if (pct >= 25) return "󰁼"
    if (pct >= 15) return "󰁻"
    if (pct >= 5)  return "󰁺"
    return "󰁹"
  }

  function batteryColor(pct) {
    if (pct > 50) return Theme.green
    if (pct > 20) return Theme.yellow
    return Theme.red
  }

  Rectangle {
    id: pill
    height: 24; width: pillRow.implicitWidth + 20; radius: 12
    color: "#e0302d31"
    border.color: Theme.accent; border.width: 1

    Row {
      id: pillRow
      anchors.centerIn: parent; spacing: 10

      Text {
        text: "󰕾"; font.pixelSize: 13; color: Theme.purple
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        text: "󰖩"; font.pixelSize: 13; color: Theme.green
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        text: {
          if (!root.shell || !root.shell.batteryAvailable) return "󰁹"
          return root.batteryIcon(root.shell.batteryPercent, root.shell.batteryStatus === "Charging")
        }
        font.pixelSize: 13
        color: {
          if (!root.shell || !root.shell.batteryAvailable) return Theme.green
          return root.batteryColor(root.shell.batteryPercent)
        }
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        visible: root.shell && root.shell.batteryAvailable
        text: root.shell ? (Math.round(root.shell.batteryPercent) + "%") : ""
        font.pixelSize: 10
        color: root.shell && root.shell.batteryAvailable
          ? root.batteryColor(root.shell.batteryPercent)
          : Theme.green
        anchors.verticalCenter: parent.verticalCenter
      }
    }
  }
}
