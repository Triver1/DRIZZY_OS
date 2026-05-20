import Quickshell
import Quickshell.Wayland
import QtQuick
import "../theme"

PanelWindow {
  id: backdrop
  property var shell

  screen: shell.modelData
  implicitWidth: shell.modelData.width
  implicitHeight: shell.modelData.height
  exclusionMode: ExclusionMode.Ignore
  aboveWindows: false

  WlrLayershell.layer: WlrLayer.Background
  WlrLayershell.namespace: "backdrop"

  color: Theme.bg

  function getBlurredPath(path) {
    if (!path) return ""
    var parts = path.split("/")
    var filename = parts.pop()
    return parts.join("/") + "/Blurred/" + filename
  }

  Image {
    anchors.fill: parent
    source: getBlurredPath(shell.wallpaperPath)
    fillMode: Image.PreserveAspectCrop
    visible: shell.wallpaperPath !== ""
    asynchronous: true; smooth: true; cache: false
  }
}
