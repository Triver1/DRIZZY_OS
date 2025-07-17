pragma Singleton
pragma ComponentBehavior: Bound

import Quickshell
import Quickshell.Io
import QtQuick

/**
 * Simple brightness management using brightnessctl
 */
Singleton {
    id: root

    property real brightness: 0.5
    property bool ready: false
    
    // Format brightness as percentage for display
    property string displayText: Math.round(brightness * 100) + "%"

    function setBrightness(value: real): void {
        value = Math.max(0.01, Math.min(1, value))
        const rounded = Math.round(value * 100)
        if (Math.round(brightness * 100) === rounded)
            return
        brightness = value
        setProc.command = ["brightnessctl", "s", `${rounded}%`, "--quiet"]
        setProc.startDetached()
    }

    function increaseBrightness(): void {
        setBrightness(brightness + 0.1)
    }

    function decreaseBrightness(): void {
        setBrightness(brightness - 0.1)
    }

    // Get current brightness on startup
    Component.onCompleted: {
        initProc.running = true
    }

    Process {
        id: initProc
        command: ["sh", "-c", "echo \"$(brightnessctl g) $(brightnessctl m)\""]
        stdout: SplitParser {
            onRead: data => {
                const [current, max] = data.trim().split(" ")
                root.brightness = parseInt(current) / parseInt(max)
                root.ready = true
            }
        }
    }

    Process {
        id: setProc
    }
} 