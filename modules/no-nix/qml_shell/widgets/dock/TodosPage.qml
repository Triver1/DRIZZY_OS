import QtQuick
import QtQuick.Layouts
import "../../theme"

Item {
  id: root

  // 0=all, 1=active, 2=done
  property int filter: 0
  readonly property var filterLabels: ["All", "Active", "Done"]

  ListModel {
    id: todoModel
  }

  property int nextId: 0

  function addTodo(text) {
    var t = text.trim()
    if (t.length === 0) return
    todoModel.append({ tid: nextId++, text: t, done: false })
    newInput.text = ""
  }

  function toggleTodo(idx) {
    todoModel.setProperty(idx, "done", !todoModel.get(idx).done)
  }

  function removeTodo(idx) {
    todoModel.remove(idx)
  }

  function visibleAt(idx) {
    var item = todoModel.get(idx)
    if (!item) return false
    if (filter === 0) return true
    if (filter === 1) return !item.done
    return item.done
  }

  ColumnLayout {
    anchors { fill: parent; margins: 16; topMargin: 10 }
    spacing: 10

    // Header row: title + filter tabs + count
    RowLayout {
      Layout.fillWidth: true
      spacing: 8

      Text {
        text: "Todos"
        color: Theme.fg; font.pixelSize: 14; font.weight: Font.DemiBold
      }

      Rectangle {
        width: countText.implicitWidth + 10; height: 18; radius: 9
        color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
        Text {
          id: countText
          anchors.centerIn: parent
          text: {
            var c = 0
            for (var i = 0; i < todoModel.count; i++) if (!todoModel.get(i).done) c++
            return c
          }
          color: Theme.accent; font.pixelSize: 10; font.weight: Font.DemiBold
        }
      }

      Item { Layout.fillWidth: true }

      Repeater {
        model: root.filterLabels
        Rectangle {
          property bool active: root.filter === index
          width: fl.implicitWidth + 16; height: 24; radius: 12
          color: active ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15) : "transparent"
          border.color: active ? Theme.accent : Theme.border; border.width: 1
          Behavior on color { ColorAnimation { duration: 130 } }
          Text {
            id: fl; anchors.centerIn: parent; text: modelData
            color: active ? Theme.accent : Theme.fgMuted
            font.pixelSize: 10; font.weight: Font.Medium
            Behavior on color { ColorAnimation { duration: 130 } }
          }
          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler { onTapped: root.filter = index }
        }
      }
    }

    // Input row
    Rectangle {
      Layout.fillWidth: true; height: 36; radius: 10
      color: Theme.input; border.color: newInput.activeFocus ? Theme.borderFocus : Theme.border
      border.width: 1
      Behavior on border.color { ColorAnimation { duration: 150 } }

      Row {
        anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
        spacing: 6

        Text {
          anchors.verticalCenter: parent.verticalCenter
          text: ""; color: Theme.fgMuted; font.pixelSize: 14
        }

        TextInput {
          id: newInput
          width: parent.width - 20
          anchors.verticalCenter: parent.verticalCenter
          color: Theme.fg; font.pixelSize: 12
          selectionColor: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.3)
          selectedTextColor: Theme.fg

          Text {
            anchors.fill: parent; anchors.verticalCenter: undefined
            verticalAlignment: Text.AlignVCenter
            text: "Add a task..."
            color: Theme.fgDim; font.pixelSize: 12
            visible: newInput.text.length === 0 && !newInput.activeFocus
          }

          Keys.onReturnPressed: root.addTodo(text)
          Keys.onEnterPressed:  root.addTodo(text)
        }
      }
    }

    // Todo list
    ListView {
      id: listView
      Layout.fillWidth: true; Layout.fillHeight: true
      spacing: 5; clip: true

      model: todoModel

      delegate: Item {
        width: parent ? parent.width : 0
        visible: root.visibleAt(index)
        height: visible ? 42 : 0

        Behavior on height { NumberAnimation { duration: 160; easing.type: Easing.OutQuart } }

        Rectangle {
          id: card
          anchors { fill: parent; bottomMargin: 0 }
          height: 38
          anchors.verticalCenter: parent.verticalCenter
          radius: 10
          color: rowHov.hovered ? Theme.raised : Theme.card
          border.color: Theme.border; border.width: 1
          Behavior on color { ColorAnimation { duration: 120 } }

          HoverHandler { id: rowHov }

          Row {
            anchors { fill: parent; leftMargin: 10; rightMargin: 8 }
            spacing: 8

            // Checkbox
            Rectangle {
              width: 18; height: 18; radius: 9
              anchors.verticalCenter: parent.verticalCenter
              color: model.done
                ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.2)
                : "transparent"
              border.color: model.done ? Theme.accent : Theme.border; border.width: 2
              Behavior on color { ColorAnimation { duration: 120 } }
              Behavior on border.color { ColorAnimation { duration: 120 } }

              Text {
                anchors.centerIn: parent; text: "󰄵"
                color: Theme.accent; font.pixelSize: 11
                visible: model.done
              }
              HoverHandler { cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: root.toggleTodo(index) }
            }

            // Text
            Text {
              width: parent.width - 18 - 28 - 8 * 2
              anchors.verticalCenter: parent.verticalCenter
              text: model.text
              color: model.done ? Theme.fgDim : Theme.fg
              font.pixelSize: 12
              font.strikeout: model.done
              elide: Text.ElideRight
              Behavior on color { ColorAnimation { duration: 120 } }
            }

            // Delete
            Rectangle {
              width: 24; height: 24; radius: 12
              anchors.verticalCenter: parent.verticalCenter
              color: delHov.hovered ? Qt.rgba(Theme.red.r, Theme.red.g, Theme.red.b, 0.18) : "transparent"
              visible: rowHov.hovered
              Behavior on color { ColorAnimation { duration: 120 } }
              Text {
                anchors.centerIn: parent; text: "󰅖"
                color: Theme.red; font.pixelSize: 12
              }
              HoverHandler { id: delHov; cursorShape: Qt.PointingHandCursor }
              TapHandler { onTapped: root.removeTodo(index) }
            }
          }
        }
      }

      Text {
        anchors.centerIn: parent
        visible: todoModel.count === 0
        text: "Nothing here. Add a task above."
        color: Theme.fgDim; font.pixelSize: 11
      }
    }
  }
}
