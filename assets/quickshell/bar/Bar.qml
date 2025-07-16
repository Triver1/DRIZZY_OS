// Bar.qml
import Quickshell

Scope {
  // no more time object

  Variants {
    model: Quickshell.screens

    PanelWindow {
      property var modelData
      screen: modelData
      color: "#2d3748"
      anchors {
        top: true
        left: true
        right: true
      }

      implicitHeight: 30

      ClockWidget {
        anchors.centerIn: parent

        // no more time binding
      }
    }
  }
}
