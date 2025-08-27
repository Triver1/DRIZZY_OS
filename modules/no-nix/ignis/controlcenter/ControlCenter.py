import os
import subprocess
from ignis import widgets
from ignis.services.network import NetworkService
import asyncio
import time
from ignis import utils
from ignis.services.fetch import FetchService
from ignis.services.backlight import BacklightService
from ignis.services.bluetooth import BluetoothService
# from ignis.services.audio import AudioService

from theme_manager import ThemeManager
from sharedwidgets import PopupWindow
from sharedwidgets.StatusBox import public_status_box, open_menu

# audio = AudioService.get_default()
network = NetworkService.get_default()
backlight = BacklightService.get_default()
bluetooth = BluetoothService().get_default()




class DropDownPanel(widgets.Box):
    def __init__(self, label, buttons, child=[], vertical=False):

        if isinstance(label, str):
            label = widgets.Label(label=label)

        # Else it is simply not a label..

            
        self.revealed = False
        buttons = [widgets.Button(label=btn_label, on_click=on_click, css_classes=["wordbutton"]) for (btn_label, on_click) in buttons] 
        buttons.append(child)
        revealer = widgets.Revealer(
                visible=True,
                child=widgets.Box(
                    hexpand=True,
                    vertical=vertical,
                    child=buttons),
                transition_type='slide_down',
                transition_duration=300,
                reveal_child=False,                 
            css_classes=["dropdownpaneldropdown"]
        )

        arrow = widgets.Arrow(
            pixel_size=20,
            rotated=False,
            degree=90,
            time=135,
            direction="right",
            counterclockwise=False,
        )
        button = widgets.Button(css_classes=["dropdownpanel"], child=widgets.Box(child=[label, arrow]), on_click=lambda x: self.toggle())
        # Fixed: button first, then revealer
        super().__init__(child=[button, revealer], vertical=True)
        self.revealer = revealer
        self.arrow = arrow
        
    def toggle(self):
        self.revealed = not self.revealed
        self.revealer.reveal_child = self.revealed
        self.arrow.rotated = self.revealed



class WifiModule(widgets.Box):
    def __init__(self):
        # all networks 
        super().__init__(
            vertical= True,
            spacing = 10,
            child = network.wifi.devices[0].bind("access_points", transform= lambda aps: self.get_wifi_list(aps))
        )

    
    def create_ap_widget(self, ap):

        def update_ap(password):
            ap.psk = password
            asyncio.create_task(ap.connect_to(password))

        password_entry = widgets.Entry(
            placeholder_text="Enter password...",
            on_accept=lambda password: update_ap(password.text),
        )
        password_box = widgets.Box(child=[
            password_entry,
        ])
        password_entry.text = ap.psk

        # Initially hidden container
        password_dropdown = widgets.Revealer(
            visible=True,
            reveal_child=False,
            child=password_box
        )

        # Wrapper so `child` passed to DropDownPanel is correct
        password_wrapper = widgets.Box(child=[password_dropdown])

        def show_password_field(x):
            password_dropdown.reveal_child = not password_dropdown.reveal_child

        return DropDownPanel(
            ap.ssid,
            [
                ("Connect", update_ap),
                ("Edit", show_password_field),
                ("Disconnect", lambda x: asyncio.create_task(ap.disconnect_from())),  # Placeholder
                ("Forget", lambda x: asyncio.create_task(ap.forget()))  # Placeholder
            ],
            child=password_wrapper,
            vertical=True,
        )

    def get_wifi_list(self, aps):
        aps = list({ap.ssid: ap for ap in aps if ap.ssid}.values())
        return [self.create_ap_widget(ap) for ap in aps]



