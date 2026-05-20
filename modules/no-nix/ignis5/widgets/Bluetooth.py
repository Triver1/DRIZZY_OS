from ignis import widgets
from ignis.services.bluetooth import BluetoothService


class Bluetooth(widgets.Box):
    def __init__(self):
        self.bluetooth = BluetoothService.get_default()

        def render_status(devices):
            if not self.bluetooth.powered:
                return [
                    widgets.Icon(pixel_size=12, image="bluetooth-disabled-symbolic"),
                    widgets.Label(label="Off"),
                ]
            connected = next((d for d in devices if d.connected), None)
            if connected is None:
                return [
                    widgets.Icon(pixel_size=12, image="bluetooth-disconnected-symbolic"),
                    widgets.Label(label="—"),
                ]
            return [
                widgets.Icon(pixel_size=12, image=connected.icon_name),
                widgets.Label(label=connected.name[:12]),
            ]

        super().__init__(
            orientation="vertical",
            spacing=5,
            halign="center",
            css_classes=["bluetooth-indicator"],
            child=self.bluetooth.bind("devices", render_status),
        )
