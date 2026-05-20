import Quickshell
import Quickshell.Wayland
import QtQuick
import "../theme"

PanelWindow {
  id: launcherWin
  property var shell

  screen: shell.modelData
  anchors { top: true; left: true; right: true; bottom: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: shell.launcherOpen
    ? WlrKeyboardFocus.Exclusive
    : WlrKeyboardFocus.None
  WlrLayershell.namespace: "quickshell-applauncher"

  visible: shell.launcherOpen || dimBg.opacity > 0.005

  property var allApps: []

  function buildApps() {
    var result = []
    var apps = DesktopEntries.applications
    if (!apps || !apps.values) return
    var vals = apps.values
    for (var i = 0; i < vals.length; i++) {
      var e = vals[i]
      if (e && !e.noDisplay && e.name) result.push(e)
    }
    allApps = result
    appGrid.model = result
  }

  function fuzzyScore(text, query) {
    if (!text) return 0
    var t = text.toLowerCase()
    var q = query.toLowerCase()
    if (t === q) return 200
    if (t.startsWith(q)) return 150
    var idx = t.indexOf(q)
    if (idx > 0) return 70
    var ti = 0, qi = 0, score = 0
    while (ti < t.length && qi < q.length) {
      if (t[ti] === q[qi]) { score += Math.max(1, 8 - qi); qi++ }
      ti++
    }
    return qi === q.length ? score : 0
  }

  function searchApps(query) {
    if (!query || !query.trim()) {
      appGrid.model = allApps
      return
    }
    var q = query.trim()
    var scored = []
    for (var i = 0; i < allApps.length; i++) {
      var a = allApps[i]
      if (!a) continue
      var s = fuzzyScore(a.name || "", q) * 3
            + fuzzyScore(a.genericName || "", q) * 2
            + fuzzyScore(a.comment || "", q)
      if (s > 0) scored.push({ app: a, score: s })
    }
    scored.sort(function(x, y) { return y.score - x.score })
    appGrid.model = scored.map(function(item) { return item.app })
  }

  Connections {
    target: launcherWin.shell
    function onLauncherOpenChanged() {
      if (launcherWin.shell.launcherOpen) {
        launcherWin.buildApps()
        searchInput.text = ""
        searchInput.forceActiveFocus()
      }
    }
  }

  // Dimmed background
  Rectangle {
    id: dimBg
    anchors.fill: parent
    color: Theme.overlayDim
    opacity: shell.launcherOpen ? 1.0 : 0.0
    Behavior on opacity { NumberAnimation { duration: 220 } }

    TapHandler {
      enabled: shell.launcherOpen
      onTapped: shell.launcherOpen = false
    }
  }

  // Centered search panel
  Rectangle {
    id: searchPanel
    width: 700
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.top: parent.top
    anchors.topMargin: Math.round(shell.modelData.height * 0.15)

    height: searchBar.height + gridContainer.height + 32
    radius: 22
    clip: true
    color: Theme.surface
    border.color: Theme.border
    border.width: 1

    opacity: shell.launcherOpen ? 1.0 : 0.0
    scale:   shell.launcherOpen ? 1.0 : 0.93
    transformOrigin: Item.Top

    Behavior on opacity { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
    Behavior on scale   { NumberAnimation { duration: 300; easing.type: Easing.OutBack  } }

    // Prevent taps inside panel from falling through to dimBg
    TapHandler {}

    Column {
      anchors { top: parent.top; left: parent.left; right: parent.right; margins: 16 }
      spacing: 14

      // Search bar
      Rectangle {
        id: searchBar
        width: parent.width; height: 58; radius: 16
        color: Theme.input
        border.color: searchInput.activeFocus ? Theme.accent : Theme.border
        border.width: 1
        Behavior on border.color { ColorAnimation { duration: 150 } }

        Row {
          anchors { fill: parent; leftMargin: 18; rightMargin: 18 }
          spacing: 14

          Text {
            text: "󰍉"
            color: Theme.accent; font.pixelSize: 20
            anchors.verticalCenter: parent.verticalCenter
          }

          TextInput {
            id: searchInput
            width: parent.width - 48
            anchors.verticalCenter: parent.verticalCenter
            color: Theme.fg; font.pixelSize: 16; font.weight: Font.Medium
            selectionColor: Qt.rgba(0.62, 0.80, 0.44, 0.28)
            selectedTextColor: Theme.accent
            clip: true

            Text {
              anchors.fill: parent
              text: "Search applications..."
              color: Theme.fgDim; font.pixelSize: 16
              verticalAlignment: Text.AlignVCenter
              visible: !searchInput.text && !searchInput.activeFocus
            }

            onTextChanged: launcherWin.searchApps(text)

            Keys.onEscapePressed: {
              if (text.length > 0) text = ""
              else shell.launcherOpen = false
            }

            onAccepted: {
              var m = appGrid.model
              if (m && m.length > 0) {
                shell.launchApp(m[0])
                text = ""
              }
            }
          }
        }
      }

      // App grid container
      Item {
        id: gridContainer
        width: parent.width
        clip: true
        height: appGrid.count > 0 ? Math.min(Math.ceil(appGrid.count / 6), 4) * 104 : 60

        Behavior on height { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }

        GridView {
          id: appGrid
          anchors.fill: parent
          cellWidth: Math.floor(parent.width / 6)
          cellHeight: 104
          clip: true

          delegate: Item {
            width: appGrid.cellWidth; height: 100

            property var entry: modelData || null

            Rectangle {
              id: appTile
              anchors { centerIn: parent }
              width: parent.width - 6; height: 92
              radius: 16
              color: tileHover.hovered
                ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12)
                : "transparent"
              border.color: tileHover.hovered ? Theme.accent : "transparent"
              border.width: 1
              Behavior on color { ColorAnimation { duration: 120 } }
              Behavior on border.color { ColorAnimation { duration: 120 } }

              Column {
                anchors.centerIn: parent; spacing: 7

                Image {
                  anchors.horizontalCenter: parent.horizontalCenter
                  width: 48; height: 48
                  source: entry && entry.icon ? Quickshell.iconPath(entry.icon) : ""
                  sourceSize.width: 48; sourceSize.height: 48
                  asynchronous: true; fillMode: Image.PreserveAspectFit; smooth: true
                }

                Text {
                  anchors.horizontalCenter: parent.horizontalCenter
                  text: entry ? (entry.name || "") : ""
                  color: Theme.fg; font.pixelSize: 10; font.weight: Font.Medium
                  elide: Text.ElideRight; width: appGrid.cellWidth - 10
                  horizontalAlignment: Text.AlignHCenter; maximumLineCount: 1
                }
              }
            }

            HoverHandler { id: tileHover; cursorShape: Qt.PointingHandCursor }
            TapHandler {
              onTapped: {
                if (entry) {
                  shell.launchApp(entry)
                  searchInput.text = ""
                }
              }
            }
          }
        }

        Text {
          anchors.centerIn: parent
          visible: appGrid.count === 0
          text: "No results"
          color: Theme.fgDim; font.pixelSize: 13
        }
      }
    }
  }
}
