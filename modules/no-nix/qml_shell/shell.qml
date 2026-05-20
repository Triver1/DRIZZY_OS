import Quickshell
import QtQuick

Scope {
  Variants {
    model: Quickshell.screens
    ScreenShell {}
  }
}
