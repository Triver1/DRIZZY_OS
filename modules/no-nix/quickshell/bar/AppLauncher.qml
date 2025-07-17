// AppLauncher.qml
import QtQuick
import Quickshell
import "root:/services"

Item {
    width: nixLogo.width + 20
    height: nixLogo.height + 10

    // NixOS Logo
    Image {
        id: nixLogo
        source: "root:/assets/nix_logo.svg"
        width: 20
        height: 20
        anchors.centerIn: parent
    }

    MouseArea {
        anchors.fill: parent
        onClicked: Apps.toggleLauncher()
        cursorShape: Qt.PointingHandCursor
    }

    // App Launcher Popup Window
    PopupWindow {
        id: appLauncherPopup
        visible: Apps.visible
        width: 500
        height: 400
        color: Theme.backgroundColor
        
        screen: Quickshell.screens[0]

        Rectangle {
            anchors.fill: parent
            color: Theme.backgroundColor
            border.color: Theme.textColor
            border.width: 2
            radius: 12

            // Header
            Rectangle {
                id: header
                width: parent.width
                height: 50
                color: Qt.rgba(0.2, 0.2, 0.2, 0.5)
                radius: 12

                Text {
                    text: "Applications"
                    color: Theme.textColor
                    font.family: Theme.fontFamily
                    font.pixelSize: 16
                    font.weight: Theme.fontWeightThick
                    anchors.centerIn: parent
                }

                // Close button
                Text {
                    text: "✕"
                    color: Theme.textColor
                    font.pixelSize: 18
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.rightMargin: 15

                    MouseArea {
                        anchors.fill: parent
                        anchors.margins: -5
                        onClicked: Apps.hideLauncher()
                        cursorShape: Qt.PointingHandCursor
                    }
                }
            }

            // App Grid
            GridView {
                id: grid
                anchors.top: header.bottom
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: 20
                cellWidth: 120
                cellHeight: 100
                model: Apps.apps

                delegate: Rectangle {
                    width: grid.cellWidth - 10
                    height: grid.cellHeight - 10
                    color: "transparent"
                    radius: 8

                    Column {
                        anchors.centerIn: parent
                        spacing: 8

                        Text {
                            text: modelData.icon
                            font.pixelSize: 32
                            anchors.horizontalCenter: parent.horizontalCenter
                        }

                        Text {
                            text: modelData.name
                            color: Theme.textColor
                            font.family: Theme.fontFamily
                            font.pixelSize: 12
                            anchors.horizontalCenter: parent.horizontalCenter
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        onClicked: Apps.launchApp(modelData.exec)
                        cursorShape: Qt.PointingHandCursor

                        onEntered: parent.color = Qt.rgba(1, 1, 1, 0.1)
                        onExited: parent.color = "transparent"
                    }
                }
            }
        }

        // Click outside to close
        MouseArea {
            anchors.fill: parent
            z: -1
            onClicked: Apps.hideLauncher()
        }
    }
} 