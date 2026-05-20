import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import "../../theme"

Item {
  id: root

  property var shell
  property string homePath: ""

  Process {
    running: true
    command: ["sh", "-c", "printf '%s' \"$HOME\""]
    stdout: StdioCollector {
      onStreamFinished: root.homePath = this.text.trim()
    }
  }

  Process { id: themeSaver; running: false }

  function selectTheme(idx) {
    Theme.applyTheme(idx)
    persist(idx)
  }

  function selectWallpaperTheme() {
    // Save index 3, let ScreenShell.maybeRunMatugen() do the color extraction
    Theme.currentTheme = 3
    persist(3)
    if (root.shell) root.shell.maybeRunMatugen()
  }

  function persist(idx) {
    if (!homePath) return
    themeSaver.command = ["python3", "-c",
      "import sys; open(sys.argv[1],'w',encoding='utf-8').write(sys.argv[2])",
      homePath + "/.quickshell-theme.json",
      JSON.stringify({ themeIndex: idx })]
    if (!themeSaver.running) themeSaver.running = true
  }

  ColumnLayout {
    anchors.fill: parent
    spacing: 24

    Text {
      text: "Appearance"
      color: Theme.fg; font.pixelSize: 16; font.weight: Font.DemiBold
    }

    ColumnLayout {
      Layout.fillWidth: true
      spacing: 12

      Text {
        text: "THEME"
        color: Theme.fgMuted; font.pixelSize: 10; font.weight: Font.Bold
        font.letterSpacing: 1.2
      }

      Row {
        spacing: 14

        // Static theme cards (0, 1, 2)
        Repeater {
          model: Theme.themes

          Rectangle {
            id: themeCard
            property var td: modelData
            property bool sel: Theme.currentTheme === index

            width: 178; height: 116; radius: 16
            color: td.card
            border.color: sel ? td.green : td.border
            border.width: sel ? 2 : 1
            Behavior on border.color { ColorAnimation { duration: 180 } }

            Column {
              anchors { fill: parent; margins: 14 }
              spacing: 8

              // Accent dot row
              Row {
                spacing: 6
                Repeater {
                  model: [themeCard.td.green, themeCard.td.blue, themeCard.td.purple,
                          themeCard.td.red, themeCard.td.orange, themeCard.td.yellow]
                  Rectangle { width: 14; height: 14; radius: 7; color: modelData }
                }
              }

              // Bg scale strip
              Row {
                spacing: 4
                Repeater {
                  model: [themeCard.td.bg, themeCard.td.surface, themeCard.td.card, themeCard.td.raised]
                  Rectangle {
                    width: 22; height: 10; radius: 3; color: modelData
                    border.color: themeCard.td.border; border.width: 1
                  }
                }
              }

              Column {
                spacing: 2
                Text {
                  text: themeCard.td.name
                  color: themeCard.td.fg; font.pixelSize: 13; font.weight: Font.DemiBold
                }
                Text {
                  text: themeCard.td.description
                  color: themeCard.td.fgDim; font.pixelSize: 10
                }
              }
            }

            // Active badge
            Rectangle {
              anchors { right: parent.right; top: parent.top; margins: 10 }
              width: badgeLabel.implicitWidth + 12; height: 20; radius: 10
              visible: themeCard.sel
              color: "transparent"
              border.color: themeCard.td.green; border.width: 1
              Text {
                id: badgeLabel; anchors.centerIn: parent; text: "Active"
                color: themeCard.td.green; font.pixelSize: 9; font.weight: Font.DemiBold
              }
            }

            HoverHandler { cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: root.selectTheme(index) }
          }
        }

        // Wallpaper card (index 3, dynamic)
        Rectangle {
          id: wpCard
          property bool sel: Theme.currentTheme === 3

          width: 178; height: 116; radius: 16
          color: Theme.card
          border.color: sel ? Theme.green : Theme.border
          border.width: sel ? 2 : 1
          clip: true
          Behavior on border.color { ColorAnimation { duration: 180 } }

          // Wallpaper thumbnail as background
          Image {
            anchors.fill: parent
            source: root.shell ? root.shell.wallpaperPath : ""
            fillMode: Image.PreserveAspectCrop
            opacity: 0.35
            asynchronous: true; smooth: true; cache: false
          }

          // Overlay for readability
          Rectangle {
            anchors.fill: parent; radius: parent.radius
            color: Qt.rgba(Theme.card.r, Theme.card.g, Theme.card.b, 0.55)
          }

          Column {
            anchors { fill: parent; margins: 14 }
            spacing: 8

            // Live accent dots (from current matugen-derived colors, if active)
            Row {
              spacing: 6
              Repeater {
                model: wpCard.sel
                  ? [Theme.green, Theme.blue, Theme.purple, Theme.red, Theme.orange, Theme.yellow]
                  : ["#888", "#777", "#666", "#555", "#444", "#333"]
                Rectangle { width: 14; height: 14; radius: 7; color: modelData }
              }
            }

            Text {
              text: ""; font.pixelSize: 22; color: Theme.fgMuted
            }

            Column {
              spacing: 2
              Text {
                text: "Wallpaper"
                color: Theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold
              }
              Text {
                text: "Colors from your wallpaper"
                color: Theme.fgDim; font.pixelSize: 10
              }
            }
          }

          // Active badge
          Rectangle {
            anchors { right: parent.right; top: parent.top; margins: 10 }
            width: wpBadge.implicitWidth + 12; height: 20; radius: 10
            visible: wpCard.sel
            color: "transparent"
            border.color: Theme.green; border.width: 1
            Text {
              id: wpBadge; anchors.centerIn: parent; text: "Active"
              color: Theme.green; font.pixelSize: 9; font.weight: Font.DemiBold
            }
          }

          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler { onTapped: root.selectWallpaperTheme() }
        }
      }

      Text {
        text: Theme.currentTheme === 3
          ? "Wallpaper — colors extracted via matugen on each wallpaper change"
          : Theme.themes[Theme.currentTheme].name + " — " + Theme.themes[Theme.currentTheme].description
        color: Theme.fgDim; font.pixelSize: 11
      }
    }

    Item { Layout.fillHeight: true }
  }
}
