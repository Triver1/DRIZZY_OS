import QtQuick
import Quickshell

Item {
  id: root
  property var shell

  property var allApps: DesktopEntries.applications

  function fuzzyScore(text, query) {
    var ti = 0, qi = 0, score = 0
    text = text.toLowerCase()
    query = query.toLowerCase()
    while (ti < text.length && qi < query.length) {
      if (text[ti] === query[qi]) { score += 10 - qi; qi++ }
      ti++
    }
    return qi === query.length ? score : 0
  }

  function searchApps(query) {
    if (!query) { appGrid.model = DesktopEntries.applications; return }
    var q = query.toLowerCase()
    var apps = allApps.values || []
    var scored = []
    for (var i = 0; i < apps.length; i++) {
      var a = apps[i]
      if (!a) continue
      var s1 = fuzzyScore(a.name || "", q) * 3
      var s2 = fuzzyScore(a.comment || "", q)
      var s3 = fuzzyScore(a.keywords || "", q) * 2
      var total = s1 + s2 + s3
      if (total > 0) scored.push({ app: a, score: total })
    }
    scored.sort(function(x, y) { return y.score - x.score })
    var results = []
    for (var j = 0; j < scored.length; j++) results.push(scored[j].app)
    appGrid.model = results
  }

  Column {
    anchors.fill: parent
    anchors.margins: 20
    spacing: 12

    Text {
      text: "Applications"
      color: "#9DA1AF"
      font.pixelSize: 13
      font.weight: Font.Bold
      font.letterSpacing: 0.8
    }

    Rectangle {
      width: parent.width; height: 44; radius: 22
      color: "#FF2C2F3E"
      border.color: searchInput.activeFocus ? "#B4E49A" : "#3D4052"
      border.width: 1
      Behavior on border.color { ColorAnimation { duration: 150 } }

      Row {
        anchors.fill: parent; anchors.leftMargin: 18; anchors.rightMargin: 18; spacing: 10
        Text {
          text: "󰍉"
          color: "#B4E49A"
          font.pixelSize: 16
          anchors.verticalCenter: parent.verticalCenter
        }
        TextInput {
          id: searchInput
          anchors.verticalCenter: parent.verticalCenter
          width: parent.width - 30
          color: "#E2E2E9"; font.pixelSize: 13; font.weight: Font.Medium
          selectionColor: "#33B4E49A"; selectedTextColor: "#B4E49A"
          clip: true

          Text {
            anchors.fill: parent; text: "Search apps..."
            color: "#6B6E7E"; font.pixelSize: 13
            verticalAlignment: Text.AlignVCenter
            visible: !searchInput.text && !searchInput.activeFocus
          }

          onTextChanged: root.searchApps(text)
          onAccepted: {
            var model = appGrid.model
            if (model && model.length > 0) {
              shell.launchApp(model[0])
              searchInput.text = ""
            }
          }
        }
      }
    }

    GridView {
      id: appGrid
      width: parent.width; height: parent.height - 70
      cellWidth: 92; cellHeight: 100
      clip: true

      delegate: Rectangle {
        id: appDelegate
        width: 84; height: 92; radius: 16
        color: appHover.hovered ? "#1AB4E49A" : "transparent"
        border.color: appHover.hovered ? "#B4E49A" : "transparent"
        border.width: 1
        Behavior on color { ColorAnimation { duration: 120 } }

        property var entry: typeof modelData !== "undefined" ? modelData : null

        Column {
          anchors.centerIn: parent; spacing: 6
          Image {
            anchors.horizontalCenter: parent.horizontalCenter
            width: 48; height: 48
            source: appDelegate.entry && appDelegate.entry.icon ? Quickshell.iconPath(appDelegate.entry.icon) : ""
            sourceSize.width: 48; sourceSize.height: 48
            asynchronous: true; fillMode: Image.PreserveAspectFit; smooth: true
          }
          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: appDelegate.entry && appDelegate.entry.name ? appDelegate.entry.name : ""
            color: "#E2E2E9"
            font.pixelSize: 10; font.weight: Font.Medium
            elide: Text.ElideRight; width: 76
            horizontalAlignment: Text.AlignHCenter; maximumLineCount: 1
          }
        }

        HoverHandler { id: appHover; cursorShape: Qt.PointingHandCursor }
        TapHandler {
          onTapped: {
            if (appDelegate.entry) {
              shell.launchApp(appDelegate.entry)
              searchInput.text = ""
            }
          }
        }
      }
    }
  }
}
