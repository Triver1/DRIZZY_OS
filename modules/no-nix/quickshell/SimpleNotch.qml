import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Widgets

WlrLayershell {
  id: notch

  required property ShellScreen modelData

  anchors.left: true
  anchors.right: true
  anchors.top: true
  color: "transparent"
  exclusionMode: ExclusionMode.Ignore
  focusable: false
  implicitHeight: 50
  layer: WlrLayer.Top
  namespace: "simple.notch.quickshell"
  screen: modelData
  surfaceFormat.opaque: false

  mask: Region {
    Region {
      item: notchRect
    }
  }

  Rectangle {
    id: notchRect

    width: 700
    height: 35
    anchors.horizontalCenter: parent.horizontalCenter
    bottomLeftRadius: 20
    bottomRightRadius: 20
    color: "#2A2A2A"
    opacity: 0.9

    // Widget container layout
    RowLayout {
      anchors.fill: parent
      anchors.margins: 8
      spacing: 12

      // Left section for widgets
      RowLayout {
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignLeft
        spacing: 8
        
        // Add your left-side widgets here
      }

      // Center section for widgets  
      RowLayout {
        Layout.alignment: Qt.AlignCenter
        spacing: 8
        
        // Add your center widgets here (like clock)
      }

      // Right section for widgets
      RowLayout {
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignRight
        spacing: 8
        
        // Add your right-side widgets here (like battery, brightness, etc.)
      }
    }
  }
} 