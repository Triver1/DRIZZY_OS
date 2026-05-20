import Quickshell
import Quickshell.Wayland
import QtQuick
import Niri 0.1
import "../widgets/topbar"
import "../theme"

PanelWindow {
  id: bar
  property var shell
  property var niri
  property var stopwatchTimer
  property var notifOverlay

  screen: shell.modelData
  anchors { top: true; left: true; right: true }

  implicitHeight: 28
  exclusiveZone: 28
  color: "transparent"

  WlrLayershell.namespace: "quickshell-bar-blur"

  // Very subtle bottom border
  Rectangle {
    anchors { bottom: parent.bottom; left: parent.left; right: parent.right }
    height: 1
    color: Qt.rgba(Theme.border.r, Theme.border.g, Theme.border.b, 0.3)
  }

  LeftPill {
    id: leftPill
    anchors { left: parent.left; leftMargin: 10; verticalCenter: parent.verticalCenter }
    shell: bar.shell
    niri: bar.niri
  }

  Row {
    id: centerCluster
    anchors { horizontalCenter: parent.horizontalCenter; verticalCenter: parent.verticalCenter }
    spacing: 8

    CenterPill {
      shell: bar.shell
      dateClock: dateClock
    }

    FilePill {
      shell: bar.shell
    }
  }

  RightPill {
    anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
    shell: bar.shell
    notifOverlay: bar.notifOverlay
  }

  SystemClock { id: dateClock; precision: SystemClock.Minutes; enabled: true }
}
