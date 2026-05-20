import Quickshell
import Quickshell.Wayland
import QtQuick

PanelWindow {
  id: wl
  property var shell

  screen: shell.modelData
  implicitWidth:  shell.modelData.width
  implicitHeight: shell.modelData.height
  exclusionMode: ExclusionMode.Ignore
  aboveWindows: false
  WlrLayershell.layer: WlrLayer.Background
  WlrLayershell.namespace: "quickshell-wallpaper"
  color: "#2b2d3a"

  // Previous wallpaper — stays visible underneath during the whole reveal.
  // swapping = true means the animation just finished and we are waiting for
  // this image to finish loading the new source before we collapse the iris.
  Image {
    id: imgBack
    anchors.fill: parent
    fillMode: Image.PreserveAspectCrop
    asynchronous: true; smooth: true; cache: true

    property bool swapping: false

    onStatusChanged: {
      if (swapping && status === Image.Ready) {
        swapping = false
        iris.size   = 0
        imgFront.source = ""
      }
    }
  }

  // Expanding circle that reveals the new wallpaper.
  //
  // layer.enabled: true is the key — clip:true alone clips to the bounding
  // *rectangle*, but layer.enabled causes Qt to composite the item using its
  // actual visual shape (the rounded rect / circle) so children are masked
  // to a real circle.
  Rectangle {
    id: iris

    property real size: 0
    property real ox: wl.implicitWidth  / 2
    property real oy: wl.implicitHeight / 2

    x: ox - size / 2
    y: oy - size / 2
    width:  size
    height: size
    radius: size / 2

    clip: true           // clips imgFront to the bounding square first …
    layer.enabled: true  // … then Qt masks that square to the rounded circle shape
    color: "transparent"
    visible: size > 0

    Image {
      id: imgFront
      // Shift so the image is always aligned with the full screen, not the iris.
      x: -iris.x
      y: -iris.y
      width:  wl.implicitWidth
      height: wl.implicitHeight
      fillMode: Image.PreserveAspectCrop
      asynchronous: true; smooth: true; cache: true

      onStatusChanged: {
        if (status !== Image.Ready || source === "") return
        // Pick a fresh near-centre origin each time
        iris.ox = wl.implicitWidth  * (0.38 + Math.random() * 0.24)
        iris.oy = wl.implicitHeight * (0.38 + Math.random() * 0.24)
        // Diameter must reach the farthest corner from that origin
        var dx = Math.max(iris.ox, wl.implicitWidth  - iris.ox)
        var dy = Math.max(iris.oy, wl.implicitHeight - iris.oy)
        revealAnim.to = Math.ceil(Math.sqrt(dx * dx + dy * dy)) * 2 + 8
        revealAnim.restart()
      }
    }
  }

  NumberAnimation {
    id: revealAnim
    target: iris; property: "size"
    from: 0; to: 3000            // overwritten dynamically above
    duration: 860
    easing.type: Easing.OutCirc  // natural for a circular reveal

    onFinished: {
      // Hand the new image to imgBack and wait for it to confirm it is ready
      // before collapsing iris. This prevents the one-frame flash where imgBack
      // would briefly show the old wallpaper while still loading.
      imgBack.swapping = true
      imgBack.source   = imgFront.source
      // iris.size stays > 0 (keeping imgFront visible) until imgBack.onStatusChanged
    }
  }

  Connections {
    target: shell
    function onWallpaperPathChanged() {
      if (!shell.wallpaperPath) return

      // Very first load — no reveal, just set the background directly.
      if (imgBack.source === "") {
        imgBack.source = shell.wallpaperPath
        return
      }

      // If a reveal or swap is already in progress, abort it cleanly.
      imgBack.swapping = false
      if (revealAnim.running) {
        revealAnim.stop()
        imgBack.source = imgFront.source !== "" ? imgFront.source : imgBack.source
      }
      iris.size = 0
      imgFront.source = shell.wallpaperPath   // onStatusChanged → revealAnim
    }
  }
}
