from ignis import widgets
from ignis.services.network import NetworkService

class Wifi(widgets.Box):
    def __init__(self, monitor_name=None):
        self.wifi_device = NetworkService.get_default().wifi.devices[0]

        def render_ap(ap):
            if ap is None:
                return [widgets.Label(label="X")]
            # Re-render when connection state flips
            return ap.bind(
                "is_connected",
                lambda connected: (
                    [widgets.Label(label="X")]
                    if not connected else
                    [widgets.Icon(pixel_size=10, image=getattr(ap, "icon_name", ""))]
                )
            )

        super().__init__(
            orientation="vertical",
            spacing=5,
            hexpand=True,
            halign="center",
            child=self.wifi_device.bind("ap", render_ap),
        )
