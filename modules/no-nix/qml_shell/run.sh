#!/usr/bin/env sh
DIR="$(dirname "$(realpath "$0")")"
export QML_IMPORT_PATH="$DIR:$QML_IMPORT_PATH"
exec quickshell -p "$DIR/shell.qml" "$@"
