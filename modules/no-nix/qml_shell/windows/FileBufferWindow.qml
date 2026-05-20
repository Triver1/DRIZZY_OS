pragma ComponentBehavior: Bound

import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import QtQuick
import QtQuick.Dialogs
import "../theme"

PanelWindow {
  id: bufferWin
  property var shell

  readonly property int panelW: 340
  readonly property int panelH: 310

  screen: shell.modelData
  anchors { top: true; left: true }
  margins.top: 34
  margins.left: Math.round((shell.modelData.width - panelW) / 2)
  implicitWidth: panelW
  implicitHeight: panelH
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  visible: bufferWin.shell.fileBufferOpen || panel.opacity > 0.005

  WlrLayershell.namespace: "quickshell-file-buffer"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

  ListModel { id: fileModel }

  FileView {
    id: store
    path: Quickshell.statePath("file-buffer.json")
    blockLoading: true
    printErrors: false
    atomicWrites: true

    Component.onCompleted: bufferWin.loadStore()
    onLoaded: bufferWin.loadStore()
  }

  FileDialog {
    id: picker
    title: "Add files"
    fileMode: FileDialog.OpenFiles
    acceptLabel: "Add"
    onAccepted: bufferWin.addUrls(selectedFiles)
  }

  Connections {
    target: bufferWin.shell
    function onFileBufferDropSerialChanged() {
      if (bufferWin.shell.fileBufferDropUrls && bufferWin.shell.fileBufferDropUrls.length > 0)
        bufferWin.addUrls(bufferWin.shell.fileBufferDropUrls)
      else if (bufferWin.shell.fileBufferDropText)
        bufferWin.addTextList(bufferWin.shell.fileBufferDropText)
    }
  }

  function basename(path) {
    var clean = (path || "").replace(/\/+$/, "")
    var idx = clean.lastIndexOf("/")
    return idx >= 0 ? clean.slice(idx + 1) : clean
  }

  function dirname(path) {
    var idx = (path || "").lastIndexOf("/")
    return idx > 0 ? path.slice(0, idx) : path
  }

  function pathToUrl(path) {
    if (!path) return ""
    if (path.indexOf("file://") === 0) return path
    var parts = path.split("/")
    for (var i = 0; i < parts.length; i++)
      parts[i] = encodeURIComponent(parts[i])
    return "file://" + parts.join("/")
  }

  function urlToPath(url) {
    var value = String(url || "").trim()
    if (value.indexOf("file://") === 0)
      return decodeURIComponent(value.slice(7))
    return decodeURIComponent(value)
  }

  function normalizeUrl(value) {
    var raw = String(value || "").trim()
    if (!raw) return ""
    if (raw.indexOf("file://") === 0) return raw
    if (raw[0] === "/") return pathToUrl(raw)
    return raw
  }

  function uriList(url) {
    return url + "\r\n"
  }

  function gnomeCopiedFiles(url) {
    return "copy\n" + url + "\n"
  }

  function dragWithRipdrag(path) {
    if (!path) return
    Quickshell.execDetached([
      "sh", "-lc",
      "if command -v ripdrag >/dev/null 2>&1; then exec ripdrag -x -i -b \"$1\"; else exec \"$HOME/.nix-profile/bin/ripdrag\" -x -i -b \"$1\"; fi",
      "ripdrag", path
    ])
    if (shell) shell.triggerStatusPopup("󰇚", "Drag source", basename(path))
  }

  function containsUrl(url) {
    for (var i = 0; i < fileModel.count; i++) {
      if (fileModel.get(i).url === url) return true
    }
    return false
  }

  function updateCount() {
    if (shell) shell.fileBufferCount = fileModel.count
  }

  function saveStore() {
    var files = []
    for (var i = 0; i < fileModel.count; i++) {
      var item = fileModel.get(i)
      files.push({ url: item.url, path: item.path, name: item.name })
    }
    store.setText(JSON.stringify(files))
    updateCount()
  }

  function addUrl(value, saveAfter) {
    var url = normalizeUrl(value)
    if (!url || containsUrl(url)) return false
    var path = urlToPath(url)
    fileModel.append({ url: url, path: path, name: basename(path) })
    if (saveAfter) saveStore()
    else updateCount()
    return true
  }

  function addUrls(urls) {
    var added = 0
    for (var i = 0; i < urls.length; i++) {
      if (addUrl(urls[i], false)) added++
    }
    if (added > 0) {
      saveStore()
      if (shell) shell.triggerStatusPopup("󰈙", "Files buffered", added + " added")
    }
  }

  function addTextList(text) {
    var added = 0
    var lines = String(text || "").split(/\r?\n/)
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i].trim()
      if (!line || line[0] === "#") continue
      if (addUrl(line, false)) added++
    }
    if (added > 0) {
      saveStore()
      if (shell) shell.triggerStatusPopup("󰈙", "Files buffered", added + " added")
    }
  }

  function loadStore() {
    fileModel.clear()
    var raw = ""
    try {
      raw = store.text().trim()
    } catch(e) {
      updateCount()
      return
    }
    if (!raw) {
      updateCount()
      return
    }
    try {
      var files = JSON.parse(raw)
      if (!files || !files.length) {
        updateCount()
        return
      }
      for (var i = 0; i < files.length; i++) {
        var item = files[i]
        addUrl(item.url || item.path || item, false)
      }
    } catch(e) {
      console.log("file buffer parse:", e)
    }
    updateCount()
  }

  function removeAt(idx) {
    if (idx < 0 || idx >= fileModel.count) return
    fileModel.remove(idx)
    saveStore()
  }

  function clearFiles() {
    fileModel.clear()
    saveStore()
  }

  function copyPath(path) {
    Quickshell.clipboardText = path
    if (shell) shell.triggerStatusPopup("󰆏", "Copied path", basename(path))
  }

  function openFolder(path) {
    var dir = dirname(path)
    if (dir) Quickshell.execDetached(["xdg-open", dir])
  }

  Rectangle {
    id: panel
    width: bufferWin.panelW
    height: bufferWin.panelH
    radius: 18
    color: Theme.surface
    border.color: dropZone.containsDrag ? Theme.accent : Theme.border
    border.width: dropZone.containsDrag ? 2 : 1
    clip: true

    opacity: bufferWin.shell.fileBufferOpen ? 1.0 : 0.0
    scale: bufferWin.shell.fileBufferOpen ? 1.0 : 0.94
    transformOrigin: Item.TopLeft

    Behavior on opacity { NumberAnimation { duration: 170; easing.type: Easing.OutCubic } }
    Behavior on scale { NumberAnimation { duration: 260; easing.type: Easing.OutBack } }
    Behavior on border.color { ColorAnimation { duration: 120 } }

    Rectangle {
      anchors { fill: parent; margins: -3; topMargin: 6 }
      z: -1
      radius: parent.radius + 3
      color: Theme.shadow
    }

    HoverHandler {
      onHoveredChanged: {
        bufferWin.shell.fileBufferPanelHovered = hovered
        if (hovered) bufferWin.shell.showFileBuffer()
        else if (!bufferWin.shell.fileBufferDragging) bufferWin.shell.hideFileBufferSoon()
      }
    }

    DropArea {
      id: dropZone
      anchors.fill: parent
      onEntered: function(drag) {
        if (drag.hasUrls || drag.hasText) {
          bufferWin.shell.fileBufferPanelHovered = true
          bufferWin.shell.showFileBuffer()
          drag.acceptProposedAction()
        }
      }
      onExited: {
        bufferWin.shell.fileBufferPanelHovered = false
        if (!bufferWin.shell.fileBufferIconHovered) bufferWin.shell.hideFileBufferSoon()
      }
      onDropped: function(drop) {
        if (drop.hasUrls) bufferWin.addUrls(drop.urls)
        else if (drop.hasText) bufferWin.addTextList(drop.text)
        drop.acceptProposedAction()
      }
    }

    Column {
      anchors { fill: parent; margins: 12 }
      spacing: 10

      Row {
        width: parent.width
        height: 34
        spacing: 8

        Rectangle {
          width: 34; height: 34; radius: 11
          color: Theme.alpha(Theme.accent, 0.14)
          border.color: Theme.alpha(Theme.accent, 0.45)
          Text {
            anchors.centerIn: parent
            text: "󰈙"
            color: Theme.accent
            font.pixelSize: 17
          }
        }

        Column {
          width: parent.width - 34 - 8 - actionRow.width
          anchors.verticalCenter: parent.verticalCenter
          spacing: 1
          Text {
            width: parent.width
            text: fileModel.count === 1 ? "1 file" : fileModel.count + " files"
            color: Theme.fg
            font.pixelSize: 13
            font.weight: Font.DemiBold
            elide: Text.ElideRight
          }
          Text {
            width: parent.width
            text: "Portable shelf"
            color: Theme.fgDim
            font.pixelSize: 10
            elide: Text.ElideRight
          }
        }

        Row {
          id: actionRow
          height: 28
          anchors.verticalCenter: parent.verticalCenter
          spacing: 6

          MiniButton {
            icon: "+"
            accent: Theme.accent
            onClicked: picker.open()
          }

          MiniButton {
            icon: "󰆴"
            accent: Theme.red
            enabled: fileModel.count > 0
            opacity: enabled ? 1.0 : 0.35
            onClicked: bufferWin.clearFiles()
          }
        }
      }

      Rectangle {
        width: parent.width
        height: 74
        radius: 14
        color: dropZone.containsDrag
          ? Theme.alpha(Theme.accent, 0.18)
          : Theme.alpha(Theme.bg, 0.65)
        border.color: dropZone.containsDrag ? Theme.accent : Theme.border
        border.width: 1

        Behavior on color { ColorAnimation { duration: 120 } }
        Behavior on border.color { ColorAnimation { duration: 120 } }

        Column {
          anchors.centerIn: parent
          spacing: 4
          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: dropZone.containsDrag ? "󰇚" : "󰉋"
            color: dropZone.containsDrag ? Theme.accent : Theme.fgMuted
            font.pixelSize: 20
          }
          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: dropZone.containsDrag ? "Release" : "Drop files"
            color: dropZone.containsDrag ? Theme.accent : Theme.fgMuted
            font.pixelSize: 12
            font.weight: Font.Medium
          }
        }
      }

      Item {
        width: parent.width
        height: parent.height - y
        clip: true

        ListView {
          id: fileList
          anchors.fill: parent
          model: fileModel
          spacing: 6
          clip: true

          delegate: Item {
            id: fileDelegate
            required property int index
            required property string url
            required property string path
            required property string name

            width: fileList.width
            height: 42

            Rectangle {
              id: rowCard
              width: parent.width
              height: parent.height
              x: 0
              y: 0
              radius: 12
              color: rowMouse.containsMouse
                ? Theme.alpha(Theme.accent, 0.11)
                : Theme.card
              border.color: rowMouse.containsMouse ? Theme.alpha(Theme.accent, 0.55) : Theme.border
              border.width: 1

              Behavior on color { ColorAnimation { duration: 110 } }
              Behavior on border.color { ColorAnimation { duration: 110 } }

              Drag.active: rowMouse.drag.active
              Drag.source: rowCard
              Drag.dragType: Drag.Automatic
              Drag.supportedActions: Qt.CopyAction
              Drag.proposedAction: Qt.CopyAction
              Drag.keys: ["text/uri-list"]
              Drag.mimeData: ({
                "text/uri-list": bufferWin.uriList(fileDelegate.url),
                "application/x-kde-urilist": bufferWin.uriList(fileDelegate.url),
                "text/plain": fileDelegate.path,
                "x-special/gnome-copied-files": bufferWin.gnomeCopiedFiles(fileDelegate.url)
              })
              Drag.hotSpot.x: rowMouse.mouseX
              Drag.hotSpot.y: rowMouse.mouseY
              Drag.onDragFinished: {
                rowCard.x = 0
                rowCard.y = 0
                bufferWin.shell.fileBufferDragging = false
                if (!bufferWin.shell.fileBufferIconHovered && !bufferWin.shell.fileBufferPanelHovered)
                  bufferWin.shell.hideFileBufferSoon()
              }

              MouseArea {
                id: rowMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                preventStealing: true
                drag.target: rowCard
                drag.threshold: 8
                onPressed: bufferWin.shell.fileBufferDragging = true
                onClicked: bufferWin.copyPath(fileDelegate.path)
                onPressAndHold: bufferWin.dragWithRipdrag(fileDelegate.path)
                onReleased: {
                  rowCard.x = 0
                  rowCard.y = 0
                  bufferWin.shell.fileBufferDragging = false
                  if (!bufferWin.shell.fileBufferIconHovered && !bufferWin.shell.fileBufferPanelHovered)
                    bufferWin.shell.hideFileBufferSoon()
                }
              }

              Row {
                anchors { fill: parent; leftMargin: 10; rightMargin: 8 }
                spacing: 8

                Text {
                  text: "󰈙"
                  color: Theme.accent
                  font.pixelSize: 14
                  anchors.verticalCenter: parent.verticalCenter
                }

                Column {
                  width: parent.width - 14 - 8 - rowActions.width - 8
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: 1
                  Text {
                    width: parent.width
                    text: fileDelegate.name
                    color: Theme.fg
                    font.pixelSize: 12
                    font.weight: Font.Medium
                    elide: Text.ElideMiddle
                    maximumLineCount: 1
                  }
                  Text {
                    width: parent.width
                    text: fileDelegate.path
                    color: Theme.fgDim
                    font.pixelSize: 9
                    elide: Text.ElideMiddle
                    maximumLineCount: 1
                  }
                }

                Row {
                  id: rowActions
                  spacing: 4
                  anchors.verticalCenter: parent.verticalCenter

                  MiniButton {
                    icon: "󰇚"
                    accent: Theme.accent
                    onClicked: bufferWin.dragWithRipdrag(fileDelegate.path)
                  }

                  MiniButton {
                    icon: "󰆏"
                    accent: Theme.blue
                    onClicked: bufferWin.copyPath(fileDelegate.path)
                  }

                  MiniButton {
                    icon: "󰉋"
                    accent: Theme.yellow
                    onClicked: bufferWin.openFolder(fileDelegate.path)
                  }

                  MiniButton {
                    icon: "×"
                    accent: Theme.red
                    onClicked: bufferWin.removeAt(fileDelegate.index)
                  }
                }
              }
            }
          }
        }

        Text {
          anchors.centerIn: parent
          visible: fileModel.count === 0
          text: "No files buffered"
          color: Theme.fgDim
          font.pixelSize: 12
        }
      }
    }
  }

  component MiniButton: Rectangle {
    id: mini
    property string icon: ""
    property color accent: Theme.accent
    signal clicked()

    width: 28
    height: 28
    radius: 9
    color: btnHover.hovered ? Theme.alpha(mini.accent, 0.18) : Theme.alpha(Theme.fg, 0.04)
    border.color: btnHover.hovered ? mini.accent : Theme.border
    border.width: 1

    Behavior on color { ColorAnimation { duration: 110 } }
    Behavior on border.color { ColorAnimation { duration: 110 } }

    Text {
      anchors.centerIn: parent
      text: mini.icon
      color: mini.enabled ? mini.accent : Theme.fgDim
      font.pixelSize: 13
      font.weight: Font.DemiBold
    }

    HoverHandler {
      id: btnHover
      enabled: mini.enabled
      cursorShape: Qt.PointingHandCursor
    }

    TapHandler {
      enabled: mini.enabled
      onTapped: mini.clicked()
    }
  }
}