class BluetoothModule(widgets.Box):
    def __init__(self):
        enablebuttons = widgets.Box(spacing=10, child=[
            widgets.Label(label=bluetooth.bind("state")),
            widgets.Button(
                label=bluetooth.bind(
                    "powered", transform=lambda powered: "Powered: On" if powered else "Powered: Off"
                ),
                on_click=lambda x: bluetooth.set_powered(not bluetooth.powered)
            ),
            widgets.Button(
                label=bluetooth.bind(
                    "setup_mode", transform=lambda scanning: "Scanning: On" if scanning else "Scanning: Off"
                ),
                on_click=lambda x: bluetooth.set_setup_mode(not bluetooth.setup_mode)
            ),
        ])

        super().__init__(
            vertical=True,
            spacing=10,
            child=[
                enablebuttons,
                widgets.Box(
                    vertical=True,
                    spacing=5,
                    child=bluetooth.bind(
                        "devices",
                        transform=self.get_bluetooth_list
                    )
                )
            ]
        )

    def get_bluetooth_list(self, devices):
        print("Device list updated:", devices)
        return [self.create_device_widget(device) for device in devices]

    def create_device_widget(self, device):
        device_name = getattr(device, 'name', 'Unknown Device')
        information_text = f'{"Trusted" if device.trusted else ""}{"Connected " if device.connected else ""} {str(device.battery_level) if device.battery_level else ""}'

        return DropDownPanel(

             widgets.Box(spacing=10, child=[
                widgets.Icon(image=device.icon_name),
                widgets.Box(vertical=True,child=[
                    widgets.Label(label=device.name),
                    widgets.Label(label=information_text)
                ])

            ]),
            [
                ("Connect", lambda x: asyncio.create_task(device.connect_to())),
                ("Disconnect", lambda x: asyncio.create_task(device.disconnect_from())),
            ],
            vertical=True,
        )



class Brightness(widgets.Box):
    def __init__(self):
        super().__init__(
            visible=backlight.bind("available"),
            hexpand=True,
            style="margin-top: 0.25rem;",
            child=[
                    widgets.Button(

                        child = widgets.Icon(
                        image="display-brightness-symbolic",
                        pixel_size=18,
                    )
                ),
                widgets.Scale(
                    min=0,
                    max=backlight.max_brightness,
                    hexpand=True,
                    value=backlight.bind("brightness"),
                    css_classes=["material-slider"],
                    on_change=lambda x: asyncio.create_task(backlight.set_brightness_async(x.value)),
                ),
            ],
        )


class Brightness(widgets.Box):
    def __init__(self):
        super().__init__(
            visible=backlight.bind("available"),
            hexpand=True,
            style="margin-top: 0.25rem;",
            child=[
                    widgets.Button(

                        child = widgets.Icon(
                        image="display-brightness-symbolic",
                        pixel_size=18,
                    )
                ),
                widgets.Scale(
                    min=0,
                    max=backlight.max_brightness,
                    hexpand=True,
                    value=backlight.bind("brightness"),
                    css_classes=["material-slider"],
                    on_change=lambda x: asyncio.create_task(backlight.set_brightness_async(x.value)),
                ),
            ],
        )

class ToggleBox(widgets.Box):
    def __init__(
        self,
        label: str = "DEFAULT",
        active: bool = True,
        on_change =  lambda x: (),
        css_classes: list[str] = [],
        **kwargs,
    ):
        super().__init__(
            child=[
                widgets.Label(label=label),
                widgets.Switch(
                    halign="end",
                    hexpand=True,
                    active=active,
                    on_change=on_change,
                ),
            ],
            css_classes=["toggle-box"] + css_classes,
        )

class Volume(widgets.Box):
    def __init__(self):
        stream = audio.speaker
        super().__init__(
            visible=backlight.bind("available"),
            hexpand=True,
            style="margin-top: 0.25rem;",
            child=[
                widgets.Button(child= widgets.Icon(
                    image=stream.bind("icon_name"),
                    pixel_size=18,
                ),on_click= lambda x: stream.set_is_muted(not stream.is_muted)
                ),
                widgets.Scale(
                    step=1,
                    min=0,
                    max=100,
                    hexpand=True,
                    value=stream.bind("volume"),
                    css_classes=["material-slider"],
                    on_change=lambda x: stream.set_volume(x.value), 
                ),
            ],
        )

