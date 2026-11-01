// Quickshell twin of the nimbus top bar, for fair benchmarking.
//
//   quickshell -p modules/no-nix/nimbus/bench/qml/shell.qml
//   NIMBUS_BENCH=idle quickshell -p …/shell.qml   # static after entrance
//
// No niri, no external processes, no system polling: the bar is driven by
// the same deterministic workload as `NIMBUS_BENCH=1 nimbus`.
import Quickshell
import Quickshell.Wayland
import QtQuick

ShellRoot {
    id: root

    readonly property bool idle: Quickshell.env("NIMBUS_BENCH") === "idle"

    // -1: no state yet. nimbus applies snapshot 0 after 300 ms, then (unless
    // idle) advances n every 350 ms on an independent timer.
    property int n: -1
    property int tick: 0

    SystemClock {
        id: clock
        precision: SystemClock.Minutes
    }

    Timer {
        interval: 300
        running: true
        onTriggered: if (root.n < 0) root.n = 0
    }

    Timer {
        interval: 350
        repeat: true
        running: !root.idle
        onTriggered: {
            root.tick += 1
            root.n = root.tick
        }
    }

    Variants {
        model: Quickshell.screens

        PanelWindow {
            id: win
            required property var modelData

            screen: modelData
            anchors {
                top: true
                left: true
                right: true
            }
            implicitHeight: 46
            exclusiveZone: 40
            color: "transparent"
            focusable: false

            WlrLayershell.namespace: "nimbus-qml"
            WlrLayershell.layer: WlrLayer.Top
            WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

            // Like nimbus: clicks outside the islands (inflated 3 px) fall through.
            mask: Region {
                Region {
                    x: Math.floor(bar.islands[0].x - 3)
                    y: Math.floor(bar.islands[0].y - 3)
                    width: Math.ceil(bar.islands[0].width + 6)
                    height: Math.ceil(bar.islands[0].height + 6)
                }
                Region {
                    x: Math.floor(bar.islands[1].x - 3)
                    y: Math.floor(bar.islands[1].y - 3)
                    width: Math.ceil(bar.islands[1].width + 6)
                    height: Math.ceil(bar.islands[1].height + 6)
                }
                Region {
                    x: Math.floor(bar.islands[2].x - 3)
                    y: Math.floor(bar.islands[2].y - 3)
                    width: Math.ceil(bar.islands[2].width + 6)
                    height: Math.ceil(bar.islands[2].height + 6)
                }
            }

            // Like nimbus' glass look: ask the compositor to blur behind each
            // visible glass body (ext-background-effect), capsule-shaped.
            BackgroundEffect.blurRegion: Region {
                Region {
                    readonly property Item isl: bar.islands[0]
                    readonly property bool on: isl.alpha > 0.05
                    x: Math.round(isl.x - isl.grow)
                    y: Math.round(isl.y - isl.grow)
                    width: on ? Math.round(isl.width + 2 * isl.grow) : 0
                    height: on ? Math.round(isl.height + 2 * isl.grow) : 0
                    radius: height / 2
                }
                Region {
                    readonly property Item isl: bar.islands[1]
                    readonly property bool on: isl.alpha > 0.05
                    x: Math.round(isl.x - isl.grow)
                    y: Math.round(isl.y - isl.grow)
                    width: on ? Math.round(isl.width + 2 * isl.grow) : 0
                    height: on ? Math.round(isl.height + 2 * isl.grow) : 0
                    radius: height / 2
                }
                Region {
                    readonly property Item isl: bar.islands[2]
                    readonly property bool on: isl.alpha > 0.05
                    x: Math.round(isl.x - isl.grow)
                    y: Math.round(isl.y - isl.grow)
                    width: on ? Math.round(isl.width + 2 * isl.grow) : 0
                    height: on ? Math.round(isl.height + 2 * isl.grow) : 0
                    radius: height / 2
                }
            }

            Bar {
                id: bar
                anchors.fill: parent
                n: root.n
                now: clock.date
            }
        }
    }
}
