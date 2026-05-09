import asyncio
from ignis import widgets
from ignis.services.bluetooth import BluetoothService
from widgets.OptionButton import OptionButton


class BluetoothLabel(widgets.Box):
    def __init__(self, monitor_name=None):
        self.bluetooth = BluetoothService.get_default()

        def render_summary(devices):
            if not self.bluetooth.powered:
                return [
                    widgets.Icon(pixel_size=10, image="bluetooth-disabled-symbolic"),
                    widgets.Label(label="Bluetooth"),
                ]

            connected = next((d for d in devices if d.connected), None)
            if connected is None:
                return [
                    widgets.Icon(pixel_size=10, image="bluetooth-disconnected-symbolic"),
                    widgets.Label(label="Bluetooth"),
                ]
            return [
                widgets.Icon(pixel_size=10, image=connected.icon_name),
                widgets.Label(label=connected.name),
            ]

        super().__init__(
            spacing=5,
            vexpand=True,
            valign="start",
            child=self.bluetooth.bind("devices", render_summary),
        )


class BluetoothButton(widgets.Button):
    def __init__(self):
        super().__init__(child=BluetoothLabel())


class BluetoothPage(widgets.Box):
    def __init__(self):
        self.bluetooth = BluetoothService.get_default()

        def make_device_item(device):
            name = device.name or "(unknown device)"

            def do_connect(*_, d=device):
                asyncio.create_task(d.connect_to())

            def do_disconnect(*_, d=device):
                asyncio.create_task(d.disconnect_from())

            label_text = name + (" (connected)" if device.connected else "")
            return OptionButton(
                label_text,
                {
                    "default": {
                        "connect": {"on_click": do_connect, "next_menu": "close"},
                        "disconnect": {"on_click": do_disconnect, "next_menu": "close"},
                    },
                },
            )

        def render_devices(devices):
            power_button = widgets.Button(
                label=self.bluetooth.bind(
                    "powered", transform=lambda p: "Power: On" if p else "Power: Off"
                ),
                on_click=lambda *_: self.bluetooth.set_powered(not self.bluetooth.powered),
            )

            scan_button = widgets.Button(
                label=self.bluetooth.bind(
                    "setup_mode", transform=lambda s: "Scanning: On" if s else "Scanning: Off"
                ),
                on_click=lambda *_: self.bluetooth.set_setup_mode(not self.bluetooth.setup_mode),
            )

            controls = widgets.Box(spacing=10, child=[power_button, scan_button])
            items = [make_device_item(d) for d in devices[0:5]]
            return [controls] + items

        super().__init__(
            hexpand=True,
            spacing=8,
            halign="fill",
            vertical=True,
            child=self.bluetooth.bind("devices", render_devices),
        )


