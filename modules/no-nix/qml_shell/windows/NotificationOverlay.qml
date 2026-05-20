import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Notifications
import "../theme"

PanelWindow {
  id: notifWin
  property var shell
  required property var notifServer
  readonly property int cardWidth: 340
  readonly property int slideRoom: 32

  screen: shell.modelData
  anchors { top: true; right: true }
  margins { top: 40; right: 12 }
  implicitWidth: notifModel.count > 0 ? cardWidth + slideRoom : 0
  implicitHeight: notifModel.count > 0 ? 600 : 0
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "quickshell-notifications"

  Connections {
    target: notifServer
    function onNotification(notif) {
      notif.tracked = true
      notifModel.insert(0, {
        summary: notif.summary || "",
        body: notif.body || "",
        icon: notif.appIcon || "",
        actions: [],
        notifObj: notif
      })
      notif.closed.connect(function(reason) {
        for (var i = 0; i < notifModel.count; i++) {
          if (notifModel.get(i).notifObj === notif) {
            notifModel.remove(i)
            break
          }
        }
      })
      dismissTimer.restart()
    }
  }

  Column {
    anchors.fill: parent
    spacing: 8

    Repeater {
      model: notifModel

      Rectangle {
        id: notifCard
        width: notifWin.cardWidth; height: cardContent.implicitHeight + 28
        radius: 18
        color: "#f8302d31"
        border.color: Theme.border; border.width: 1
        clip: true

        property real cardOpacity: 0.0
        property real cardScale:   0.92
        property real cardX:       0

        opacity: cardOpacity
        scale:   cardScale
        x:       cardX
        transformOrigin: Item.TopRight

        Behavior on cardOpacity { NumberAnimation { duration: 220 } }
        Behavior on cardScale   { NumberAnimation { duration: 260; easing.type: Easing.OutBack } }
        Behavior on cardX       { NumberAnimation { duration: 260; easing.type: Easing.OutCubic } }

        Component.onCompleted: {
          cardOpacity = 1.0
          cardScale   = 1.0
          cardX       = notifWin.slideRoom
        }

        Column {
          id: cardContent
          anchors { left: parent.left; right: parent.right; top: parent.top; margins: 14 }
          spacing: 4

          Item {
            width: parent.width
            height: 18

            Image {
              id: appIcon
              width: 18; height: 18
              source: model.icon && shell ? Quickshell.iconPath(model.icon) : ""
              sourceSize.width: 18; sourceSize.height: 18
              visible: model.icon !== ""
              anchors { left: parent.left; verticalCenter: parent.verticalCenter }
            }

            Text {
              id: closeButton
              text: "󰅖"
              color: Theme.fgDim; font.pixelSize: 14
              anchors { right: parent.right; verticalCenter: parent.verticalCenter }
              TapHandler { onTapped: dismissNotification(index) }
              HoverHandler { cursorShape: Qt.PointingHandCursor }
            }

            Text {
              text: model.summary || ""
              color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold
              elide: Text.ElideRight
              anchors {
                left: parent.left
                leftMargin: appIcon.visible ? 26 : 0
                right: closeButton.left
                rightMargin: 8
                verticalCenter: parent.verticalCenter
              }
            }
          }

          Text {
            visible: model.body !== ""
            text: model.body || ""
            color: Theme.fgMuted; font.pixelSize: 11
            width: parent.width
            elide: Text.ElideRight; maximumLineCount: 2; wrapMode: Text.Wrap
          }
        }
      }
    }
  }

  ListModel { id: notifModel; objectName: "notifModel" }

  property alias notificationModel: notifModel

  function showNotification(summary, body, icon, actions) {
    notifModel.insert(0, {
      summary: summary, body: body || "",
      icon: icon || "", actions: actions || [],
      notifObj: null
    })
    dismissTimer.restart()
  }

  function dismissNotification(idx) {
    if (idx < 0 || idx >= notifModel.count) return
    var entry = notifModel.get(idx)
    if (entry.notifObj) entry.notifObj.dismiss()
    notifModel.remove(idx)
  }

  Timer {
    id: dismissTimer
    interval: 5000
    onTriggered: {
      if (notifModel.count > 0) {
        var last = notifModel.get(notifModel.count - 1)
        if (last.notifObj) last.notifObj.expire()
        notifModel.remove(notifModel.count - 1)
        if (notifModel.count > 0) restart()
      }
    }
  }
}
