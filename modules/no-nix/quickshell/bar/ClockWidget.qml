// ClockWidget.qml
import QtQuick
import "root:/services"

Text {
  // we no longer need time as an input

  // directly access the time property from the Time singleton
  text: Time.time
  color: Theme.textColor
  font.family: Theme.fontFamily
  font.pixelSize: Theme.fontSize
  font.weight: Theme.fontWeightThick
}
