import os
import subprocess
from ignis import widgets
from ignis.services.network import NetworkService
import asyncio
from ignis.services.backlight import BacklightService

network = NetworkService.get_default()
backlight = BacklightService.get_default()
        
        


class DropDownPanel(widgets.Box):
    def __init__(self,label,revealercontent):
        self.revealed = False
        revealer = widgets.Revealer(
                visible=True,
                child=revealercontent,
                transition_type='slide_down',
                transition_duration=300,
                reveal_child=False, )
        arrow = widgets.Arrow(
            pixel_size=20,
            rotated=False,
            degree=90,
            time=135,
            direction="right",
            counterclockwise=False,
        )
        button = widgets.Button(child=widgets.Box(child=[label,arrow]), on_click=lambda x: self.toggle())
        super().__init__(child=[button,revealer], vertical=True)
        self.revealer = revealer
        self.arrow = arrow

    def toggle(self):
        self.revealed = not self.revealed
        self.revealer.reveal_child = self.revealed
        self.arrow.rotated = self.revealed

class WifiModule(widgets.Box):
    # List access points

    def __init__(self):
        # all networks 
        super().__init__(
            vertical= True,
            child = network.wifi.devices[0].bind("access_points", transform= lambda aps: self.get_wifi_list(aps))
        )


    def create_ap_widget(self, ap):
        label = widgets.Label(label=ap.ssid)
        content = widgets.Box(child=[widgets.Button(label="Connect"),widgets.Button(label="Forget")])
        return DropDownPanel(label,content)

    def get_wifi_list(self, aps):
        return [self.create_ap_widget(ap) for ap in aps]

class Brightness(widgets.Box):
    def __init__(self):
        super().__init__(
            visible=backlight.bind("available"),
            hexpand=True,
            style="margin-top: 0.25rem;",
            child=[
                widgets.Icon(
                    image="display-brightness-symbolic",
                    pixel_size=18,
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
        super().__init__(
            visible=backlight.bind("available"),
            hexpand=True,
            style="margin-top: 0.25rem;",
            child=[
                widgets.Icon(
                    image="audio-volume-medium-symbolic",
                    pixel_size=18,
                ),
                widgets.Scale(
                    min=0,
                    # max=backlight.max_brightness,
                    hexpand=True,
                    # value=backlight.bind("brightness"),
                    css_classes=["material-slider"],
                    # on_change=lambda x: asyncio.create_task(backlight.set_brightness_async(x.value)),
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
        label = widgets.Label(label="Power")
        menu = widgets.Box(vertical=True, child=[
            self.create_confirm_panel("Shutdown", self.shutdown),
            self.create_confirm_panel("Restart", self.restart),
            self.create_confirm_panel("Logout", self.logout),
        ])
        super().__init__(label, menu)

    def create_confirm_panel(self, label: str, action_callback):
        confirm_buttons = widgets.Box(
            spacing=10,
            child=[
                widgets.Button(label="Sure", on_click=lambda x: action_callback()),
                widgets.Button(label="Cancel", on_click=lambda x: panel.toggle()),
            ]
        )

        panel_label = widgets.Label(label=label)
        panel = DropDownPanel(panel_label, confirm_buttons)
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
                widgets.Label(css_classes=["title"],label='Quicksettings'),
                Brightness(),
                Volume(),
                SidebarMenu([
                    ("Network", WifiModule()), 
                    ("Bluetooth", widgets.Label(label="TEST2")), 
                    ("AI", widgets.Label(label="TEST2")),
                    ("EnvKeys", widgets.Label(label="ENVKEYS")),
                    ("Wallpaper",WallpaperMenu()),
                    ("Powermenu", PowerMenu())
                ]),
        ])

        revealer = widgets.Revealer(
            child=contents,
            transition_type='slide_left',
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

    def reveal(self):
        self.visible = True

    def hide(self):
        self.visible = False


