import QtQuick
import Quickshell
import Quickshell.Wayland
import "../theme"

PanelWindow {
  id: popup
  property var shell
  property string icon: ""
  property string title: ""
  property string subtitle: ""

  screen: shell.modelData
  anchors { top: true; left: true }
  color: "transparent"
  exclusionMode: ExclusionMode.Ignore
  WlrLayershell.namespace: "quickshell-statuspopup"
  WlrLayershell.layer: WlrLayer.Overlay

  Component.onCompleted: positionCenter()

  function positionCenter() {
    implicitWidth  = card.implicitWidth
    implicitHeight = card.implicitHeight
    margins.left = Math.round((shell.modelData.width  - card.implicitWidth)  / 2)
    margins.top  = Math.round((shell.modelData.height - card.implicitHeight) / 2)
  }

  function show(ic, t, sub) {
    icon = ic; title = t; subtitle = sub || ""
    implicitWidth  = card.implicitWidth
    implicitHeight = card.implicitHeight
    positionCenter()
    card.opacity = 1
    hideTimer.restart()
  }

  Timer {
    id: hideTimer
    interval: 1500
    onTriggered: {
      card.opacity = 0
      implicitWidth  = 0
      implicitHeight = 0
    }
  }

  Rectangle {
    id: card
    implicitWidth:  row.implicitWidth + 44
    implicitHeight: row.implicitHeight + 34
    radius: 22
    color: "#f8302d31"
    border.color: Theme.border; border.width: 1
    opacity: 0
    scale: opacity < 1.0 ? 0.82 : 1.0

    Behavior on opacity { NumberAnimation { duration: 180 } }
    Behavior on scale   { NumberAnimation { duration: 280; easing.type: Easing.OutBack } }

    Row {
      id: row
      anchors.centerIn: parent
      spacing: 14

      Rectangle {
        width: 46; height: 46; radius: 15
        color: Qt.rgba(Theme.purple.r, Theme.purple.g, Theme.purple.b, 0.15)
        anchors.verticalCenter: parent.verticalCenter
        Text {
          anchors.centerIn: parent
          text: popup.icon
          color: Theme.purple; font.pixelSize: 22
        }
      }

      Column {
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2
        Text {
          text: popup.title
          color: Theme.fg; font.pixelSize: 14; font.weight: Font.DemiBold
        }
        Text {
          text: popup.subtitle
          color: Theme.fgMuted; font.pixelSize: 12
          visible: popup.subtitle.length > 0
        }
      }
    }
  }
}
