import Quickshell
import Quickshell.Wayland
import QtQuick
import QtQuick.Layouts
import "../theme"
import "../widgets/settings"

PanelWindow {
  id: settingsWin
  property var shell

  property string currentPage: "appearance"

  screen: shell.modelData
  anchors { top: true; left: true; right: true; bottom: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: shell.settingsOpen
    ? WlrKeyboardFocus.Exclusive
    : WlrKeyboardFocus.None
  WlrLayershell.namespace: "quickshell-settings"

  visible: shell.settingsOpen || dimBg.opacity > 0.005

  readonly property var pages: [
    { key: "appearance", icon: "󰉼",  label: "Appearance" },
    { key: "network",    icon: "󰖩",  label: "Network"    },
    { key: "display",    icon: "󰍹",  label: "Display"    },
    { key: "audio",      icon: "󰕾",  label: "Audio"      },
    { key: "power",      icon: "󰂟",  label: "Power"      },
    { key: "system",     icon: "󰻠",  label: "System"     },
    { key: "about",      icon: "󰋑",  label: "About"      },
  ]

  // Dim background
  Rectangle {
    id: dimBg
    anchors.fill: parent
    color: Theme.overlayDim
    opacity: shell.settingsOpen ? 1.0 : 0.0
    Behavior on opacity { NumberAnimation { duration: 220 } }

    TapHandler {
      enabled: shell.settingsOpen
      onTapped: function(ep) {
        var pt = dimBg.mapToItem(panel, ep.position.x, ep.position.y)
        if (pt.x < 0 || pt.x > panel.width || pt.y < 0 || pt.y > panel.height)
          shell.settingsOpen = false
      }
    }
  }

  // Main panel
  Rectangle {
    id: panel
    width: 960; height: 580
    anchors.centerIn: parent
    radius: 20; clip: true
    color: Theme.surface
    border.color: Theme.border; border.width: 1

    opacity: shell.settingsOpen ? 1.0 : 0.0
    scale:   shell.settingsOpen ? 1.0 : 0.94
    Behavior on opacity { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
    Behavior on scale   { NumberAnimation { duration: 300; easing.type: Easing.OutBack  } }

    // Absorb clicks so they don't fall through to dimBg
    TapHandler {}

    // Keyboard: Escape closes
    Item {
      anchors.fill: parent
      focus: shell.settingsOpen
      Keys.onEscapePressed: shell.settingsOpen = false
    }

    RowLayout {
      anchors.fill: parent
      spacing: 0

      // ── Left sidebar ───────────────────────────────────────────────────────

      Rectangle {
        Layout.preferredWidth: 200
        Layout.fillHeight: true
        color: Theme.bg
        radius: 20

        // Square off the right-side corners
        Rectangle {
          anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
          width: 20; color: parent.color
        }

        Column {
          anchors { fill: parent; topMargin: 20; leftMargin: 10; rightMargin: 10; bottomMargin: 20 }
          spacing: 2

          // Title
          RowLayout {
            width: parent.width; height: 44

            Text {
              text: "Settings"
              color: Theme.fg; font.pixelSize: 17; font.weight: Font.DemiBold
              Layout.fillWidth: true; leftPadding: 6
            }
            Rectangle {
              width: 28; height: 28; radius: 8
              color: closeHov.hovered ? Theme.card : "transparent"
              Text { anchors.centerIn: parent; text: "✕"; color: Theme.fgMuted; font.pixelSize: 12 }
              HoverHandler { id: closeHov; cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: shell.settingsOpen = false }
            }
          }

          Rectangle { width: parent.width; height: 1; color: Theme.border; opacity: 0.5 }
          Item { width: 1; height: 6 }

          Repeater {
            model: settingsWin.pages

            Rectangle {
              width: parent.width; height: 40; radius: 10
              color: navHov.hovered && settingsWin.currentPage !== modelData.key
                ? Theme.card : "transparent"
              border.color: settingsWin.currentPage === modelData.key ? Theme.accent : "transparent"
              border.width: 1
              Behavior on color { ColorAnimation { duration: 120 } }
              Behavior on border.color { ColorAnimation { duration: 120 } }

              RowLayout {
                anchors { fill: parent; leftMargin: 10; rightMargin: 8 }
                spacing: 10

                Text {
                  text: modelData.icon; font.pixelSize: 17
                  color: settingsWin.currentPage === modelData.key ? Theme.accent : Theme.fgMuted
                  Behavior on color { ColorAnimation { duration: 120 } }
                }
                Text {
                  text: modelData.label
                  color: settingsWin.currentPage === modelData.key ? Theme.fg : Theme.fgMuted
                  font.pixelSize: 12; font.weight: Font.Medium
                  Layout.fillWidth: true
                  Behavior on color { ColorAnimation { duration: 120 } }
                }
              }

              HoverHandler { id: navHov; cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: settingsWin.currentPage = modelData.key }
            }
          }
        }
      }

      // Divider
      Rectangle {
        Layout.preferredWidth: 1
        Layout.fillHeight: true
        color: Theme.border
      }

      // ── Content area ───────────────────────────────────────────────────────

      Item {
        Layout.fillWidth: true
        Layout.fillHeight: true

        // Padding wrapper
        Item {
          anchors { fill: parent; margins: 20; topMargin: 16; bottomMargin: 16 }

          AppearanceSettings {
            anchors.fill: parent
            shell: settingsWin.shell
            visible: settingsWin.currentPage === "appearance"
            opacity: visible ? 1.0 : 0.0
            Behavior on opacity { NumberAnimation { duration: 160 } }
          }

          NetworkSettings {
            anchors.fill: parent
            visible: settingsWin.currentPage === "network"
            opacity: visible ? 1.0 : 0.0
            Behavior on opacity { NumberAnimation { duration: 160 } }
          }

          DisplaySettings {
            anchors.fill: parent
            visible: settingsWin.currentPage === "display"
            opacity: visible ? 1.0 : 0.0
            Behavior on opacity { NumberAnimation { duration: 160 } }
          }

          AudioSettings {
            anchors.fill: parent
            visible: settingsWin.currentPage === "audio"
            opacity: visible ? 1.0 : 0.0
            Behavior on opacity { NumberAnimation { duration: 160 } }
          }

          PowerSettings {
            anchors.fill: parent
            visible: settingsWin.currentPage === "power"
            opacity: visible ? 1.0 : 0.0
            Behavior on opacity { NumberAnimation { duration: 160 } }
          }

          SystemSettings {
            anchors.fill: parent
            visible: settingsWin.currentPage === "system"
            opacity: visible ? 1.0 : 0.0
            Behavior on opacity { NumberAnimation { duration: 160 } }
          }

          AboutSettings {
            anchors.fill: parent
            visible: settingsWin.currentPage === "about"
            opacity: visible ? 1.0 : 0.0
            Behavior on opacity { NumberAnimation { duration: 160 } }
          }
        }
      }
    }
  }
}
