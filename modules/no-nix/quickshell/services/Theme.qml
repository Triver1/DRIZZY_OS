// Theme.qml
pragma Singleton
import QtQuick

QtObject {
    readonly property color textColor: "#e0def4"  // Light text for dark background
    readonly property string fontFamily: "DejaVu Sans"
    readonly property int fontSize: 12
    readonly property color backgroundColor: "#191724"
    
    // Font weight levels
    readonly property int fontWeightThin: Font.Light      // 300
    readonly property int fontWeightThick: Font.Bold      // 700
    readonly property int fontWeightThicker: Font.Black   // 900
} 
