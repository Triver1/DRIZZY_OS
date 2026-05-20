import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import "../../theme"

Item {
  id: root

  readonly property var columns: [
    { label: "Backlog",     color: Theme.blue   },
    { label: "In Progress", color: Theme.yellow },
    { label: "Done",        color: Theme.accent }
  ]

  ListModel { id: col0 }
  ListModel { id: col1 }
  ListModel { id: col2 }
  readonly property var models: [col0, col1, col2]

  property int nextId: 0

  // Active drag state
  property int dragFromCol: -1
  property int dragFromIdx: -1

  // ── Persistence ───────────────────────────────────────────────────────────

  property string homePath: ""
  property bool stateLoaded: false

  Process {
    running: true
    command: ["sh", "-c", "printf '%s' \"$HOME\""]
    stdout: StdioCollector {
      onStreamFinished: {
        root.homePath = this.text.trim()
        kanbanLoader.command = ["sh", "-c",
          "cat \"" + root.homePath + "/.quickshell-kanban.json\" 2>/dev/null || echo 'null'"]
        kanbanLoader.running = true
      }
    }
  }

  Process {
    id: kanbanLoader
    running: false
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          var s = JSON.parse(this.text.trim())
          if (s && s.columns) {
            for (var c = 0; c < 3; c++) {
              root.models[c].clear()
              var col = s.columns[c] || []
              for (var i = 0; i < col.length; i++)
                root.models[c].append(col[i])
            }
            if (s.nextId !== undefined) root.nextId = s.nextId
          }
        } catch(e) { console.log("kanban load:", e) }
        root.stateLoaded = true
      }
    }
  }

  Timer {
    id: saveDebounce; interval: 800; repeat: false
    onTriggered: root.doSave()
  }

  Process { id: kanbanSaver; running: false }

  function scheduleSave() {
    if (stateLoaded) saveDebounce.restart()
  }

  function doSave() {
    if (!homePath) return
    var cols = []
    for (var c = 0; c < 3; c++) {
      var col = []
      for (var i = 0; i < models[c].count; i++) {
        var item = models[c].get(i)
        col.push({ cid: item.cid, text: item.text })
      }
      cols.push(col)
    }
    kanbanSaver.command = ["python3", "-c",
      "import sys; open(sys.argv[1],'w',encoding='utf-8').write(sys.argv[2])",
      homePath + "/.quickshell-kanban.json",
      JSON.stringify({ nextId: nextId, columns: cols }, null, 2)]
    if (!kanbanSaver.running) kanbanSaver.running = true
  }

  // ── Data operations ───────────────────────────────────────────────────────

  function addCard(colIdx, text) {
    var t = text.trim()
    if (t.length === 0) return
    models[colIdx].append({ cid: nextId++, text: t })
    scheduleSave()
  }

  function removeCard(colIdx, itemIdx) {
    models[colIdx].remove(itemIdx)
    scheduleSave()
  }

  function moveCard(fromCol, fromIdx, toCol) {
    if (fromCol === toCol) return
    var item = models[fromCol].get(fromIdx)
    models[toCol].append({ cid: item.cid, text: item.text })
    models[fromCol].remove(fromIdx)
    scheduleSave()
  }

  // ── UI ────────────────────────────────────────────────────────────────────

  RowLayout {
    anchors { fill: parent; margins: 14; topMargin: 10 }
    spacing: 10

    Repeater {
      model: 3

      Rectangle {
        id: columnRect
        property int colIdx: index
        property bool addingCard: false
        property color colColor: root.columns[index].color

        Layout.fillWidth: true
        Layout.fillHeight: true
        radius: 14
        color: Theme.card
        border.color: dropZone.containsDrag
          ? Qt.rgba(colColor.r, colColor.g, colColor.b, 0.9)
          : Theme.border
        border.width: dropZone.containsDrag ? 2 : 1
        clip: true

        Behavior on border.color { ColorAnimation { duration: 130 } }

        DropArea {
          id: dropZone
          anchors.fill: parent
          keys: ["kanban-card"]
          onDropped: root.moveCard(root.dragFromCol, root.dragFromIdx, columnRect.colIdx)
        }

        Rectangle {
          anchors.fill: parent; radius: parent.radius; z: 0
          color: Qt.rgba(columnRect.colColor.r, columnRect.colColor.g, columnRect.colColor.b, 0.06)
          visible: dropZone.containsDrag
        }

        ColumnLayout {
          anchors { fill: parent; margins: 10 }
          spacing: 8
          z: 1

          // Column header
          RowLayout {
            Layout.fillWidth: true

            Rectangle { width: 8; height: 8; radius: 4; color: columnRect.colColor }

            Text {
              text: root.columns[index].label
              color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold
              Layout.fillWidth: true
            }

            Rectangle {
              width: badge.implicitWidth + 10; height: 18; radius: 9
              color: "transparent"
              border.color: columnRect.colColor; border.width: 1
              Text {
                id: badge; anchors.centerIn: parent
                text: root.models[index].count
                color: columnRect.colColor; font.pixelSize: 9; font.weight: Font.DemiBold
              }
            }

            Rectangle {
              width: 24; height: 24; radius: 12
              color: "transparent"
              border.color: addBtnHov.hovered ? columnRect.colColor : Theme.border
              border.width: 1
              Behavior on border.color { ColorAnimation { duration: 130 } }
              Text {
                anchors.centerIn: parent
                text: columnRect.addingCard ? "󰅖" : "󰐕"
                color: addBtnHov.hovered ? columnRect.colColor : Theme.fgMuted; font.pixelSize: 13
                Behavior on color { ColorAnimation { duration: 130 } }
              }
              HoverHandler { id: addBtnHov; cursorShape: Qt.PointingHandCursor }
              TapHandler {
                onTapped: {
                  columnRect.addingCard = !columnRect.addingCard
                  if (columnRect.addingCard) cardInput.forceActiveFocus()
                }
              }
            }
          }

          // Inline add input
          Rectangle {
            Layout.fillWidth: true; height: 34; radius: 9
            visible: columnRect.addingCard
            color: Theme.input
            border.color: cardInput.activeFocus ? columnRect.colColor : Theme.border
            border.width: 1
            Behavior on border.color { ColorAnimation { duration: 150 } }

            TextInput {
              id: cardInput
              anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
              verticalAlignment: TextInput.AlignVCenter
              color: Theme.fg; font.pixelSize: 11
              selectionColor: Qt.rgba(columnRect.colColor.r, columnRect.colColor.g, columnRect.colColor.b, 0.25)
              selectedTextColor: Theme.fg

              Text {
                anchors.fill: parent; verticalAlignment: Text.AlignVCenter
                text: "Card title..."; color: Theme.fgDim; font.pixelSize: 11
                visible: cardInput.text.length === 0 && !cardInput.activeFocus
              }

              Keys.onReturnPressed: {
                root.addCard(columnRect.colIdx, text)
                text = ""; columnRect.addingCard = false
              }
              Keys.onEscapePressed: {
                text = ""; columnRect.addingCard = false
              }
            }
          }

          // Card list
          ListView {
            Layout.fillWidth: true; Layout.fillHeight: true
            spacing: 6; clip: true
            model: root.models[index]

            delegate: Item {
              id: cardDelegate
              width: parent ? parent.width : 0; height: 64

              property bool beingDragged: root.dragFromCol === columnRect.colIdx && root.dragFromIdx === index

              Rectangle {
                id: cardRect
                width: parent.width; height: 60
                anchors.verticalCenter: parent.verticalCenter
                radius: 10
                color: dragMa.containsMouse && !cardDelegate.beingDragged ? Theme.raised : Theme.surface
                border.color: Theme.border; border.width: 1
                opacity: cardDelegate.beingDragged ? 0.25 : 1.0
                Behavior on color { ColorAnimation { duration: 120 } }
                Behavior on opacity { NumberAnimation { duration: 100 } }

                MouseArea {
                  id: dragMa
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: drag.active ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                  drag.target: ghost
                  drag.filterChildren: true

                  onPressed: {
                    var pos = cardRect.mapToItem(root, 0, 0)
                    ghost.x = pos.x; ghost.y = pos.y
                    ghost.cardText = model.text
                    ghost.visible = true
                    root.dragFromCol = columnRect.colIdx
                    root.dragFromIdx = index
                  }
                  onReleased: {
                    ghost.Drag.drop()
                    ghost.visible = false
                    root.dragFromCol = -1
                    root.dragFromIdx = -1
                  }
                }

                Column {
                  anchors { fill: parent; margins: 8; rightMargin: 30 }
                  spacing: 2

                  Text {
                    width: parent.width
                    text: model.text
                    color: Theme.fg; font.pixelSize: 11
                    wrapMode: Text.WordWrap; maximumLineCount: 3; elide: Text.ElideRight
                  }
                }

                // Delete button
                Rectangle {
                  anchors { right: parent.right; top: parent.top; margins: 5 }
                  width: 22; height: 22; radius: 11
                  visible: dragMa.containsMouse && !dragMa.drag.active
                  color: "transparent"
                  border.color: delHov.hovered ? Theme.red : Theme.border
                  border.width: 1
                  Behavior on border.color { ColorAnimation { duration: 100 } }

                  Text {
                    anchors.centerIn: parent; text: "󰅖"
                    color: delHov.hovered ? Theme.red : Theme.fgMuted; font.pixelSize: 11
                    Behavior on color { ColorAnimation { duration: 100 } }
                  }

                  HoverHandler { id: delHov; cursorShape: Qt.PointingHandCursor }
                  TapHandler { onTapped: root.removeCard(columnRect.colIdx, index) }
                }
              }
            }

            Text {
              anchors.centerIn: parent
              visible: root.models[index].count === 0 && !dropZone.containsDrag
              text: "Drop cards here"
              color: Theme.fgDim; font.pixelSize: 10
            }
          }
        }
      }
    }
  }

  // Ghost card following the drag
  Rectangle {
    id: ghost
    property string cardText: ""

    width: 160; height: 56; radius: 10
    visible: false; z: 999
    color: Theme.raised
    border.color: Theme.accent; border.width: 1
    opacity: 0.88

    Drag.active: ghost.visible
    Drag.keys: ["kanban-card"]
    Drag.hotSpot { x: width / 2; y: height / 2 }

    Text {
      anchors { fill: parent; margins: 8 }
      text: ghost.cardText
      color: Theme.fg; font.pixelSize: 11
      wrapMode: Text.WordWrap; maximumLineCount: 2
    }
  }
}