class Microphone(widgets.Box):
    def __init__(self):
        stream = audio.microphone
        super().__init__(
            visible=backlight.bind("available"),
            hexpand=True,
            style="margin-top: 0.25rem;",
            child=[
                widgets.Button(child= widgets.Icon(
                    image=stream.bind("icon_name"),
                    pixel_size=18,
                ),on_click= lambda x: stream.set_is_muted(not stream.is_muted)
                ),
                widgets.Scale(
                    step=1,
                    min=0,
                    max=100,
                    hexpand=True,
                    value=stream.bind("volume"),
                    css_classes=["material-slider"],
                    on_change=lambda x: stream.set_volume(x.value), 
                ),
            ],
        )

class WallpaperMenu(widgets.Box):
    def __init__(self):
        super().__init__(
            vertical=True,
            child=[
                widgets.Button(label="Next wallpaper", on_click=lambda x:subprocess.run(['wpaperctl', 'next'])),
                widgets.FileChooserButton(
                    dialog=widgets.FileDialog(
                        on_file_set=lambda self, file: print(file.get_path()),
                        initial_path=os.path.expanduser("~/Downloads/"),
                        filters=[
                            widgets.FileFilter(
                                mime_types=["image/jpeg", "image/png"],
                                default=True,
                                name="Images JPEG/PNG",
                            )
                        ]
                    ),
                    label=widgets.Label(label="Select a wallpaper"),

)
            ]
        )



class ThemingMenu(widgets.Box):
    def __init__(self):
        # Available color schemes and themes
        self.color_schemes = {
            "Rose Pine": "rose_pine",
            "Catppuccin Mocha": "catppuccin_mocha",
            "Gruvbox Dark": "gruvbox_dark",
            "Nord": "nord"
        }

        self.theme_manager = ThemeManager() 

        self.selected_scheme_key = None
        self.selected_theme = None

        scheme_buttons = [
            (key, lambda _, key=key: self.set_scheme(key)) 
                for (key,name) in self.theme_manager.get_color_options()
        ]
        print(scheme_buttons)

        theme_buttons = [
            (theme, lambda _, t=theme: self.set_theme(t))
            for theme in self.theme_manager.get_themes()
        ]

        # Display labels showing current selections
        self.theme_label = widgets.Label(label=f"None")
        self.scheme_label = widgets.Label(label=f"None")

        scheme_dropdown = DropDownPanel("Select Color Scheme", scheme_buttons, vertical=True)
        theme_dropdown = DropDownPanel("Select Theme", theme_buttons,vertical=True)

        super().__init__(
            vertical=True,
            spacing=10,
            child=[
                scheme_dropdown,
                self.scheme_label,
                theme_dropdown,
                self.theme_label,
            ]
        )

    def set_scheme(self, scheme_key):
        self.selected_scheme_key = scheme_key
        self.scheme_label.label = f"Selected Scheme: {scheme_key}"
        self.theme_manager.update_colors(scheme_key)

    def set_theme(self, theme):
        self.selected_theme = theme
        self.theme_label.label = f"Selected Theme: {theme}"
        self.theme_manager.update_theme(theme)


        



class SidebarMenu(widgets.Box):
    def __init__(self, menus):
        self.menu_names = [name for name, _ in menus]
        self.buttons = []
        self.stack = widgets.Stack(transition_type="SLIDE_UP_DOWN")
        for name, content in menus:
            self.stack.add_named(content, name)
        if menus:
            self.stack.set_visible_child_name(self.menu_names[0])
        
        for i, (name, _) in enumerate(menus):
            btn = widgets.Button(

                child=widgets.Label(label=name),
                css_classes=["sidebarentry"],
                on_click=lambda x, n=name: self.switch(n)
            )
            self.buttons.append(btn)
            if i == 0:
                btn.add_css_class("active")
                btn.add_css_class("first-child")
            if i == len(menus) -1:
                btn.add_css_class("last-child")
        
        super().__init__(
            orientation="horizontal",
            spacing=10,
            child=[
                widgets.Box(css_classes=["sidebar"], orientation="vertical", child=self.buttons),
                self.stack
            ]
        )
    
    def switch(self, name):
        for btn in self.buttons: btn.remove_css_class("active")
        self.buttons[self.menu_names.index(name)].add_css_class("active")
        self.stack.set_visible_child_name(name)

    def show(self, name: str):
        if name in self.menu_names:
            self.switch(name)

