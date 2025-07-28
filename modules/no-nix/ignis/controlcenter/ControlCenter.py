from ignis import widgets
from ignis.services.network import NetworkService
import asyncio
from ignis.services.backlight import BacklightService

network = NetworkService.get_default()
backlight = BacklightService.get_default()
        
        



class WifiModule(widgets.Box):
    # List access points

    def __init__(self):
        # all networks 
        super().__init__(
            vertical= True,
            child = network.wifi.devices[0].bind("access_points", transform= lambda aps: self.get_wifi_list(aps))
        )


    def get_wifi_list(self, aps):
        return [widgets.Label(label=ap.ssid) for ap in aps]

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

class DropDownPanel(widgets.Button):
    def __init__(self):
        revealer = widgets.Revealer(
                visible=False,
                child=widgets.Label(label='animation!!!'),
                transition_type='slide_right',
                transition_duration=500,
                reveal_child=True, )
        super().__init__(label="UWU", child=revealer, on_click=lambda x: setattr(revealer, 'visible', True))


class DropdownPanelMenu(widgets.Box):
    def __init__(self):

        dropdown = [ToggleBox(),DropDownPanel()]
        super().__init__(child=dropdown)


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
                    ("TEST",DropdownPanelMenu()),
                    ("Network", WifiModule()), 
                    ("Bluetooth", widgets.Label(label="TEST2")), 
                    ("AI", widgets.Label(label="TEST2")),
                    ("EnvKeys", widgets.Label(label="ENVKEYS")),
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


