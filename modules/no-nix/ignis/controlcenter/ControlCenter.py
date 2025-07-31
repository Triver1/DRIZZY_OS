import os
import subprocess
from ignis import widgets
from ignis.services.network import NetworkService
import asyncio
from ignis.services.backlight import BacklightService
from ignis.services.bluetooth import BluetoothService
from ignis.services.audio import AudioService

audio = AudioService.get_default()
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




class ControlCenter(widgets.RevealerWindow):
    def __init__(self):

        contents = widgets.Box(

            vertical=True,
            spacing=15,
            child=[
                widgets.Box(child= [
                
                widgets.Label(css_classes=["title"],label='Settings'),
                ]),
                Brightness(),
                Volume(),
                Microphone(),
                SidebarMenu([
                    ("Network", WifiModule()), 
                    ("Bluetooth",  BluetoothModule()),
                    ("Wallpaper",WallpaperMenu()),
                    ("Powermenu", PowerMenu()),
                    # ("AI", widgets.Label(label="TEST2")),
                    # ("EnvKeys", widgets.Label(label="ENVKEYS")),
                ]),
        ])

        revealer = widgets.Revealer(
            child=contents,
            transition_type='slide_down',
            transition_duration=250,
            reveal_child=True, # Whether child is revealed.
)
        super().__init__(
            css_classes=["controlcenter"],
            namespace="ControlCenter",
            visible=False,  
            popup=True,
            kb_mode="on_demand",
            layer="top",
            anchor=["top", "left"],
            revealer=revealer,  # Added the revealer as child
            child=widgets.Box(
                child=[revealer])
        )
        self.revealer = revealer

    def toggle_reveal(self):
        self.visible = not self.visible



