import getpass
import socket
from ignis import widgets
from widgets.QuickMenuPage import QuickMenuPage
from widgets.OptionButton import OptionButton
from ignis.command_manager import CommandManager
from bar.menus.quickmenupages.WifiPage import WifiButton, WifiPage
from bar.menus.quickmenupages.BluetoothPage import BluetoothButton, BluetoothPage
from bar.menus.quickmenupages.AudioPage import AudioButton, AudioPage
from bar.menus.quickmenupages.BrightnessPage import BrightnessButton, BrightnessPage


class QuickMenuRow(widgets.Box):
    def __init__(self):
        self.open_page = "empty"
        self.current_items = 0
        self.items = widgets.Box(spacing=10, child=[])
        self.extend_stack = widgets.Stack(
            hhomogeneous=False,
            vhomogeneous=False,
            interpolate_size=True,
            transition_type="crossfade",
            hexpand=True,
            halign="fill"
        )
        self.extend_stack.add_named(widgets.Box(), "empty")
        super().__init__( child=[self.items, self.extend_stack], vertical=True, hexpand=True)

    def _open_page(self, name):
        if name == self.open_page:
            self.extend_stack.set_visible_child_name("empty")
            self.open_page = "empty"
            return
        self.open_page = name
        self.extend_stack.set_visible_child_name(name)


    # page widget should be of button type !
    def add_item(self, page_widget, extend_page_widget, title: str):
        self.current_items += 1
        name = str(self.current_items)
        wrapped = QuickMenuPage(title=title, body_child=extend_page_widget)
        self.extend_stack.add_named(child=wrapped, name=name)
        self.items.append(page_widget)
        page_widget.on_click = lambda *_: self._open_page(name)
    
    def add_item_no_button(self, page_widget, button_object, extend_page_widget, title: str):
        self.current_items += 1
        name = str(self.current_items)
        wrapped = QuickMenuPage(title=title, body_child=extend_page_widget)
        self.extend_stack.add_named(child=wrapped, name=name)
        self.items.append(page_widget)
        button_object.on_click = lambda *_: self._open_page(name)

    # simple action button without an extend page
    def add_action(self, action_button):
        self.items.append(action_button)

class QuickMenuButton(widgets.Button):
    def __init__(self, label):
        super().__init__(on_click=None, label=label)  # no return!
class WallpaperActionButton(widgets.Button):
    def __init__(self):
        command_manager = CommandManager.get_default()
        super().__init__(label="Wallpaper", on_click=lambda *_: command_manager.run_command("wallpaper_pick"))





class PowerMenuActionButton(widgets.Button):
    def __init__(self):
        command_manager = CommandManager.get_default()
        super().__init__(
            child=widgets.Icon(image="system-shutdown-symbolic"),
            on_click=lambda *_: command_manager.run_command("power_menu")
        )


class NotificationMenuActionButton(widgets.Button):
    def __init__(self):
        command_manager = CommandManager.get_default()
        super().__init__(
            child=widgets.Icon(image="preferences-system-notifications-symbolic"),
            on_click=lambda *_: command_manager.run_command("notification_menu")
        )


class QuickMenu(widgets.Box):
    def __init__(self):
        # Get username and hostname
        username = getpass.getuser()
        hostname = socket.gethostname()
        
        top_box = widgets.Box(child=[
            NotificationMenuActionButton(),
            widgets.Box(hexpand=True),  # spacer
            widgets.Label(
                label=f"{username}@{hostname}",
                css_classes=["user-hostname-label"]
            ),
            widgets.Box(hexpand=True),  # spacer
            
            PowerMenuActionButton(),
        ])
        # Audio/Brightness row - controls for audio and brightness
        controls_row = QuickMenuRow()
        
        # Audio controls
        audio_button = AudioButton()
        audio_page = AudioPage()
        controls_row.add_item_no_button(audio_button, audio_button.icon_button, audio_page, "Audio")
        
        # Brightness controls
        brightness_button = BrightnessButton()
        brightness_page = BrightnessPage()
        controls_row.add_item_no_button(brightness_button, brightness_button.icon_button, brightness_page, "Brightness")
        
        # Main controls row
        row1 = QuickMenuRow()
        row1.add_item(WifiButton(), WifiPage(), "Wifi")
        row1.add_item(BluetoothButton(), BluetoothPage(), "Bluetooth")
        row1.add_action(WallpaperActionButton())

        super().__init__(
            child=[
                top_box,
                controls_row,
                row1
            ],
            vertical=True,
            spacing=8,
            vexpand=False,
            hexpand=True,
            valign="start",
        )
