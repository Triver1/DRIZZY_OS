pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick

/**
 * Apps service with mock data for app launcher
 */
QtObject {
    id: root

    property bool visible: false
    
    readonly property list<QtObject> apps: [
        QtObject {
            readonly property string name: "Firefox"
            readonly property string icon: "🌐"
            readonly property string exec: "firefox"
            readonly property string description: "Web Browser"
        },
        QtObject {
            readonly property string name: "Kitty"
            readonly property string icon: "💻"
            readonly property string exec: "kitty"
            readonly property string description: "Terminal Emulator"
        },
        QtObject {
            readonly property string name: "Code"
            readonly property string icon: "📝"
            readonly property string exec: "code"
            readonly property string description: "Code Editor"
        },
        QtObject {
            readonly property string name: "Nautilus"
            readonly property string icon: "📁"
            readonly property string exec: "nautilus"
            readonly property string description: "File Manager"
        },
        QtObject {
            readonly property string name: "Discord"
            readonly property string icon: "💬"
            readonly property string exec: "discord"
            readonly property string description: "Chat Application"
        },
        QtObject {
            readonly property string name: "GIMP"
            readonly property string icon: "🎨"
            readonly property string exec: "gimp"
            readonly property string description: "Image Editor"
        }
    ]

    function toggleLauncher() {
        visible = !visible
    }

    function hideLauncher() {
        visible = false
    }

    function showLauncher() {
        visible = true
    }

    function launchApp(execCommand) {
        Qt.createQmlObject(`
            import Quickshell.Io
            import QtQuick
            
            Process {
                command: ["${execCommand}"]
                running: true
                
                Component.onCompleted: {
                    startDetached()
                    destroy()
                }
            }
        `, root)
        hideLauncher()
    }
}
