import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import "../../theme"

Item {
  id: root

  // 0=work, 1=short break, 2=long break, 3=custom
  property int mode: 0
  readonly property var modeBaseTotals: [25 * 60, 5 * 60, 15 * 60]
  readonly property var modeLabels: ["Work", "Short Break", "Long Break", "Custom"]
  readonly property var modeColors: [Theme.accent, Theme.blue, Theme.purple, Theme.orange]

  property int customMinutes: 60
  property int totalTime: mode < 3 ? modeBaseTotals[mode] : customMinutes * 60
  property int timeLeft:  totalTime
  property bool running:  false

  // ── Persistence ───────────────────────────────────────────────────────────

  property string homePath: ""
  property bool stateLoaded: false

  Process {
    running: true
    command: ["sh", "-c", "printf '%s' \"$HOME\""]
    stdout: StdioCollector {
      onStreamFinished: {
        root.homePath = this.text.trim()
        timerLoader.command = ["sh", "-c",
          "cat \"" + root.homePath + "/.quickshell-timer.json\" 2>/dev/null || echo 'null'"]
        timerLoader.running = true
      }
    }
  }

  Process {
    id: timerLoader
    running: false
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          var s = JSON.parse(this.text.trim())
          if (s) {
            if (s.customMinutes !== undefined) root.customMinutes = s.customMinutes
            if (s.mode !== undefined) root.mode = s.mode
          }
        } catch(e) {}
        root.stateLoaded = true
      }
    }
  }

  Process { id: timerSaver; running: false }

  function saveState() {
    if (!stateLoaded || !homePath) return
    timerSaver.command = ["python3", "-c",
      "import sys; open(sys.argv[1],'w',encoding='utf-8').write(sys.argv[2])",
      homePath + "/.quickshell-timer.json",
      JSON.stringify({ mode: root.mode, customMinutes: root.customMinutes })]
    if (!timerSaver.running) timerSaver.running = true
  }

  // ── Timer logic ───────────────────────────────────────────────────────────

  Timer {
    id: tick
    interval: 1000; repeat: true; running: root.running
    onTriggered: {
      if (root.timeLeft > 0) {
        root.timeLeft--
        ring.requestPaint()
      } else {
        root.running = false
        ring.requestPaint()
      }
    }
  }

  function resetTimer() {
    running = false
    timeLeft = totalTime
    ring.requestPaint()
  }

  onModeChanged: {
    resetTimer()
    saveState()
  }

  onCustomMinutesChanged: {
    if (mode === 3 && !running) {
      timeLeft = totalTime
      ring.requestPaint()
    }
    saveState()
  }

  function fmt(secs) {
    var m = Math.floor(secs / 60)
    var s = secs % 60
    return (m < 10 ? "0" : "") + m + ":" + (s < 10 ? "0" : "") + s
  }

  // ── UI ────────────────────────────────────────────────────────────────────

  ColumnLayout {
    anchors.centerIn: parent
    spacing: 16

    // Mode selector
    Row {
      Layout.alignment: Qt.AlignHCenter
      spacing: 6

      Repeater {
        model: root.modeLabels
        Rectangle {
          property bool active: root.mode === index
          width: modeText.implicitWidth + 20; height: 26; radius: 13
          color: "transparent"
          border.color: active ? root.modeColors[index] : Theme.border
          border.width: 1
          Behavior on border.color { ColorAnimation { duration: 150 } }

          Text {
            id: modeText
            anchors.centerIn: parent
            text: modelData
            color: active ? root.modeColors[index] : Theme.fgMuted
            font.pixelSize: 11
            font.weight: active ? Font.DemiBold : Font.Medium
            Behavior on color { ColorAnimation { duration: 150 } }
          }
          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler { onTapped: root.mode = index }
        }
      }
    }

    // Custom duration adjuster (only in custom mode, only when stopped)
    Column {
      Layout.alignment: Qt.AlignHCenter
      spacing: 6
      visible: root.mode === 3 && !root.running

      Row {
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: 6

        Repeater {
          model: [
            { label: "-15", delta: -15 },
            { label: "-5",  delta:  -5 },
            { label: "-1",  delta:  -1 },
            { label: "+1",  delta:   1 },
            { label: "+5",  delta:   5 },
            { label: "+15", delta:  15 }
          ]

          Rectangle {
            width: adjLabel.implicitWidth + 14; height: 24; radius: 12
            color: "transparent"
            border.color: adjHov.hovered ? Theme.orange : Theme.border
            border.width: 1
            Behavior on border.color { ColorAnimation { duration: 120 } }

            Text {
              id: adjLabel
              anchors.centerIn: parent
              text: modelData.label
              color: adjHov.hovered ? Theme.orange : Theme.fgMuted
              font.pixelSize: 10; font.weight: Font.DemiBold
              Behavior on color { ColorAnimation { duration: 120 } }
            }
            HoverHandler { id: adjHov; cursorShape: Qt.PointingHandCursor }
            TapHandler {
              onTapped: root.customMinutes = Math.min(180, Math.max(1, root.customMinutes + modelData.delta))
            }
          }
        }
      }

      Text {
        anchors.horizontalCenter: parent.horizontalCenter
        text: root.customMinutes + " min"
        color: Theme.orange; font.pixelSize: 11; font.weight: Font.DemiBold
      }
    }

    // Ring + time display
    Item {
      Layout.alignment: Qt.AlignHCenter
      width: 200; height: 200

      Canvas {
        id: ring
        anchors.fill: parent

        onPaint: {
          var ctx = getContext("2d")
          ctx.clearRect(0, 0, width, height)
          var cx = width / 2, cy = height / 2, r = 82

          ctx.beginPath()
          ctx.arc(cx, cy, r, 0, Math.PI * 2)
          ctx.strokeStyle = Theme.border.toString()
          ctx.lineWidth = 10; ctx.lineCap = "butt"
          ctx.stroke()

          var progress = root.totalTime > 0 ? root.timeLeft / root.totalTime : 0
          if (progress > 0) {
            ctx.beginPath()
            ctx.arc(cx, cy, r, -Math.PI / 2, -Math.PI / 2 + progress * Math.PI * 2)
            ctx.strokeStyle = root.modeColors[root.mode].toString()
            ctx.lineWidth = 10; ctx.lineCap = "round"
            ctx.stroke()
          }
        }
      }

      Column {
        anchors.centerIn: parent
        spacing: 4

        Text {
          anchors.horizontalCenter: parent.horizontalCenter
          text: root.fmt(root.timeLeft)
          color: Theme.fg; font.pixelSize: 36; font.weight: Font.DemiBold
          font.family: "monospace"
        }
        Text {
          anchors.horizontalCenter: parent.horizontalCenter
          text: root.modeLabels[root.mode]
          color: root.modeColors[root.mode]; font.pixelSize: 11; font.weight: Font.Medium
        }
      }
    }

    // Controls
    Row {
      Layout.alignment: Qt.AlignHCenter
      spacing: 12

      Rectangle {
        width: 40; height: 40; radius: 20
        color: "transparent"
        border.color: resetHov.hovered ? Theme.fgMuted : Theme.border; border.width: 1
        Behavior on border.color { ColorAnimation { duration: 150 } }
        Text { anchors.centerIn: parent; text: "󰑙"; color: Theme.fgMuted; font.pixelSize: 16 }
        HoverHandler { id: resetHov; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: root.resetTimer() }
      }

      Rectangle {
        width: 56; height: 56; radius: 28
        color: "transparent"
        border.color: root.modeColors[root.mode]; border.width: 2
        Behavior on border.color { ColorAnimation { duration: 150 } }
        Text {
          anchors.centerIn: parent
          text: root.running ? "󰏤" : "󰐊"
          color: root.modeColors[root.mode]; font.pixelSize: 22
        }
        HoverHandler { id: playHov; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: root.running = !root.running }
      }

      Rectangle {
        width: 40; height: 40; radius: 20
        color: "transparent"
        border.color: skipHov.hovered ? Theme.fgMuted : Theme.border; border.width: 1
        Behavior on border.color { ColorAnimation { duration: 150 } }
        Text { anchors.centerIn: parent; text: "󰒭"; color: Theme.fgMuted; font.pixelSize: 16 }
        HoverHandler { id: skipHov; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: root.mode = (root.mode + 1) % 4 }
      }
    }
  }
}
