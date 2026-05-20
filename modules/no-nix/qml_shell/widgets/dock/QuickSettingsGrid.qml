import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import Quickshell.Services.Pipewire
import Quickshell.Networking
import Quickshell.Bluetooth
import "../../theme"

Item {
  id: root
  property var shell

  property bool nightOn:    false
  property bool caffeineOn: false
  property bool muteOn:     false
  property bool dndOn:      false

  property real volumeLevel:     shell ? shell.volumeLevel : 0.7
  property real brightnessLevel: 0.6

  property string expandedKey: ""

  // ─── Native networking bindings ────────────────────────────────────────────

  readonly property bool wifiEnabled: Networking.wifiEnabled
  readonly property var wifiDevice: {
    if (!Networking || !Networking.devices) return null
    var devs = Networking.devices.values
    for (var i = 0; i < devs.length; i++) {
      var d = devs[i]
      if (d && d.networks !== undefined) return d
    }
    return null
  }
  readonly property var wifiNetworks: wifiDevice && wifiDevice.networks
    ? wifiDevice.networks.values : []

  // ─── Native Bluetooth bindings ─────────────────────────────────────────────

  readonly property var btAdapter:  Bluetooth.defaultAdapter || null
  readonly property bool btEnabled: btAdapter ? btAdapter.enabled : false
  readonly property var btDevices:  btAdapter && btAdapter.devices
    ? btAdapter.devices.values : []

  // ─── Shared utilities ──────────────────────────────────────────────────────

  PwObjectTracker { objects: [ Pipewire.defaultAudioSink, Pipewire.defaultAudioSource ] }
  Process { id: cmdRunner; running: false }
  function exec(cmd) { cmdRunner.command = ["sh", "-c", cmd]; cmdRunner.running = true }

  function toggleNight()    { if (nightOn) exec("pkill -f gammastep 2>/dev/null; pkill wlsunset 2>/dev/null"); else exec("command -v gammastep >/dev/null && gammastep -l 0:0 -O 4000 &"); nightOn = !nightOn }
  function toggleCaffeine() { if (caffeineOn) exec("pkill -f 'systemd-inhibit.*idle' 2>/dev/null"); else exec("systemd-inhibit --what=idle --why='Caffeine' sleep 86400 &"); caffeineOn = !caffeineOn }
  function toggleMute()     { var s = Pipewire.defaultAudioSink; if (s && s.audio) { s.audio.muted = !s.audio.muted; muteOn = s.audio.muted } }
  function toggleDnd()      { dndOn = !dndOn }
  function setNightTemp(k)  { exec("pkill -f gammastep 2>/dev/null; command -v gammastep >/dev/null && gammastep -l 0:0 -O " + k + " &"); nightOn = true }

  function wifiSignalIcon(strength) {
    if (strength >= 0.75) return "󰤨"
    if (strength >= 0.5)  return "󰤥"
    if (strength >= 0.25) return "󰤢"
    return "󰤟"
  }

  function btDeviceIcon(dev) {
    if (!dev || !dev.icon) return "󰂯"
    var ic = dev.icon.toLowerCase()
    if (ic.indexOf("phone")    >= 0) return "󰄜"
    if (ic.indexOf("headset")  >= 0 || ic.indexOf("audio") >= 0) return "󰋋"
    if (ic.indexOf("keyboard") >= 0) return "󰌌"
    if (ic.indexOf("mouse")    >= 0) return "󰍽"
    if (ic.indexOf("computer") >= 0) return "󰇄"
    return "󰂯"
  }

  // ─── Grid view ─────────────────────────────────────────────────────────────

  Item {
    id: gridView
    anchors.fill: parent
    opacity: expandedKey === "" ? 1.0 : 0.0
    scale:   expandedKey === "" ? 1.0 : 0.94
    enabled: expandedKey === ""
    Behavior on opacity { NumberAnimation { duration: 180 } }
    Behavior on scale   { NumberAnimation { duration: 220 } }

    Column {
      anchors { fill: parent; topMargin: 4; bottomMargin: 4 }
      spacing: 10

      Text {
        text: "Quick Settings"
        color: Theme.fgMuted
        font.pixelSize: 10; font.weight: Font.Bold; font.letterSpacing: 1.0
      }

      Grid {
        id: toggleGrid
        columns: 3; spacing: 8; width: parent.width

        ExpandTile {
          tileLabel: "Wi-Fi"; tileIcon: "󰖩"; tileColor: "#9ecd6f"
          tileActive: root.wifiEnabled
          tileSub: root.wifiEnabled ? "Enabled" : "Off"
          width: (toggleGrid.width - 16) / 3; height: 84
          onExpand: root.expandedKey = "wifi"
        }
        ExpandTile {
          tileLabel: "Bluetooth"; tileIcon: "󰂯"; tileColor: "#7accd7"
          tileActive: root.btEnabled
          tileSub: root.btEnabled ? "On" : "Off"
          width: (toggleGrid.width - 16) / 3; height: 84
          onExpand: root.expandedKey = "bluetooth"
        }
        ExpandTile {
          tileLabel: "Night Light"; tileIcon: "󰖔"; tileColor: "#e5c463"
          tileActive: root.nightOn
          tileSub: root.nightOn ? "4000K" : "Off"
          width: (toggleGrid.width - 16) / 3; height: 84
          onExpand: root.expandedKey = "night"
        }
        ExpandTile {
          tileLabel: "Caffeine"; tileIcon: "󰅶"; tileColor: "#ef9062"
          tileActive: root.caffeineOn
          tileSub: root.caffeineOn ? "Active" : "Off"
          width: (toggleGrid.width - 16) / 3; height: 84
          onExpand: root.expandedKey = "caffeine"
        }
        SimpleTile {
          tileLabel: "Mute"; tileIcon: "󰝟"; tileColor: "#ab9df2"
          tileActive: root.muteOn
          width: (toggleGrid.width - 16) / 3; height: 84
          onToggle: root.toggleMute()
        }
        SimpleTile {
          tileLabel: "DND"; tileIcon: "󰂛"; tileColor: "#f85e84"
          tileActive: root.dndOn
          width: (toggleGrid.width - 16) / 3; height: 84
          onToggle: root.toggleDnd()
        }
      }

      SliderRow {
        width: parent.width
        rowIcon: "󰕾"; rowLabel: "Volume"; rowColor: "#ab9df2"
        sliderValue: root.volumeLevel
        onSlid: function(v) {
          root.volumeLevel = v
          if (shell) shell.volumeLevel = v
          root.exec("wpctl set-volume @DEFAULT_AUDIO_SINK@ " + Math.round(v * 100) + "%")
          if (shell) shell.triggerStatusPopup("󰕾", "Volume", Math.round(v * 100) + "%")
        }
      }

      SliderRow {
        width: parent.width
        rowIcon: "󰃠"; rowLabel: "Brightness"; rowColor: "#e5c463"
        sliderValue: root.brightnessLevel
        onSlid: function(v) {
          root.brightnessLevel = v
          root.exec("brightnessctl set " + Math.round(v * 100) + "%")
          if (shell) shell.triggerStatusPopup("󰃠", "Brightness", Math.round(v * 100) + "%")
        }
      }
    }
  }

  // ─── Expand detail view ────────────────────────────────────────────────────

  Item {
    id: detailView
    anchors.fill: parent
    opacity: expandedKey !== "" ? 1.0 : 0.0
    scale:   expandedKey !== "" ? 1.0 : 0.96
    enabled: expandedKey !== ""
    Behavior on opacity { NumberAnimation { duration: 200 } }
    Behavior on scale   { NumberAnimation { duration: 240; easing.type: Easing.OutBack } }

    Column {
      anchors { fill: parent; topMargin: 4; bottomMargin: 4 }
      spacing: 8

      // Back header
      Row {
        spacing: 8

        Rectangle {
          width: 30; height: 30; radius: 10
          color: Theme.card; border.color: Theme.border; border.width: 1
          Text { anchors.centerIn: parent; text: "‹"; color: Theme.fgMuted; font.pixelSize: 18 }
          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler { onTapped: root.expandedKey = "" }
        }

        Text {
          anchors.verticalCenter: parent.verticalCenter
          text: expandedKey === "wifi" ? "Wi-Fi"
              : expandedKey === "bluetooth" ? "Bluetooth"
              : expandedKey === "night" ? "Night Light"
              : "Caffeine"
          color: Theme.fg; font.pixelSize: 14; font.weight: Font.DemiBold
        }
      }

      // ── WiFi detail ───────────────────────────────────────────────────────

      Column {
        visible: expandedKey === "wifi"
        width: parent.width; spacing: 6

        ToggleRow {
          width: parent.width
          rowIcon: "󰖩"; rowLabel: "Wi-Fi"; rowColor: "#9ecd6f"
          rowActive: root.wifiEnabled
          rowSub: root.wifiEnabled ? "Enabled" : "Disabled"
          onToggle: { Networking.wifiEnabled = !Networking.wifiEnabled }
        }

        Text {
          visible: !root.wifiEnabled
          text: "Wi-Fi is disabled"
          color: Theme.fgDim; font.pixelSize: 10
          anchors.horizontalCenter: parent.horizontalCenter
        }

        // Network list
        Column {
          visible: root.wifiEnabled
          width: parent.width
          spacing: 4

          Repeater {
            model: root.wifiNetworks.slice(0, 5)

            Item {
              id: netItem
              width: parent.width
              height: netMenuOpen ? 42 + 4 + 36 : 42
              clip: true

              Behavior on height { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }

              property var network: modelData || null
              property bool netMenuOpen: false

              Rectangle {
                width: parent.width; height: 42; radius: 12
                color: netHover.hovered ? Theme.raised : Theme.card
                border.color: netItem.network && netItem.network.connected ? "#9ecd6f" : Theme.border
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }

                Row {
                  anchors { fill: parent; leftMargin: 12; rightMargin: 10 }
                  spacing: 8

                  Text {
                    text: netItem.network ? root.wifiSignalIcon(netItem.network.signalStrength || 0) : "󰤟"
                    color: netItem.network && netItem.network.connected ? "#9ecd6f" : Theme.fgMuted
                    font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter
                  }

                  Text {
                    text: netItem.network ? (netItem.network.name || "Unknown") : "..."
                    color: netItem.network && netItem.network.connected ? Theme.fg : Theme.fgMuted
                    font.pixelSize: 11
                    font.weight: netItem.network && netItem.network.connected ? Font.DemiBold : Font.Normal
                    elide: Text.ElideRight
                    width: parent.width - 68
                    anchors.verticalCenter: parent.verticalCenter
                  }

                  Text {
                    text: netItem.netMenuOpen ? "▴" : "▾"
                    color: Theme.fgDim; font.pixelSize: 9
                    anchors.verticalCenter: parent.verticalCenter
                  }
                }

                HoverHandler { id: netHover; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: netItem.netMenuOpen = !netItem.netMenuOpen }
              }

              // Action buttons
              Row {
                id: netActions
                y: 42 + 4
                width: parent.width; height: 36; spacing: 4

                Repeater {
                  id: netActRepeater
                  model: {
                    if (!netItem.network) return []
                    if (netItem.network.connected) return ["Disconnect"]
                    return netItem.network.known ? ["Connect", "Forget"] : ["Connect"]
                  }

                  Rectangle {
                    width: (netActions.width - 4 * (netActRepeater.count - 1)) / netActRepeater.count
                    height: 34; radius: 10
                    color: Theme.card
                    border.color: modelData === "Disconnect" || modelData === "Forget"
                      ? Theme.red : Theme.accent
                    border.width: 1

                    Text {
                      anchors.centerIn: parent
                      text: modelData
                      color: modelData === "Disconnect" || modelData === "Forget"
                        ? Theme.red : Theme.accent
                      font.pixelSize: 10; font.weight: Font.DemiBold
                    }

                    HoverHandler { cursorShape: Qt.PointingHandCursor }
                    TapHandler {
                      onTapped: {
                        if (!netItem.network) return
                        if (modelData === "Connect")    netItem.network.connect()
                        if (modelData === "Disconnect") netItem.network.disconnect()
                        netItem.netMenuOpen = false
                      }
                    }
                  }
                }
              }
            }
          }
        }

        // Scan button
        Rectangle {
          visible: root.wifiEnabled
          width: parent.width; height: 36; radius: 10
          color: Theme.card; border.color: Theme.border; border.width: 1

          Row {
            anchors.centerIn: parent; spacing: 8
            Text { text: "󰑓"; color: Theme.fgMuted; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter }
            Text { text: root.wifiDevice && root.wifiDevice.scannerEnabled ? "Scanning..." : "Scan for Networks"; color: Theme.fgMuted; font.pixelSize: 11 }
          }

          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler {
            onTapped: {
              if (root.wifiDevice) root.wifiDevice.scannerEnabled = true
            }
          }
        }
      }

      // ── Bluetooth detail ──────────────────────────────────────────────────

      Column {
        visible: expandedKey === "bluetooth"
        width: parent.width; spacing: 6

        ToggleRow {
          width: parent.width
          rowIcon: "󰂯"; rowLabel: "Bluetooth"; rowColor: "#7accd7"
          rowActive: root.btEnabled
          rowSub: root.btEnabled ? "Enabled" : "Disabled"
          onToggle: { if (root.btAdapter) root.btAdapter.enabled = !root.btAdapter.enabled }
        }

        Text {
          visible: !root.btEnabled
          text: "Bluetooth is disabled"
          color: Theme.fgDim; font.pixelSize: 10
          anchors.horizontalCenter: parent.horizontalCenter
        }

        // Device list
        Column {
          visible: root.btEnabled
          width: parent.width
          spacing: 4

          Repeater {
            model: root.btDevices.slice(0, 5)

            Item {
              id: btItem
              width: parent.width
              height: btMenuOpen ? 42 + 4 + 36 : 42
              clip: true

              Behavior on height { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }

              property var device: modelData || null
              property bool btMenuOpen: false

              Rectangle {
                width: parent.width; height: 42; radius: 12
                color: btHover.hovered ? Theme.raised : Theme.card
                border.color: btItem.device && btItem.device.connected ? "#7accd7" : Theme.border
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }

                Row {
                  anchors { fill: parent; leftMargin: 12; rightMargin: 10 }
                  spacing: 8

                  Text {
                    text: root.btDeviceIcon(btItem.device)
                    color: btItem.device && btItem.device.connected ? "#7accd7" : Theme.fgMuted
                    font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter
                  }

                  Column {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width - 68

                    Text {
                      text: btItem.device ? (btItem.device.name || "Unknown") : "..."
                      color: btItem.device && btItem.device.connected ? Theme.fg : Theme.fgMuted
                      font.pixelSize: 11
                      font.weight: btItem.device && btItem.device.connected ? Font.DemiBold : Font.Normal
                      elide: Text.ElideRight; width: parent.width
                    }
                    Text {
                      text: {
                        if (!btItem.device) return ""
                        if (btItem.device.pairing)  return "Pairing..."
                        if (btItem.device.connected) return "Connected"
                        if (btItem.device.paired)    return "Paired"
                        return "Available"
                      }
                      color: btItem.device && btItem.device.connected ? "#7accd7" : Theme.fgDim
                      font.pixelSize: 9
                    }
                  }

                  Text {
                    text: btItem.btMenuOpen ? "▴" : "▾"
                    color: Theme.fgDim; font.pixelSize: 9
                    anchors.verticalCenter: parent.verticalCenter
                  }
                }

                HoverHandler { id: btHover; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: btItem.btMenuOpen = !btItem.btMenuOpen }
              }

              // Action buttons
              Row {
                id: btActions
                y: 42 + 4
                width: parent.width; height: 36; spacing: 4

                Repeater {
                  id: btActRepeater
                  model: {
                    if (!btItem.device) return []
                    if (btItem.device.connected)      return ["Disconnect"]
                    if (btItem.device.paired)         return ["Connect"]
                    return ["Pair"]
                  }

                  Rectangle {
                    width: (btActions.width - 4 * (btActRepeater.count - 1)) / btActRepeater.count
                    height: 34; radius: 10
                    color: Theme.card
                    border.color: modelData === "Disconnect" ? Theme.red : Theme.accent
                    border.width: 1

                    Text {
                      anchors.centerIn: parent
                      text: modelData
                      color: modelData === "Disconnect" ? Theme.red : Theme.accent
                      font.pixelSize: 10; font.weight: Font.DemiBold
                    }

                    HoverHandler { cursorShape: Qt.PointingHandCursor }
                    TapHandler {
                      onTapped: {
                        if (!btItem.device) return
                        if (modelData === "Connect")    btItem.device.connect()
                        if (modelData === "Disconnect") btItem.device.disconnect()
                        if (modelData === "Pair")       btItem.device.pair()
                        btItem.btMenuOpen = false
                      }
                    }
                  }
                }
              }
            }
          }
        }

        // Scan toggle
        Rectangle {
          visible: root.btEnabled
          width: parent.width; height: 36; radius: 10
          color: root.btAdapter && root.btAdapter.discovering
            ? Qt.rgba(0.48, 0.80, 0.84, 0.15)
            : Theme.card
          border.color: root.btAdapter && root.btAdapter.discovering
            ? "#7accd7" : Theme.border
          border.width: 1

          Row {
            anchors.centerIn: parent; spacing: 8
            Text { text: "󰂰"; color: Theme.fgMuted; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter }
            Text {
              text: root.btAdapter && root.btAdapter.discovering ? "Scanning..." : "Scan for Devices"
              color: Theme.fgMuted; font.pixelSize: 11
            }
          }

          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler {
            onTapped: {
              if (root.btAdapter) root.btAdapter.discovering = !root.btAdapter.discovering
            }
          }
        }
      }

      // ── Night Light detail ────────────────────────────────────────────────

      Column {
        visible: expandedKey === "night"
        width: parent.width; spacing: 6

        ToggleRow {
          width: parent.width
          rowIcon: "󰖔"; rowLabel: "Night Light"; rowColor: "#e5c463"
          rowActive: root.nightOn
          rowSub: root.nightOn ? "Active" : "Off"
          onToggle: root.toggleNight()
        }

        Grid {
          columns: 2; spacing: 6; width: parent.width

          Repeater {
            model: [
              { label: "Warm  2500K", k: 2500 },
              { label: "Cozy  3500K", k: 3500 },
              { label: "Std   4000K", k: 4000 },
              { label: "Cool  5000K", k: 5000 }
            ]

            Rectangle {
              width: (parent.width - 6) / 2; height: 36; radius: 10
              color: Theme.card; border.color: Theme.border; border.width: 1

              Text {
                anchors.centerIn: parent
                text: modelData.label
                color: Theme.fgMuted; font.pixelSize: 10; font.weight: Font.Medium
              }

              HoverHandler { cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: root.setNightTemp(modelData.k) }
            }
          }
        }
      }

      // ── Caffeine detail ───────────────────────────────────────────────────

      Column {
        visible: expandedKey === "caffeine"
        width: parent.width; spacing: 6

        ToggleRow {
          width: parent.width
          rowIcon: "󰅶"; rowLabel: "Caffeine"; rowColor: "#ef9062"
          rowActive: root.caffeineOn
          rowSub: root.caffeineOn ? "Screen stays on" : "Normal sleep"
          onToggle: root.toggleCaffeine()
        }

        Rectangle {
          width: parent.width; height: 52; radius: 14
          color: Theme.card; border.color: Theme.border; border.width: 1
          Text {
            anchors { fill: parent; margins: 12 }
            text: "Prevents automatic screen lock and sleep while active."
            color: Theme.fgDim; font.pixelSize: 10; wrapMode: Text.Wrap
          }
        }
      }
    }
  }

  // ─── Reusable components ───────────────────────────────────────────────────

  component ExpandTile: Rectangle {
    id: etile
    property string tileLabel; property string tileIcon
    property string tileColor; property bool tileActive; property string tileSub
    signal expand()

    readonly property color accent: tileColor
    radius: 16
    color: Theme.card
    border.color: tileActive ? accent : Theme.border; border.width: 1
    Behavior on border.color { ColorAnimation { duration: 180 } }

    Column {
      anchors { fill: parent; margins: 10 }
      spacing: 3

      Row {
        width: parent.width
        Rectangle {
          width: 28; height: 28; radius: 9
          color: Qt.rgba(etile.accent.r, etile.accent.g, etile.accent.b, 0.12)
          Text { anchors.centerIn: parent; text: etile.tileIcon; color: etile.accent; font.pixelSize: 15 }
        }
        Item { width: parent.width - 28 - 16; height: 1 }
        Rectangle {
          width: 10; height: 10; radius: 5; y: 9
          color: tileActive ? etile.accent : Theme.border
          Behavior on color { ColorAnimation { duration: 160 } }
          scale: tileActive ? 1.1 : 0.8
          Behavior on scale { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
        }
      }

      Text { text: etile.tileLabel; color: tileActive ? Theme.fg : Theme.fgMuted; font.pixelSize: 10; font.weight: Font.DemiBold; Behavior on color { ColorAnimation { duration: 180 } } }

      Row {
        spacing: 4
        Text { text: etile.tileSub; color: tileActive ? etile.accent : Theme.fgDim; font.pixelSize: 9; Behavior on color { ColorAnimation { duration: 160 } } }
        Text { text: "›"; color: Theme.fgDim; font.pixelSize: 11; anchors.verticalCenter: parent.verticalCenter }
      }
    }

    HoverHandler { cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: etile.expand() }
  }

  component SimpleTile: Rectangle {
    id: stile
    property string tileLabel; property string tileIcon
    property string tileColor; property bool tileActive
    signal toggle()

    readonly property color accent: tileColor
    radius: 16
    color: Theme.card
    border.color: tileActive ? accent : Theme.border; border.width: 1
    Behavior on border.color { ColorAnimation { duration: 180 } }

    Column {
      anchors { fill: parent; margins: 10 }
      spacing: 3

      Row {
        width: parent.width
        Rectangle {
          width: 28; height: 28; radius: 9
          color: Qt.rgba(stile.accent.r, stile.accent.g, stile.accent.b, 0.12)
          Text { anchors.centerIn: parent; text: stile.tileIcon; color: stile.accent; font.pixelSize: 15 }
        }
        Item { width: parent.width - 28 - 16; height: 1 }
        Rectangle {
          width: 10; height: 10; radius: 5; y: 9
          color: tileActive ? stile.accent : Theme.border
          Behavior on color { ColorAnimation { duration: 160 } }
          scale: tileActive ? 1.1 : 0.8
          Behavior on scale { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
        }
      }

      Text { text: stile.tileLabel; color: tileActive ? Theme.fg : Theme.fgMuted; font.pixelSize: 10; font.weight: Font.DemiBold; Behavior on color { ColorAnimation { duration: 180 } } }
      Text { text: tileActive ? "On" : "Off"; color: tileActive ? stile.accent : Theme.fgDim; font.pixelSize: 9; Behavior on color { ColorAnimation { duration: 160 } } }
    }

    HoverHandler { cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: stile.toggle() }
  }

  component ToggleRow: Rectangle {
    id: trow
    height: 56; radius: 16
    color: Theme.card; border.color: Theme.border; border.width: 1
    property string rowIcon; property string rowLabel; property string rowColor
    property bool rowActive; property string rowSub
    signal toggle()

    readonly property color accent: rowColor

    RowLayout {
      anchors { fill: parent; leftMargin: 14; rightMargin: 12 }
      spacing: 12

      Rectangle {
        width: 34; height: 34; radius: 11
        Layout.alignment: Qt.AlignVCenter
        color: Qt.rgba(trow.accent.r, trow.accent.g, trow.accent.b, rowActive ? 0.22 : 0.10)
        Text { anchors.centerIn: parent; text: trow.rowIcon; color: trow.accent; font.pixelSize: 17 }
      }

      Column {
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignVCenter
        spacing: 1
        Text { text: trow.rowLabel; color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold }
        Text { text: trow.rowSub; color: rowActive ? trow.accent : Theme.fgDim; font.pixelSize: 10; Behavior on color { ColorAnimation { duration: 160 } } }
      }

      Rectangle {
        width: 42; height: 24; radius: 12
        Layout.alignment: Qt.AlignVCenter
        color: rowActive ? Qt.rgba(trow.accent.r, trow.accent.g, trow.accent.b, 0.25) : Theme.input
        border.color: rowActive ? trow.accent : Theme.border; border.width: 1
        Behavior on color { ColorAnimation { duration: 180 } }

        Rectangle {
          width: 18; height: 18; radius: 9; anchors.verticalCenter: parent.verticalCenter
          x: rowActive ? parent.width - width - 3 : 3
          color: rowActive ? trow.accent : Theme.fgDim
          Behavior on x     { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
          Behavior on color { ColorAnimation  { duration: 180 } }
        }
      }
    }

    HoverHandler { cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: trow.toggle() }
  }

  component SliderRow: Item {
    id: srow
    height: 56
    property string rowIcon; property string rowLabel; property string rowColor
    property real sliderValue: 0.5
    signal slid(real value)

    Rectangle {
      anchors.fill: parent; radius: 14
      color: Theme.card; border.color: Theme.border; border.width: 1

      Column {
        anchors { fill: parent; leftMargin: 14; rightMargin: 14; topMargin: 9; bottomMargin: 9 }
        spacing: 7

        Row {
          width: parent.width; spacing: 0
          Text { text: srow.rowIcon; color: srow.rowColor; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter }
          Item  { width: 8; height: 1 }
          Text { text: srow.rowLabel; color: Theme.fgMuted; font.pixelSize: 11; anchors.verticalCenter: parent.verticalCenter }
          Item  { width: parent.width - 96; height: 1 }
          Text { text: Math.round(srow.sliderValue * 100) + "%"; color: srow.rowColor; font.pixelSize: 11; font.weight: Font.DemiBold; anchors.verticalCenter: parent.verticalCenter }
        }

        Item {
          width: parent.width; height: 8
          Rectangle { anchors.fill: parent; radius: 4; color: Theme.bg }
          Rectangle {
            width: parent.width * srow.sliderValue; height: 8; radius: 4; color: srow.rowColor
            Behavior on width { NumberAnimation { duration: 40 } }
            Rectangle {
              anchors { right: parent.right; verticalCenter: parent.verticalCenter }
              width: 14; height: 14; radius: 7; color: Theme.fg; border.color: srow.rowColor; border.width: 2
            }
          }
          MouseArea {
            anchors { fill: parent; margins: -10 }
            cursorShape: Qt.PointingHandCursor
            function update(mx) { var v = Math.max(0, Math.min(1, mx / parent.width)); srow.sliderValue = v; srow.slid(v) }
            onPressed:         function(m) { update(m.x) }
            onPositionChanged: function(m) { if (pressed) update(m.x) }
          }
        }
      }
    }
  }
}