class PowerMenu(DropDownPanel):
    def __init__(self):
        menu = widgets.Box(vertical=True, child=[
            self.create_confirm_panel("Shutdown", self.shutdown),
            self.create_confirm_panel("Restart", self.restart),
            self.create_confirm_panel("Logout", self.logout),
        ])
        super().__init__("Power", [], child=menu)

    def create_confirm_panel(self, label: str, action_callback):
        panel = DropDownPanel(label,[ 
            ("Confirm", lambda x: action_callback()),
            ("Cancel", lambda x: self.toggle())
        ])

        return panel

    def shutdown(self):
        subprocess.run(["systemctl", "poweroff"])

    def restart(self):
        subprocess.run(["systemctl", "reboot"])

    def logout(self):
        subprocess.run(["niri", "msg", "action", "quit"])




class SystemMenu(widgets.Box):
    def __init__(self):
        self.fetch = FetchService.get_default()

        super().__init__(
            vertical=True,
            spacing=12,
            child=[
                widgets.Label(label="System"),
                self._kv_row(
                    icon_name="computer-symbolic",
                    key_text="CPU",
                    value_widget=widgets.Label(label=self.fetch.bind("cpu")),
                ),
                self._kv_row(
                    icon_name="temperature-symbolic",
                    key_text="Temp",
                    value_widget=widgets.Label(label=self.fetch.bind("cpu_temp", transform=lambda t: f"{t:.0f}°C" if t else "—")),
                ),
                self._kv_row(
                    icon_name="alarm-symbolic",
                    key_text="Uptime",
                    value_widget=widgets.Label(label=utils.Poll(1_000, lambda _: self._format_uptime(self.fetch.uptime)).bind("output")),
                ),
                self._kv_row(
                    icon_name="drive-harddisk-symbolic",
                    key_text="Memory",
                    value_widget=widgets.Label(label=utils.Poll(1_000, lambda _: self._format_mem_line_from_service()).bind("output")),
                ),
                widgets.Separator(),
                PowerMenu(),
            ]
        )

    def _format_gib(self, megabytes: int) -> str:
        try:
            gib = float(megabytes) / 1024.0
            return f"{gib:.1f} GiB"
        except Exception:
            return "N/A"

    def _format_uptime(self, uptime_tuple) -> str:
        try:
            days, hours, minutes, seconds = uptime_tuple
            parts = []
            if days:
                parts.append(f"{days}d")
            parts.append(f"{hours}h")
            parts.append(f"{minutes}m")
            return " ".join(parts)
        except Exception:
            return "N/A"

    def _kv_row(self, *, icon_name: str, key_text: str, value_widget):
        return widgets.Box(
            spacing=10,
            child=[
                widgets.Icon(image=icon_name, pixel_size=16),
                widgets.Label(label=key_text, hexpand=True),
                value_widget,
            ]
        )

    def _format_mem_line_from_service(self) -> str:
        try:
            total = getattr(self.fetch, 'mem_total', 0) or 0
            available = getattr(self.fetch, 'mem_available', 0) or 0
            used = getattr(self.fetch, 'mem_used', None)
            if used is None:
                used = max(total - available, 0)
            return f"{self._format_gib(used)} / {self._format_gib(total)} ({self._format_gib(available)} free)"
        except Exception:
            return "N/A"


