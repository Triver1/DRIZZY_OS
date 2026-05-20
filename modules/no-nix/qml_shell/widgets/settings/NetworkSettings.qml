import QtQuick
import QtQuick.Layouts
import Quickshell.Networking
import Quickshell.Bluetooth
import "../../theme"

Item {
  id: root

  readonly property var wifiDevice: {
    if (!Networking || !Networking.devices) return null
    var devs = Networking.devices.values
    for (var i = 0; i < devs.length; i++) {
      if (devs[i] && devs[i].networks !== undefined) return devs[i]
    }
    return null
  }
  readonly property var wifiNetworks: wifiDevice && wifiDevice.networks
    ? wifiDevice.networks.values : []
  readonly property var btAdapter: Bluetooth.defaultAdapter || null
  readonly property var btDevices: btAdapter && btAdapter.devices
    ? btAdapter.devices.values : []

  function signalIcon(s) {
    if (s >= 0.75) return "󰤨"
    if (s >= 0.5)  return "󰤥"
    if (s >= 0.25) return "󰤢"
    return "󰤟"
  }

  Flickable {
    anchors.fill: parent
    contentWidth: width
    contentHeight: content.implicitHeight + 32
    clip: true

    Column {
      id: content
      width: parent.width
      spacing: 0

      // ── WiFi ────────────────────────────────────────────────────────────────

      Item { width: 1; height: 24 }

      SettingsSectionHeader { label: "Wi-Fi" }

      SettingsCard {
        width: parent.width

        RowLayout {
          width: parent.width; spacing: 12

          Text { text: "󰖩"; color: Theme.accent; font.pixelSize: 20 }
          Column {
            Layout.fillWidth: true
            Text { text: "Wi-Fi"; color: Theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold }
            Text {
              text: Networking.wifiEnabled
                ? (root.wifiDevice ? "Connected" : "Enabled")
                : "Disabled"
              color: Theme.fgMuted; font.pixelSize: 11
            }
          }
          SettingsToggle {
            active: Networking.wifiEnabled
            onToggled: Networking.wifiEnabled = !Networking.wifiEnabled
          }
        }
      }

      Item { width: 1; height: 8 }

      // Network list
      Column {
        width: parent.width
        spacing: 2
        visible: Networking.wifiEnabled

        Repeater {
          model: root.wifiNetworks

          Item {
            id: netRow
            width: parent.width
            height: netExpanded ? 48 + 4 + 38 : 48
            clip: true
            Behavior on height { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }

            property bool netExpanded: false
            property var net: modelData || null

            Rectangle {
              width: parent.width; height: 48; radius: 12
              color: netHov.hovered ? Theme.raised : Theme.card
              border.color: netRow.net && netRow.net.connected ? Theme.accent : Theme.border
              border.width: 1
              Behavior on color { ColorAnimation { duration: 100 } }

              RowLayout {
                anchors { fill: parent; leftMargin: 16; rightMargin: 14 }
                spacing: 12

                Text {
                  text: netRow.net ? root.signalIcon(netRow.net.signalStrength || 0) : "󰤟"
                  color: netRow.net && netRow.net.connected ? Theme.accent : Theme.fgMuted
                  font.pixelSize: 16
                }
                Column {
                  Layout.fillWidth: true
                  Text {
                    text: netRow.net ? (netRow.net.name || "Unknown") : "..."
                    color: Theme.fg; font.pixelSize: 12
                    font.weight: netRow.net && netRow.net.connected ? Font.DemiBold : Font.Normal
                  }
                  Text {
                    text: netRow.net && netRow.net.connected ? "Connected"
                        : netRow.net && netRow.net.known ? "Saved" : "Available"
                    color: netRow.net && netRow.net.connected ? Theme.accent : Theme.fgDim
                    font.pixelSize: 10
                  }
                }
                Text {
                  visible: netRow.net && netRow.net.security !== undefined
                  text: "󰌾"; color: Theme.fgDim; font.pixelSize: 13
                }
                Text {
                  text: netRow.netExpanded ? "▴" : "▾"
                  color: Theme.fgDim; font.pixelSize: 9
                }
              }

              HoverHandler { id: netHov; cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: netRow.netExpanded = !netRow.netExpanded }
            }

            Row {
              y: 48 + 4; x: 8; spacing: 6; height: 34
              width: parent.width - 16

              Repeater {
                id: netActRep
                model: netRow.net
                  ? (netRow.net.connected ? ["Disconnect"] : ["Connect"])
                  : []

                Rectangle {
                  width: (netActRep.count > 0 ? (parent.width - 6 * (netActRep.count - 1)) / netActRep.count : 0)
                  height: 32; radius: 9
                  color: Theme.card
                  border.color: modelData === "Disconnect" ? Theme.red : Theme.accent; border.width: 1

                  Text {
                    anchors.centerIn: parent; text: modelData
                    color: modelData === "Disconnect" ? Theme.red : Theme.accent
                    font.pixelSize: 11; font.weight: Font.DemiBold
                  }
                  HoverHandler { cursorShape: Qt.PointingHandCursor }
                  TapHandler {
                    onTapped: {
                      if (!netRow.net) return
                      if (modelData === "Connect")    netRow.net.connect()
                      if (modelData === "Disconnect") netRow.net.disconnect()
                      netRow.netExpanded = false
                    }
                  }
                }
              }
            }
          }
        }

        // Scan button
        Rectangle {
          width: parent.width; height: 40; radius: 12
          color: Theme.card; border.color: Theme.border; border.width: 1
          visible: Networking.wifiEnabled

          RowLayout {
            anchors { fill: parent; leftMargin: 16; rightMargin: 14 }
            Text { text: "󰑓"; color: Theme.fgMuted; font.pixelSize: 16 }
            Text {
              Layout.fillWidth: true
              text: root.wifiDevice && root.wifiDevice.scannerEnabled
                ? "Scanning for networks..." : "Scan for Networks"
              color: Theme.fgMuted; font.pixelSize: 12
            }
          }
          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler { onTapped: { if (root.wifiDevice) root.wifiDevice.scannerEnabled = true } }
        }
      }

      Item { width: 1; height: 24 }

      // ── Bluetooth ────────────────────────────────────────────────────────────

      SettingsSectionHeader { label: "Bluetooth" }

      SettingsCard {
        width: parent.width

        RowLayout {
          width: parent.width; spacing: 12

          Text { text: "󰂯"; color: Theme.blue; font.pixelSize: 20 }
          Column {
            Layout.fillWidth: true
            Text { text: "Bluetooth"; color: Theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold }
            Text {
              text: root.btAdapter && root.btAdapter.enabled ? "Enabled" : "Disabled"
              color: Theme.fgMuted; font.pixelSize: 11
            }
          }
          SettingsToggle {
            active: root.btAdapter ? root.btAdapter.enabled : false
            onToggled: { if (root.btAdapter) root.btAdapter.enabled = !root.btAdapter.enabled }
          }
        }
      }

      Item { width: 1; height: 8 }

      Column {
        width: parent.width
        spacing: 2
        visible: root.btAdapter && root.btAdapter.enabled

        Repeater {
          model: root.btDevices

          Item {
            id: btRow
            width: parent.width
            height: btExpanded ? 56 + 4 + 38 : 56
            clip: true
            Behavior on height { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }

            property bool btExpanded: false
            property var dev: modelData || null

            Rectangle {
              width: parent.width; height: 56; radius: 12
              color: btHov.hovered ? Theme.raised : Theme.card
              border.color: btRow.dev && btRow.dev.connected ? Theme.blue : Theme.border
              border.width: 1
              Behavior on color { ColorAnimation { duration: 100 } }

              RowLayout {
                anchors { fill: parent; leftMargin: 16; rightMargin: 14 }
                spacing: 12

                Text {
                  text: btRow.dev && btRow.dev.icon ? "󰋋" : "󰂯"
                  color: btRow.dev && btRow.dev.connected ? Theme.blue : Theme.fgMuted
                  font.pixelSize: 20
                }
                Column {
                  Layout.fillWidth: true
                  Text {
                    text: btRow.dev ? (btRow.dev.name || "Unknown Device") : "..."
                    color: Theme.fg; font.pixelSize: 12
                    font.weight: btRow.dev && btRow.dev.connected ? Font.DemiBold : Font.Normal
                  }
                  Text {
                    text: {
                      if (!btRow.dev) return ""
                      if (btRow.dev.pairing)  return "Pairing..."
                      if (btRow.dev.connected) return "Connected"
                      if (btRow.dev.paired)    return "Paired"
                      return "Available"
                    }
                    color: btRow.dev && btRow.dev.connected ? Theme.blue : Theme.fgDim
                    font.pixelSize: 10
                  }
                }
                Text {
                  text: btRow.btExpanded ? "▴" : "▾"
                  color: Theme.fgDim; font.pixelSize: 9
                }
              }

              HoverHandler { id: btHov; cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: btRow.btExpanded = !btRow.btExpanded }
            }

            Row {
              y: 56 + 4; x: 8; spacing: 6; height: 34
              width: parent.width - 16

              Repeater {
                id: btActRep
                model: btRow.dev
                  ? (btRow.dev.connected ? ["Disconnect"]
                     : btRow.dev.paired  ? ["Connect"]
                     : ["Pair"])
                  : []

                Rectangle {
                  width: (btActRep.count > 0 ? (parent.width - 6*(btActRep.count-1)) / btActRep.count : 0)
                  height: 32; radius: 9
                  color: Theme.card
                  border.color: modelData === "Disconnect" ? Theme.red : Theme.accent; border.width: 1

                  Text {
                    anchors.centerIn: parent; text: modelData
                    color: modelData === "Disconnect" ? Theme.red : Theme.accent
                    font.pixelSize: 11; font.weight: Font.DemiBold
                  }
                  HoverHandler { cursorShape: Qt.PointingHandCursor }
                  TapHandler {
                    onTapped: {
                      if (!btRow.dev) return
                      if (modelData === "Connect")    btRow.dev.connect()
                      if (modelData === "Disconnect") btRow.dev.disconnect()
                      if (modelData === "Pair")       btRow.dev.pair()
                      btRow.btExpanded = false
                    }
                  }
                }
              }
            }
          }
        }

        Rectangle {
          width: parent.width; height: 40; radius: 12
          color: root.btAdapter && root.btAdapter.discovering
            ? Qt.rgba(Theme.blue.r, Theme.blue.g, Theme.blue.b, 0.12) : Theme.card
          border.color: root.btAdapter && root.btAdapter.discovering ? Theme.blue : Theme.border
          border.width: 1

          RowLayout {
            anchors { fill: parent; leftMargin: 16; rightMargin: 14 }
            Text { text: "󰂰"; color: Theme.fgMuted; font.pixelSize: 16 }
            Text {
              Layout.fillWidth: true
              text: root.btAdapter && root.btAdapter.discovering
                ? "Scanning..." : "Scan for Devices"
              color: Theme.fgMuted; font.pixelSize: 12
            }
          }
          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler {
            onTapped: { if (root.btAdapter) root.btAdapter.discovering = !root.btAdapter.discovering }
          }
        }
      }

      Item { width: 1; height: 24 }
    }
  }

  // ── Inline helper components ───────────────────────────────────────────────

  component SettingsSectionHeader: Item {
    property string label: ""
    width: parent.width; height: 32

    Text {
      x: 16; anchors.verticalCenter: parent.verticalCenter
      text: label.toUpperCase()
      color: Theme.fgDim; font.pixelSize: 10; font.weight: Font.Bold; font.letterSpacing: 1.5
    }
  }

  component SettingsCard: Rectangle {
    default property alias content: innerCol.data
    width: parent.width
    height: innerCol.implicitHeight + 32
    radius: 14
    color: Theme.card; border.color: Theme.border; border.width: 1

    Column {
      id: innerCol
      anchors { fill: parent; margins: 16 }
      spacing: 0
    }
  }

  component SettingsToggle: Rectangle {
    property bool active: false
    signal toggled()

    width: 44; height: 26; radius: 13
    color: active ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.25) : Theme.input
    border.color: active ? Theme.accent : Theme.border; border.width: 1
    Behavior on color { ColorAnimation { duration: 180 } }

    Rectangle {
      width: 20; height: 20; radius: 10; anchors.verticalCenter: parent.verticalCenter
      x: active ? parent.width - width - 3 : 3
      color: active ? Theme.accent : Theme.fgDim
      Behavior on x { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
      Behavior on color { ColorAnimation { duration: 180 } }
    }

    HoverHandler { cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: parent.toggled() }
  }
}
