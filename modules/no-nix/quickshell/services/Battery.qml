// Time.qml
pragma Singleton

import Quickshell
import Quickshell.Services.UPower
import QtQuick

Singleton {
    property var chargeState: UPower.displayDevice ? UPower.displayDevice.state : 0
    property real percentage: UPower.displayDevice ? UPower.displayDevice.percentage : 0
    property bool isCharging: chargeState === UPowerDevice.Charging
    property bool isDischarging: chargeState === UPowerDevice.Discharging
    property bool isFullyCharged: chargeState === UPowerDevice.FullyCharged
    
    // Optional: Add a formatted string for display
    property string displayText: {
        if (!UPower.displayDevice) return "No battery"
        
        const percent = Math.round(percentage * 100)
        switch (chargeState) {
            case UPowerDevice.Charging:
                return `${percent}% (Charging)`
            case UPowerDevice.Discharging:
                return `${percent}%`
            case UPowerDevice.FullyCharged:
                return `${percent}% (Full)`
            default:
                return `${percent}%`
        }
    }
}
