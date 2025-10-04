from ignis import widgets
from ignis.services.network import NetworkService

class Wifi(widgets.Box):
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
                    [widgets.Icon(pixel_size=10, image=getattr(ap, "icon_name", "")),
                     widgets.Label(label=getattr(ap, "ssid", ""))]
                )
            )

        super().__init__(
            spacing=5,
            vexpand=True,
            valign="start",
            child=self.wifi_device.bind("ap", render_ap),
        )
