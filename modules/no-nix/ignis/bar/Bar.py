from ignis import widgets
from ignis import utils
import datetime

# # Services
# from ignis.services.audio import AudioService
from ignis.services.system_tray import SystemTrayService, SystemTrayItem
from ignis.services.niri import NiriService, NiriWorkspace
from ignis.services.notifications import NotificationService
from ignis.services.mpris import MprisService, MprisPlayer
from ignis.services.upower import UPowerService
from ignis.services.network import NetworkService
from controlcenter import ControlCenter




# audio = AudioService.get_default()
network = NetworkService.get_default()
system_tray = SystemTrayService.get_default()
niri = NiriService.get_default()
notifications = NotificationService.get_default()
mpris = MprisService.get_default()

upower_battery = UPowerService.get_default().batteries[0]

controlcenter = ControlCenter()

# Modules:
# Clock, Battery, Time, Launcher 

class Panel(widgets.Button):
    def __init__(self, *, child=None, on_click=None, css_classes=None, **kwargs):
        all_classes = ["panel"] + (css_classes or [])

        super().__init__(
            child=child,
            on_click=on_click,
            css_classes=all_classes,
            **kwargs
        )

class IconPanel(Panel):
    def __init__(self, *, icon_name=None, text=None, pixel_size=20, spacing=5, on_click=None, css_classes=None, **kwargs):
        widgets_list = []
        
        if icon_name:
            widgets_list.append(widgets.Icon(image=icon_name, pixel_size=pixel_size))
        
        if text:
            widgets_list.append(widgets.Label(label=text))
        
        if len(widgets_list) == 0:
            child = None
        elif len(widgets_list) == 1:
            child = widgets_list[0]
        else:
            child = widgets.Box(spacing=spacing, child=widgets_list)
            
        super().__init__(
            child=child,
            on_click=on_click,
            css_classes=css_classes,
            **kwargs
        )


class Launcher(IconPanel):
    def __init__(self):
        super().__init__(
            icon_name="nix",
            pixel_size=25,
            on_click=lambda x: controlcenter.reveal() 
        )



class ClockCalendar(Panel):
    def __init__(self):
        super().__init__(
            css_classes=["clock"],
            label=utils.Poll(
                1_000, lambda self: datetime.datetime.now().strftime("%H:%M")
            ).bind("output"),
        )



class Media(Panel):
    def __init__(self):
        super().__init__(
            css_classes=["media"],
            child=widgets.Box(
                spacing=10,
                child=[
                    widgets.Label(
                        label= "No media",
                        visible= mpris.bind("players", lambda value: len(value) == 0)
                    )
                ],
                setup= lambda box: mpris.connect(
                    "player-added", lambda x, player: box.append(self.mpris_title(player))

                ),
            )
        )

    def mpris_title(self, player: MprisPlayer):
        return widgets.Box(
            spacing=10,
            setup=lambda box: player.connect(
                "closed",
                lambda x: box.unparent(),  # remove widget when player is closed
            ),
            child=[
                widgets.Icon(image="audio-x-generic-symbolic"),
                widgets.Label(
                    ellipsize="end",
                    max_width_chars=20,
                    label=player.bind("title"),
                ),
            ],
        )


class Battery(Panel):
    def __init__(self):
        super().__init__(
            css_classes=["battery"],
            child=upower_battery.bind(
                "percent",
                transform = lambda percent: self.update_battery(percent)
            )
        )
        self.update_battery(upower_battery.percent)

    def update_battery(self, battery):
        # Remove previous classes
        self.remove_css_class("low")
        self.remove_css_class("verylow")
        
        if battery < 10:
            self.add_css_class("verylow")  
        elif battery < 20:
            self.add_css_class("low")

        
        return widgets.Box(
            spacing=5,
            child=[
            widgets.Icon(image=upower_battery.icon_name),
            widgets.Label(label=f"{battery:.0f}%")
        ])

class Network(Panel):
    def __init__(self):
        if network.wifi:
            # We are dealing with a wifi module
            self.wifi_device = network.wifi.devices[0]
            
            super().__init__(
                child=self.wifi_device.bind("ap", transform=lambda ap: self.create_wifi_widget(ap))
            )
        else:
            super().__init__(
                child=widgets.Box(
                    spacing=5,
                    child=[
                        widgets.Icon(image="network-wired"),
                        widgets.Label(label="Ethernet")
                    ]
                )
            )
    
    def create_wifi_widget(self, ap):
        return widgets.Box(
            spacing=5,
            child=[
                # Bind the icon to the access point's icon_name for signal strength updates
                widgets.Icon(image=ap.bind("icon_name")),
                widgets.Label(label=ap.bind("is_connected", transform=lambda connected: ap.ssid if connected else ""))
            ]
        )






class NiriWorkspaceButton(Panel):
    def __init__(self, workspace: NiriWorkspace):
        super().__init__(
            css_classes=["workspace"],
            on_click=lambda x: workspace.switch_to(),
            child=widgets.Label(label=str(workspace.id))
        )
        if workspace.is_active:
            self.add_css_class("active")

class NiriWorkspaces(widgets.EventBox):

    def __init__(self,monitor_name):
            super().__init__(

                on_scroll_up=lambda x: self.scroll_workspace(1),
                on_scroll_down=lambda x: self.scroll_workspace(1),
                spacing=5,
                child=niri.bind(
                    "workspaces",
                    transform = lambda value: [
                        NiriWorkspaceButton(i) for i in value 
                    ]
                )
            )


    def scroll_workspace(self,dir):
        current = list(
        filter(lambda w: w.is_active, niri.workspaces) )[0].idx
        target = (current + dir) % (len(niri.workspaces) + 1)
        print(target)
        niri.switch_to_workspace(target) 






class Bar(widgets.Window):  # inheriting from widgets.Window
    __gtype_name__ = "MyBar"  # optional, this will change the widget's display name in the GTK inspector.
    
    def __init__(self, monitor: int):
        left_box = widgets.Box(
            spacing=10,
            child=[
                Launcher(),
                Network(),
                NiriWorkspaces(monitor),
            ]
        )
        
        center_box = widgets.Box(
            spacing=10,
            child=[
                # widgets.Label(label="GLITTERHONINGKOEKJE😻"),
                Media(),
                ClockCalendar(),
            ]
        )
        
        right_box = widgets.Box(
            spacing=10,
            child=[
                Battery(),
            ]
        )
        
        super().__init__(  
            namespace=f"some-window-{monitor}",
            css_classes=["bar"],
            monitor=monitor,
            anchor=["left", "top", "right"],
            exclusivity="exclusive",
            child=widgets.CenterBox(
                start_widget=left_box,
                center_widget=center_box,
                end_widget=right_box,
            ),
        )
