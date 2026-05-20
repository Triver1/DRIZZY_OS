import QtQuick
import "../../theme"

Item {
  id: root
  property var notifModel

  ListView {
    anchors.fill: parent
    spacing: 6
    clip: true
    model: root.notifModel || null

    delegate: Rectangle {
      width: parent ? parent.width : 0
      height: 48; radius: 12
      color: Theme.card
      border.color: Theme.border; border.width: 1

      Row {
        anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
        spacing: 8

        Rectangle {
          width: 28; height: 28; radius: 8
          anchors.verticalCenter: parent.verticalCenter
          color: Qt.rgba(Theme.red.r, Theme.red.g, Theme.red.b, 0.15)
          Text {
            anchors.centerIn: parent
            text: "󰎟"; color: Theme.red; font.pixelSize: 13
          }
        }

        Column {
          anchors.verticalCenter: parent.verticalCenter
          spacing: 1

          Text {
            text: model.summary || "Notification"
            color: Theme.fg; font.pixelSize: 10; font.weight: Font.DemiBold
            elide: Text.ElideRight; width: 130; maximumLineCount: 1
          }
          Text {
            text: model.body || ""
            color: Theme.fgMuted; font.pixelSize: 9
            elide: Text.ElideRight; width: 130; maximumLineCount: 1
          }
        }
      }
    }
  }

  Text {
    visible: !root.notifModel || root.notifModel.count === 0
    anchors.centerIn: parent
    text: "No notifications"
    color: Theme.fgDim; font.pixelSize: 10
  }
}
