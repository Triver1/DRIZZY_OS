// Bar.qml
import Quickshell

Scope {
  // no more time object

  Variants {
    model: Quickshell.screens

    PanelWindow {
      property var modelData
      screen: modelData
      color: "#191724"
      anchors {
        top: true
        left: true
        right: true
      }

      implicitHeight: 30

      AppLauncher {
        anchors {
          left: parent.left
          verticalCenter: parent.verticalCenter
          leftMargin: 10
        }
      }

      ClockWidget {
        anchors.centerIn: parent
      }

      
      BrightnessWidget {
        id: brightnessWidget
        anchors {
          right: parent.right
          verticalCenter: parent.verticalCenter
          rightMargin: 10
        }
      }
      
      BatteryWidget {
        anchors {
          right: brightnessWidget.left
          verticalCenter: parent.verticalCenter
          rightMargin: 10
        }
      }

      // VolumeWidget {
      //   id: volumeWidget
      //   anchors {
      //     right: parent.right
      //     verticalCenter: parent.verticalCenter
      //     rightMargin: 110
      //   }
      // }
    }
  }
}
