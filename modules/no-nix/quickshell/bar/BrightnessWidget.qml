// BrightnessWidget.qml
import QtQuick
import "root:/services"

Item {
    width: brightnessText.width
    height: brightnessText.height
    
    Text {
        id: brightnessText
        text: "☀ " + Brightness.displayText
        color: Theme.textColor
        font.family: Theme.fontFamily
        font.pixelSize: Theme.fontSize
        font.weight: Theme.fontWeightThick
    }
    
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        
        onClicked: (mouse) => {
            if (mouse.button === Qt.LeftButton) {
                Brightness.increaseBrightness()
            } else if (mouse.button === Qt.RightButton) {
                Brightness.decreaseBrightness()
            }
        }
        
        onWheel: (wheel) => {
            if (wheel.angleDelta.y > 0) {
                Brightness.increaseBrightness()
            } else {
                Brightness.decreaseBrightness()
            }
        }
    }
} 