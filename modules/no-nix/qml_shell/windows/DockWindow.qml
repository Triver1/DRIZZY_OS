import Quickshell
import Quickshell.Wayland
import QtQuick
import "../widgets/dock"
import "../theme"

PanelWindow {
  id: dockWin
  property var shell
  property var niri
  property var stopwatchTimer
  property var notifModel

  property string currentPage: shell ? shell.dockPage : "dashboard"

  // Max dimensions — window size is FIXED. No Wayland geometry updates during animation,
  // which is the main cause of choppiness.
  readonly property int maxW: 940
  readonly property int maxH: 530

  screen: shell.modelData
  anchors { top: true; left: true }
  margins.top:  40
  margins.left: Math.round((shell.modelData.width - maxW) / 2)

  implicitWidth:  maxW
  implicitHeight: maxH

  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  visible: shell.dockOpen || mainPanel.opacity > 0.005
  WlrLayershell.namespace: "quickshell-dashboard"
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

  // Per-page target dimensions for the content panel
  property int targetWidth: {
    if (currentPage === "dashboard") return 920
    if (currentPage === "kanban")    return 920
    if (currentPage === "timer")     return 520
    return 660
  }
  property int targetHeight: {
    if (currentPage === "wallpapers") return 530
    if (currentPage === "timer")      return 420
    return 490
  }

  // Animated panel dimensions — only animate when the panel is actually visible
  property int panelWidth:  targetWidth
  property int panelHeight: targetHeight

  Behavior on panelWidth {
    enabled: shell && shell.dockOpen && mainPanel.opacity > 0.4
    NumberAnimation { duration: 400; easing.type: Easing.OutQuint }
  }
  Behavior on panelHeight {
    enabled: shell && shell.dockOpen && mainPanel.opacity > 0.4
    NumberAnimation { duration: 400; easing.type: Easing.OutQuint }
  }

  Rectangle {
    id: mainPanel
    width:  dockWin.panelWidth
    height: dockWin.panelHeight
    anchors { top: parent.top; horizontalCenter: parent.horizontalCenter }

    radius: 22
    color: Theme.surface
    border.color: Theme.border
    border.width: 1
    clip: true

    opacity: shell.dockOpen ? 1.0 : 0.0
    scale:   shell.dockOpen ? 1.0 : 0.94
    transformOrigin: Item.Top

    Behavior on opacity { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
    Behavior on scale   { NumberAnimation { duration: 360; easing.type: Easing.OutBack  } }

    // Subtle drop shadow
    Rectangle {
      anchors { fill: parent; margins: -3; topMargin: 6 }
      z: -1; radius: parent.radius + 3
      color: Theme.shadow
    }

    HoverHandler {
      onHoveredChanged: shell.dockOpen = hovered
    }

    NavTabs {
      id: navBar
      anchors { top: parent.top; topMargin: 10; horizontalCenter: parent.horizontalCenter }
      dockWin: dockWin
    }

    Item {
      anchors {
        top: navBar.bottom; topMargin: 2
        left: parent.left; right: parent.right; bottom: parent.bottom
      }
      clip: true

      DashboardPage {
        anchors.fill: parent
        shell: dockWin.shell
        stopwatchTimer: dockWin.stopwatchTimer
        notifModel: dockWin.notifModel
        visible: dockWin.currentPage === "dashboard"
        opacity: visible ? 1.0 : 0.0
        Behavior on opacity { NumberAnimation { duration: 180 } }
      }

      WallpapersPage {
        anchors.fill: parent
        shell: dockWin.shell
        visible: dockWin.currentPage === "wallpapers"
        opacity: visible ? 1.0 : 0.0
        Behavior on opacity { NumberAnimation { duration: 180 } }
      }

      TimerPage {
        anchors.fill: parent
        visible: dockWin.currentPage === "timer"
        opacity: visible ? 1.0 : 0.0
        Behavior on opacity { NumberAnimation { duration: 180 } }
      }

      KanbanPage {
        anchors.fill: parent
        visible: dockWin.currentPage === "kanban"
        opacity: visible ? 1.0 : 0.0
        Behavior on opacity { NumberAnimation { duration: 180 } }
      }
    }
  }
}
