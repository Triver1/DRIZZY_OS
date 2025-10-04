import asyncio
from ignis import widgets
from ignis.services.network import NetworkService
from widgets.OptionButton import OptionButton


class WifiLabel(widgets.Box):
    def __init__(self, monitor_name=None):
        self.wifi_device = NetworkService.get_default().wifi.devices[0]

        def render_ap(ap):
            if ap is None:
                return [widgets.Label(label="No network")]
            # Re-render when connection state flips
            return ap.bind(
                "is_connected",
                lambda connected: (
                    [widgets.Label(label="No network")]
                    if not connected else
                    [widgets.Icon(pixel_size=10, image=ap.icon_name),
                     widgets.Label(label=ap.ssid)]
                )
            )

        super().__init__(
            spacing=5,
            vexpand=True,
            valign="start",
            child=self.wifi_device.bind("ap", render_ap),
        )

class WifiButton(widgets.Button):
    def __init__(self):
        super().__init__(child=WifiLabel())


class WifiPage(widgets.Box):
    def __init__(self):
        self.wifi = NetworkService.get_default().wifi
        self.wifi_device = self.wifi.devices[0]

        def make_ap_item(ap):
            ssid = ap.ssid or "(hidden SSID)"

            def do_connect(*_, ap=ap):
                asyncio.create_task(ap.connect_to_graphical())

            def do_disconnect(*_, ap=ap):
                asyncio.create_task(ap.disconnect_from())

            def do_remove(*_, ap=ap):
                ap.forget()

            def noop(*_):
                pass

            return OptionButton(
                ssid + " (connected)"if ap.is_connected else ssid,
                {
                    "default": {
                        "connect":    {"on_click": do_connect,    "next_menu": "close"},
                        "disconnect": {"on_click": do_disconnect, "next_menu": "close"},
                        "remove":     {"on_click": noop,          "next_menu": "remove_confirm"},
                    },
                    "remove_confirm": {
                        "confirm removal": {"on_click": do_remove, "next_menu": "close"},
                        "back":            {"on_click": noop,      "next_menu": "default"},
                    },
                },
            )

        def _toggle_wifi_power(*_):
            self.wifi.set_enabled(not self.wifi.enabled)

        def _scan_wifi(*_):
            self.wifi_device.request_scan()

        def render_aps(aps):
            power_button = widgets.Button(
                label=self.wifi.bind(
                    "enabled", transform=lambda e: "Power: On" if e else "Power: Off"
                ),
                on_click=_toggle_wifi_power,
            )

            scan_button = widgets.Button(label="Scan", on_click=_scan_wifi)

            controls = widgets.Box(spacing=10, child=[power_button, scan_button])
            items = [make_ap_item(ap) for ap in aps[0:5]]
            return [controls] + items

        super().__init__(
            hexpand=True,
            halign="fill",
            vertical=True,
            spacing=5,
            child=self.wifi_device.bind(
                "access_points",
                render_aps,
            ),
        )
