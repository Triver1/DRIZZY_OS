import Quickshell
import Quickshell.Io
import QtQuick
import Niri 0.1
import Quickshell.Services.Notifications
import Quickshell.Services.Pipewire
import "windows"
import "theme"

Scope {
  id: shell
  required property var modelData

  property bool dockOpen: false
  property string dockPage: "dashboard"
  property bool launcherOpen:  false
  property bool settingsOpen:  false
  property bool fileBufferOpen: false
  property bool fileBufferIconHovered: false
  property bool fileBufferPanelHovered: false
  property bool fileBufferDragging: false
  property int fileBufferCount: 0
  property var fileBufferDropUrls: []
  property string fileBufferDropText: ""
  property int fileBufferDropSerial: 0

  property string wallpaperPath: ""
  property real volumeLevel: 0.7
  property bool showVolumePopup: false

  property int batteryPercent: 100
  property string batteryStatus: "Unknown"
  property bool batteryAvailable: false
  property string lastBatteryStatus: "Unknown"
  property var notifiedBatteryThresholds: ({})
  property bool showBrightnessPopup: false
  property real lastBrightness: 0.6

  PwObjectTracker {
    objects: [
      Pipewire.defaultAudioSink,
      Pipewire.defaultAudioSource
    ]
  }

  NotificationServer {
    id: notifServer
    bodySupported: true
    actionsSupported: true
  }

  Timer {
    id: volumePopupTimer
    interval: 2000
    onTriggered: showVolumePopup = false
  }

  function triggerVolumePopup() {
    showVolumePopup = true
    volumePopupTimer.restart()
  }

  Timer {
    id: brightnessPopupTimer
    interval: 2000
    onTriggered: showBrightnessPopup = false
  }

  function triggerBrightnessPopup() {
    showBrightnessPopup = true
    brightnessPopupTimer.restart()
  }

  function triggerStatusPopup(icon, title, subtitle) {
    if (statusPopup) statusPopup.show(icon, title, subtitle)
  }

  Timer {
    id: fileBufferCloseTimer
    interval: 180
    onTriggered: {
      if (!shell.fileBufferIconHovered && !shell.fileBufferPanelHovered && !shell.fileBufferDragging)
        shell.fileBufferOpen = false
    }
  }

  function showFileBuffer() {
    fileBufferCloseTimer.stop()
    fileBufferOpen = true
  }

  function hideFileBufferSoon() {
    fileBufferCloseTimer.restart()
  }

  function submitFileBufferDrop(urls, text) {
    fileBufferDropUrls = urls || []
    fileBufferDropText = text || ""
    fileBufferDropSerial += 1
    showFileBuffer()
  }

  readonly property real sinkVolume: {
    var sink = Pipewire.defaultAudioSink
    if (!sink || !sink.audio) return -1
    return sink.audio.volume
  }

  property bool volInitialized: false

  onSinkVolumeChanged: {
    if (sinkVolume < 0) return
    if (!volInitialized) { volInitialized = true; return }
    var pct = Math.round(sinkVolume * 100)
    shell.volumeLevel = sinkVolume
    triggerStatusPopup("󰕾", "Volume", pct + "%")
    triggerVolumePopup()
  }

  function checkBatteryNotifications() {
    if (batteryStatus === "Charging" || batteryStatus === "Full" || batteryStatus === "Unknown") {
      notifiedBatteryThresholds = ({})
      return
    }
    var thresholds = [20, 10, 5]
    for (var i = 0; i < thresholds.length; i++) {
      var t = thresholds[i]
      if (batteryPercent <= t && !notifiedBatteryThresholds[t]) {
        notifiedBatteryThresholds[t] = true
        if (notifOverlay) {
          notifOverlay.showNotification("Low Battery", "Battery is at " + batteryPercent + "%", "", [])
        }
      }
    }
  }

  Timer {
    id: batteryTimer
    interval: 30000; running: true; repeat: true
    onTriggered: batteryReader.running = true
    Component.onCompleted: batteryReader.running = true
  }

  Process {
    id: batteryReader
    running: false
    command: ["sh", "-c", "for b in /sys/class/power_supply/BAT*; do if [ -f \"$b/capacity\" ]; then echo \"$(cat \"$b/capacity\")|$(cat \"$b/status\" 2>/dev/null || echo Unknown)\"; break; fi; done"]
    stdout: StdioCollector {
      onStreamFinished: {
        var out = this.text.trim()
        if (out.length > 0 && out.indexOf("|") >= 0) {
          var parts = out.split("|")
          var cap = parseInt(parts[0])
          if (isNaN(cap)) cap = 100
          var newStatus = parts[1].trim()
          if (shell.batteryAvailable && shell.lastBatteryStatus !== newStatus) {
            if (newStatus === "Charging" && shell.lastBatteryStatus !== "Unknown")
              shell.triggerStatusPopup("󰂅", "Charging", Math.round(cap) + "%")
            else if (newStatus === "Discharging" && (shell.lastBatteryStatus === "Charging" || shell.lastBatteryStatus === "Full"))
              shell.triggerStatusPopup("󰁹", "On Battery", Math.round(cap) + "%")
          }
          shell.batteryPercent = cap
          shell.batteryStatus = newStatus
          shell.lastBatteryStatus = newStatus
          shell.batteryAvailable = true
          shell.checkBatteryNotifications()
        }
      }
    }
  }

  function launchApp(entry) {
    if (entry) entry.execute()
    shell.dockOpen = false
    shell.launcherOpen = false
  }

  Niri {
    id: niri
    Component.onCompleted: connect()
    onErrorOccurred: function(err) { console.log("niri:", err) }
    workspaces.maxCount: 10
  }

  // Load saved theme on startup
  Process {
    running: true
    command: ["sh", "-c", "cat \"$HOME/.quickshell-theme.json\" 2>/dev/null || echo 'null'"]
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          var s = JSON.parse(this.text.trim())
          if (s && s.themeIndex !== undefined) Theme.applyTheme(s.themeIndex)
        } catch(e) {}
      }
    }
  }

  // Matugen: run whenever wallpaper changes while "Wallpaper" theme is active
  function maybeRunMatugen() {
    if (Theme.currentTheme !== 3 || !shell.wallpaperPath) return
    var localPath = decodeURIComponent(shell.wallpaperPath.replace("file://", ""))
    matugenProc.command = ["sh", "-c",
      "matugen image --json hex \"" + localPath + "\" 2>/dev/null || " +
      "matugen image -j hex \"" + localPath + "\" 2>/dev/null"]
    if (!matugenProc.running) matugenProc.running = true
  }

  onWallpaperPathChanged: maybeRunMatugen()

  Process {
    id: matugenProc
    running: false
    stdout: StdioCollector {
      onStreamFinished: {
        try {
          var raw = JSON.parse(this.text.trim())
          Theme.applyMatugen(raw)
        } catch(e) { console.log("matugen parse:", e) }
      }
    }
  }

  Connections {
    target: Theme
    function onCurrentThemeChanged() {
      if (Theme.currentTheme === 3) shell.maybeRunMatugen()
    }
  }

  Timer {
    id: stopwatchTimer
    interval: 1000; running: false; repeat: true
    property int elapsed: 0
    onTriggered: elapsed++
  }

  Process {
    id: wpList
    command: ["sh", "-c", "find $HOME/Pictures/Wallpapers -maxdepth 1 -type f \\( -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.png' -o -iname '*.webp' \\) 2>/dev/null | shuf -n1"]
    running: true
    stdout: StdioCollector {
      onStreamFinished: {
        var path = this.text.trim()
        if (path.length > 0) shell.wallpaperPath = "file://" + path
      }
    }
  }

  // Trigger launcher open via file IPC: `date +%s > /tmp/qs-launcher-trigger`
  FileView {
    id: launcherTrigger
    path: "/tmp/qs-launcher-trigger"
    watchChanges: true
    onFileChanged: shell.launcherOpen = true
  }

  // Trigger settings open via file IPC: `date +%s > /tmp/qs-settings-trigger`
  FileView {
    id: settingsTrigger
    path: "/tmp/qs-settings-trigger"
    watchChanges: true
    onFileChanged: shell.settingsOpen = true
  }

  BackdropWindow {
    shell: shell
  }

  WallpaperWindow {
    shell: shell
  }

  TopBarWindow {
    shell: shell
    niri: niri
    stopwatchTimer: stopwatchTimer
    notifOverlay: notifOverlay
  }

  FileBufferWindow {
    shell: shell
  }

  DockWindow {
    shell: shell
    niri: niri
    stopwatchTimer: stopwatchTimer
    notifModel: notifOverlay.notificationModel
  }

  AppLauncherWindow {
    shell: shell
  }

  SettingsWindow {
    shell: shell
  }

  NotificationOverlay {
    id: notifOverlay
    shell: shell
    notifServer: notifServer
  }

  StatusPopup {
    id: statusPopup
    shell: shell
  }
}