class PomodoroMenu(widgets.Box):
    def __init__(self):
        self.duration_minutes = 25
        self.duration_seconds = self.duration_minutes * 60
        self.started_at_s: float | None = None
        self.running = False

        # Poll calculates remaining from started_at and duration
        def tick(_):
            remaining = self._compute_remaining()
            if hasattr(self, "status_label"):
                self.status_label.label = self._format_time(remaining)
            if self.running and remaining == 0:
                self.running = False
                self.started_at_s = None
                self._remove_status_icon()
            return self._format_time(remaining)

        self.display = widgets.Label(label=utils.Poll(1_000, tick).bind("output"))

        def start(_):
            if not self.running:
                self.running = True
                self.started_at_s = time.monotonic()
                self._ensure_status_icon()

        def pause(_):
            if self.running:
                # freeze remaining by converting current remaining into new duration baseline
                remaining = self._compute_remaining()
                self.duration_seconds = remaining
                self.started_at_s = None
                self.running = False

        def reset(_):
            self.running = False
            self.started_at_s = None
            # reset to preset minutes
            self.duration_seconds = self.duration_minutes * 60
            if hasattr(self, "status_label"):
                self.status_label.label = self._format_time(self.duration_seconds)
            self._remove_status_icon()

        def set_minutes(mins):
            self.duration_minutes = mins
            self.duration_seconds = mins * 60
            if hasattr(self, "status_label"):
                self.status_label.label = self._format_time(self.duration_seconds)

        super().__init__(
            vertical=True,
            spacing=10,
            child=[
                widgets.Label(label="Pomodoro"),
                self.display,
                widgets.Box(spacing=10, child=[
                    widgets.Button(label="Start", on_click=start),
                    widgets.Button(label="Pause", on_click=pause),
                    widgets.Button(label="Reset", on_click=reset),
                ]),
                widgets.Box(spacing=10, child=[
                    widgets.Button(label="60m", on_click=lambda x: set_minutes(60)),
                    widgets.Button(label="25m", on_click=lambda x: set_minutes(25)),
                    widgets.Entry(placeholder_text="Custom minutes", on_accept=lambda entry: self._set_minutes_from_entry(entry.text)),
                ])
            ]
        )

    def _set_minutes_from_entry(self, text: str):
        try:
            mins = int(text)
            if mins > 0:
                self.duration_minutes = mins
                self.duration_seconds = mins * 60
        except Exception:
            pass

    def _format_time(self, seconds: int) -> str:
        m = seconds // 60
        s = seconds % 60
        return f"{m:02d}:{s:02d}"

    def _compute_remaining(self) -> int:
        # If not running, remaining equals duration_seconds baseline
        if not self.running:
            return max(int(self.duration_seconds), 0)
        try:
            now = time.monotonic()
            start = self.started_at_s or now
            elapsed_s = now - start
            remaining = int(self.duration_seconds - elapsed_s)
            return max(remaining, 0)
        except Exception:
            return max(int(self.duration_seconds), 0)

    def _ensure_status_icon(self):
        if not hasattr(self, "status_widget"):
            self.status_label = widgets.Label(label=self._format_time(self._compute_remaining()))
            self.status_widget = widgets.Button(
                css_classes=["panel"],
                on_click=lambda x: open_menu("Utils"),
                child=widgets.Box(spacing=5, child=[
                    widgets.Icon(image="alarm-symbolic", pixel_size=16),
                    self.status_label
                ])
            )
            public_status_box.append(self.status_widget)

    def _remove_status_icon(self):
        if hasattr(self, "status_widget"):
            self.status_widget.unparent()
            del self.status_widget
        if hasattr(self, "status_label"):
            del self.status_label


class ControlCenter(PopupWindow):
    def __init__(self):

        self.sidebar = None
        menus = [
            ("Network", WifiModule()), 
            ("Bluetooth",  BluetoothModule()),
            ("Wallpaper",WallpaperMenu()),
            ("Theme", ThemingMenu()),
            ("Utils", PomodoroMenu()),
            ("System", SystemMenu()),
        ]
        self.sidebar = SidebarMenu(menus)

        contents = widgets.Box(

            vertical=True,
            spacing=15,
            child=[
                widgets.Box(child= [
                
                widgets.Label(css_classes=["title"],label='Settings'),
                ]),
                Brightness(),
                # Volume(),
                # Microphone(),
                self.sidebar,
        ])

        super().__init__(
            child=widgets.Box(css_classes=["controlcenter"], child=[contents]),
            namespace="ControlCenter",
            popup=True,
            kb_mode="exclusive",
            layer="top",
            anchor=["top", "left"],
            background_color=None,
        )

    def OpenMenu(self, name: str):
        self.visible = True
        if self.sidebar:
            self.sidebar.show(name)



