import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import "../../theme"

Item {
  id: root

  property real cpuUsage:   0
  property real memUsed:    0
  property real memTotal:   1
  property real swapUsed:   0
  property real swapTotal:  0
  property real diskUsed:   0
  property real diskTotal:  1
  property real cpuTemp:    0
  property var  processes:  []

  // Previous CPU stat snapshot for delta calculation
  property var  prevCpu: null

  function refresh() {
    cpuProc.running    = true
    memProc.running    = true
    diskProc.running   = true
    tempProc.running   = true
    psProc.running     = true
  }

  onVisibleChanged: if (visible) { refresh(); refreshTimer.running = visible }
  Component.onCompleted: refresh()

  Timer {
    id: refreshTimer
    interval: 2000; repeat: true
    onTriggered: { cpuProc.running = true; memProc.running = true; psProc.running = true }
  }

  Process {
    id: cpuProc; running: false
    command: ["sh", "-c", "cat /proc/stat | head -1"]
    stdout: StdioCollector {
      onStreamFinished: {
        var parts = text.trim().split(/\s+/)
        if (parts.length < 8) return
        var user = parseInt(parts[1]); var nice = parseInt(parts[2])
        var sys  = parseInt(parts[3]); var idle = parseInt(parts[4])
        var iowait = parseInt(parts[5]); var irq = parseInt(parts[6]); var sirq = parseInt(parts[7])
        var total  = user + nice + sys + idle + iowait + irq + sirq
        var active = total - idle - iowait
        if (root.prevCpu !== null) {
          var dt = total - root.prevCpu.total
          var da = active - root.prevCpu.active
          if (dt > 0) root.cpuUsage = Math.max(0, Math.min(1, da / dt))
        }
        root.prevCpu = { total: total, active: active }
      }
    }
  }

  Process {
    id: memProc; running: false
    command: ["sh", "-c", "grep -E '^(MemTotal|MemAvailable|SwapTotal|SwapFree):' /proc/meminfo"]
    stdout: StdioCollector {
      onStreamFinished: {
        var lines = text.trim().split("\n")
        var vals = {}
        for (var i = 0; i < lines.length; i++) {
          var parts = lines[i].trim().split(/\s+/)
          if (parts.length >= 2) vals[parts[0].replace(":", "")] = parseInt(parts[1])
        }
        var mt = vals["MemTotal"]   || 1
        var ma = vals["MemAvailable"] || 0
        var st = vals["SwapTotal"]  || 0
        var sf = vals["SwapFree"]   || 0
        root.memTotal  = mt / 1024 / 1024
        root.memUsed   = (mt - ma) / 1024 / 1024
        root.swapTotal = st / 1024 / 1024
        root.swapUsed  = (st - sf) / 1024 / 1024
      }
    }
  }

  Process {
    id: diskProc; running: false
    command: ["sh", "-c", "df -k / | tail -1"]
    stdout: StdioCollector {
      onStreamFinished: {
        var parts = text.trim().split(/\s+/)
        if (parts.length >= 3) {
          root.diskTotal = parseInt(parts[1]) / 1024 / 1024
          root.diskUsed  = parseInt(parts[2]) / 1024 / 1024
        }
      }
    }
  }

  Process {
    id: tempProc; running: false
    command: ["sh", "-c", "cat /sys/class/thermal/thermal_zone0/temp 2>/dev/null || echo 0"]
    stdout: StdioCollector {
      onStreamFinished: { root.cpuTemp = parseInt(text.trim()) / 1000 }
    }
  }

  Process {
    id: psProc; running: false
    command: ["sh", "-c", "ps aux --sort=-%cpu | awk 'NR>1{print $11\"|\"$3\"|\"$4}' | head -8"]
    stdout: StdioCollector {
      onStreamFinished: {
        var procs = []
        var lines = text.trim().split("\n")
        for (var i = 0; i < lines.length; i++) {
          var parts = lines[i].split("|")
          if (parts.length >= 3) {
            var name = parts[0].split("/").pop().substring(0, 28)
            procs.push({ name: name, cpu: parseFloat(parts[1]), mem: parseFloat(parts[2]) })
          }
        }
        root.processes = procs
      }
    }
  }

  Flickable {
    anchors.fill: parent
    contentWidth: width
    contentHeight: col.implicitHeight + 32
    clip: true

    Column {
      id: col
      width: parent.width; spacing: 0

      Item { width: 1; height: 24 }

      // ── Overview gauges ───────────────────────────────────────────────────

      SectionHeader { label: "Resources" }

      Row {
        width: parent.width; spacing: 8

        UsageGauge {
          icon: "󰻠"; label: "CPU"
          value: root.cpuUsage
          sublabel: Math.round(root.cpuUsage * 100) + "%"
            + (root.cpuTemp > 0 ? "  " + Math.round(root.cpuTemp) + "°C" : "")
          gaugeColor: root.cpuUsage > 0.85 ? Theme.red
                    : root.cpuUsage > 0.6  ? Theme.yellow : Theme.accent
        }

        UsageGauge {
          icon: "󰍛"; label: "Memory"
          value: root.memTotal > 0 ? root.memUsed / root.memTotal : 0
          sublabel: root.memUsed.toFixed(1) + " / " + root.memTotal.toFixed(1) + " GB"
          gaugeColor: (root.memUsed / root.memTotal) > 0.85 ? Theme.red
                    : (root.memUsed / root.memTotal) > 0.65 ? Theme.yellow : Theme.blue
        }

        UsageGauge {
          icon: "󰋊"; label: "Disk"
          value: root.diskTotal > 0 ? root.diskUsed / root.diskTotal : 0
          sublabel: root.diskUsed.toFixed(0) + " / " + root.diskTotal.toFixed(0) + " GB"
          gaugeColor: (root.diskUsed / root.diskTotal) > 0.85 ? Theme.red
                    : (root.diskUsed / root.diskTotal) > 0.65 ? Theme.yellow : Theme.purple
        }
      }

      Item { width: 1; height: 24 }

      // ── Detailed stats ────────────────────────────────────────────────────

      SectionHeader { label: "Details" }

      OutlineCard {
        width: parent.width

        Column {
          width: parent.width; spacing: 14

          StatRow { label: "CPU Usage";      value: Math.round(root.cpuUsage * 100) + "%" }
          StatRow { label: "CPU Temp";       value: root.cpuTemp > 0 ? Math.round(root.cpuTemp) + "°C" : "N/A" }
          StatRow { label: "RAM Used";       value: root.memUsed.toFixed(2) + " GB" }
          StatRow { label: "RAM Total";      value: root.memTotal.toFixed(2) + " GB" }
          StatRow {
            visible: root.swapTotal > 0
            label: "Swap";
            value: root.swapUsed.toFixed(2) + " / " + root.swapTotal.toFixed(2) + " GB"
          }
          StatRow { label: "Disk Used";  value: root.diskUsed.toFixed(1)  + " GB" }
          StatRow { label: "Disk Total"; value: root.diskTotal.toFixed(1) + " GB" }
        }
      }

      Item { width: 1; height: 24 }

      // ── Processes ─────────────────────────────────────────────────────────

      SectionHeader { label: "Top Processes (by CPU)" }

      OutlineCard {
        width: parent.width

        Column {
          width: parent.width; spacing: 0

          // Header
          RowLayout {
            width: parent.width; height: 28

            Text { text: "Process"; Layout.fillWidth: true; color: Theme.fgDim; font.pixelSize: 10; font.weight: Font.Bold }
            Text { text: "CPU%";   width: 48; color: Theme.fgDim; font.pixelSize: 10; font.weight: Font.Bold; horizontalAlignment: Text.AlignRight }
            Text { text: "MEM%";   width: 48; color: Theme.fgDim; font.pixelSize: 10; font.weight: Font.Bold; horizontalAlignment: Text.AlignRight }
          }

          Rectangle { width: parent.width; height: 1; color: Theme.border }
          Item { width: 1; height: 6 }

          Repeater {
            model: root.processes

            RowLayout {
              width: parent.width; height: 32

              Rectangle {
                width: 4; height: 20; radius: 2
                color: modelData.cpu > 50 ? Theme.red
                     : modelData.cpu > 20 ? Theme.yellow : Theme.accent
              }

              Text {
                Layout.fillWidth: true
                text: modelData.name; color: Theme.fg; font.pixelSize: 11
                elide: Text.ElideRight
              }

              Text {
                width: 48; text: modelData.cpu.toFixed(1)
                color: modelData.cpu > 50 ? Theme.red
                     : modelData.cpu > 20 ? Theme.yellow : Theme.fgMuted
                font.pixelSize: 11; horizontalAlignment: Text.AlignRight
              }
              Text {
                width: 48; text: modelData.mem.toFixed(1)
                color: modelData.mem > 20 ? Theme.yellow : Theme.fgMuted
                font.pixelSize: 11; horizontalAlignment: Text.AlignRight
              }
            }
          }
        }
      }

      Item { width: 1; height: 24 }
    }
  }

  component SectionHeader: Item {
    property string label: ""
    width: parent.width; height: 32
    Text {
      x: 0; anchors.verticalCenter: parent.verticalCenter
      text: label.toUpperCase()
      color: Theme.fgDim; font.pixelSize: 10; font.weight: Font.Bold; font.letterSpacing: 1.5
    }
  }

  component OutlineCard: Rectangle {
    default property alias content: inn.data
    width: parent.width
    height: inn.implicitHeight + 32
    radius: 14; color: Theme.card; border.color: Theme.border; border.width: 1
    Column { id: inn; anchors { fill: parent; margins: 16 }
      spacing: 0 }
  }

  component UsageGauge: Rectangle {
    property string icon: ""
    property string label: ""
    property real   value: 0
    property string sublabel: ""
    property color  gaugeColor: Theme.accent

    Layout.fillWidth: true
    width: (parent.width - 16) / 3
    height: 112; radius: 14
    color: Theme.card; border.color: Theme.border; border.width: 1

    Column {
      anchors { fill: parent; margins: 14 }
      spacing: 8

      RowLayout {
        width: parent.width
        Text { text: icon; color: parent.parent.gaugeColor; font.pixelSize: 18 }
        Text { text: parent.parent.label; color: Theme.fgMuted; font.pixelSize: 11; Layout.fillWidth: true }
      }

      // Arc-style bar (horizontal)
      Item {
        width: parent.width; height: 8

        Rectangle { anchors.fill: parent; radius: 4; color: Theme.bg }
        Rectangle {
          width: parent.width * Math.max(0, Math.min(1, parent.parent.parent.value))
          height: 8; radius: 4; color: parent.parent.parent.gaugeColor
          Behavior on width { NumberAnimation { duration: 300; easing.type: Easing.OutCubic } }
        }
      }

      Text {
        width: parent.width
        text: sublabel; color: Theme.fgMuted; font.pixelSize: 10
        horizontalAlignment: Text.AlignRight
      }
    }
  }

  component StatRow: RowLayout {
    property string label: ""
    property string value: ""
    width: parent.width; height: 28

    Text { text: label; color: Theme.fgMuted; font.pixelSize: 12; Layout.fillWidth: true }
    Text { text: value; color: Theme.fg;      font.pixelSize: 12; font.weight: Font.DemiBold }
  }
}
