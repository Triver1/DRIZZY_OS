
// Time.qml
pragma Singleton

import Quickshell
import Quickshell.Services.Pipewire
import QtQuick

Singleton {
    property var defaultSink: Pipewire.defaultAudioSink
    property real volume: defaultSink ? defaultSink.volume : 0
    property bool isMuted: defaultSink ? defaultSink.muted : false
    property bool isConnected: defaultSink !== null
    
    // Optional: Add a formatted string for display
    property string displayText: {
        if (!defaultSink) return "No audio device"
        
        const percent = Math.round(volume * 100)
        if (isMuted) {
            return `${percent}% (Muted)`
        } else {
            return `${percent}%`
        }
    }
}
