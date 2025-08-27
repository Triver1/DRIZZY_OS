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
from notifications import NotificationsCenter
from media import PlayerPopup
from sharedwidgets.StatusBox import public_status_box, set_open_menu_callback




# audio = AudioService.get_default()
network = NetworkService.get_default()
system_tray = SystemTrayService.get_default()
niri = NiriService.get_default()
notifications = NotificationService.get_default()
mpris = MprisService.get_default()

upower_battery = UPowerService.get_default().batteries[0]

controlcenter = ControlCenter()
notifications_center = NotificationsCenter()
player_popup = PlayerPopup()
set_open_menu_callback(lambda name: controlcenter.OpenMenu(name))

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

class PanelNoClick(widgets.Box):
    def __init__(self, *, child=None, css_classes=None, **kwargs):
        all_classes = ["panel"] + (css_classes or [])

        super().__init__(
            child=[child],
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
            on_click=lambda x: controlcenter.toggle_reveal() 
        )



class ClockCalendar(Panel):
    def __init__(self):
        super().__init__(
            css_classes=["clock"],
            label=utils.Poll(
                1_000, lambda self: datetime.datetime.now().strftime("%H:%M")
            ).bind("output"),
        )


class Notifications(IconPanel):
    def __init__(self):
        super().__init__(
            pixel_size=17,
            icon_name="preferences-system-notifications-symbolic",  # or "bell-symbolic"
            css_classes=["notifications"],
            text=None,  # you could add count of unread notifications
            on_click=lambda x: notifications_center.toggle_reveal()
        )

class Media(Panel):
    def __init__(self):
        super().__init__(
            css_classes=["media"],
            on_click=lambda x: player_popup.toggle_reveal(),
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
        self.show_time = False  # Toggle state for displaying time vs percentage
        
        super().__init__(
            css_classes=["battery"],
            on_click=lambda x: self.toggle_display(),
            child=upower_battery.bind(
                "percent",
                transform = lambda percent: self.update_battery(percent)
            )
        )
        self.update_battery(upower_battery.percent)

    def toggle_display(self):
        """Toggle between showing percentage and remaining time"""
        self.show_time = not self.show_time
        self.child = self.update_battery(upower_battery.percent)

    def format_time(self, seconds):
        """Format seconds into a readable time string"""
        if seconds <= 0:
            return "Unknown"
        
        hours = seconds // 3600
        minutes = (seconds % 3600) // 60
        
        if hours > 0:
            return f"{hours}h {minutes}m"
        else:
            return f"{minutes}m"

    def update_battery(self, battery):
        # Remove previous classes
        self.remove_css_class("low")
        self.remove_css_class("verylow")
        
        if battery < 10:
            self.add_css_class("verylow")  
        elif battery < 20:
            self.add_css_class("low")

        # Determine what text to show
        if self.show_time:
            label_text = self.format_time(upower_battery.time_remaining)
        else:
            label_text = f"{battery:.0f}%"
        
        return widgets.Box(
            spacing=5,
            child=[
                widgets.Icon(image=upower_battery.icon_name),
                widgets.Label(label=label_text)
            ]
        )

class Network(Panel):
    def __init__(self):
        if network.wifi:
            # We are dealing with a wifi module
            self.wifi_device = network.wifi.devices[0]
            
            super().__init__(
                child=self.wifi_device.bind("ap", transform=lambda ap: self.create_wifi_widget(ap)),
                on_click=lambda x: controlcenter.OpenMenu("Network")
            )
        else:
            super().__init__(
                child=widgets.Box(
                    spacing=5,
                    child=[
                        widgets.Icon(image="network-wired"),
                        widgets.Label(label="Ethernet")
                    ]
                ),
                on_click=lambda x: controlcenter.OpenMenu("Network")
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



class NiriWorkspaces(widgets.Box):
    def __init__(self, monitor_name):
        super().__init__(
            spacing=5,
            child=niri.bind(
                "workspaces",
                transform=self._create_workspace_buttons
            )
        )
        self.monitor_name = monitor_name
    
    def _create_workspace_buttons(self, workspaces):
        # Hide the trailing empty workspace unless it is currently active
        if workspaces and not workspaces[-1].is_active:
            workspaces = workspaces[:-1]
        return [self._create_button(workspace) for workspace in workspaces]
    
    def _create_button(self, workspace):
        workspace_display = niri.bind(
            "windows", 
            transform=lambda windows: self._get_workspace_content(workspace, windows)
        )
        
        button = Panel(
            css_classes=["workspace"] + (["active"] if workspace.is_active else []),
            on_click=lambda x: workspace.switch_to(),
            child=workspace_display
        )
        
        return button
    
    def _get_workspace_content(self, workspace, windows):
        # Get icons for windows in this workspace
        icons = [
            widgets.Icon(
                pixel_size=18,
                image=utils.Utils.get_app_icon_name(window.app_id)
            )
            for window in windows 
            if window.workspace_id == workspace.id
        ]
        return widgets.Box(
            spacing=10,
            child=icons
        )



class Systray(widgets.Box):
    def __init__(self):
        super().__init__(
            child=system_tray.bind("items", transform=lambda apps: [
                widgets.Icon(image=app.icon) for app in apps
            ])
        )


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
                # Public status box for timers and mini indicators
                public_status_box,
                ClockCalendar(),
            ]
        )
        
        right_box = widgets.Box(
            spacing=5,
            child=[
                Systray(),
                Battery(),
                Notifications()
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
