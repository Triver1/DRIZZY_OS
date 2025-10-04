from PIL import Image, ImageFilter
from ignis import widgets
from ignis import utils
import datetime
from ignis.command_manager import CommandManager
from .components.Notch import Notch
from .components.EventNotch import EventNotch
from .menus.NotchHoverPane import HoverPane
from .menus.QuickMenu import QuickMenu
from .menus.AppLauncher import AppLauncher
from .menus.WallpaperPicker import WallpaperPicker
from .menus.PowerMenu import PowerMenu
from .menus.NotificationMenu import NotificationMenu, NotificationNotch
from gi.repository import GLib
from ignis.services.notifications import NotificationService
from gi.repository import Gdk
from gi.repository import Gtk
import sys
import os
sys.path.append(os.path.join(os.path.dirname(__file__), '..'))
from widgets.Battery import Battery
from widgets.Workspaces import Workspaces
from widgets.Wifi import Wifi

class NotchContents(widgets.Box):
    def __init__(self, *, child=None, css_classes=None, **kwargs):
        super().__init__(
            spacing=8,
            child=[
                widgets.Label(
                    label=utils.Poll(
                        1_000,
                        lambda self: datetime.datetime.now().strftime("%H:%M"),
                    ).bind("output"),
                ),
                Battery(),
            ]
        )


class Bar:
    def __init__(self, monitor: int):
        # --- Command Manager Setup ---
        self.command_manager = CommandManager.get_default()
        self.app_launcher_mode = False  # Track if app launcher was opened via command
        
        # --- Invisible exclusive window to reserve space ---
        self.reserver = widgets.Window(
            namespace=f"bar-reserve-{monitor}",
            css_classes=["bar-reserve"],
            monitor=monitor,
            anchor=["left", "top", "right"],
            exclusivity="exclusive",  # reserves space
        )

        # --- Actual content window (ignores exclusivity) ---
        
        # --- Create the 3 pages for the center notch ---
        # Page 1: Default - clock and battery
        default_page = widgets.Box(
            spacing=6,
            child=[
                NotchContents(),
            ],
        )
        
        # Page 2: Hover - random text
        hover_page = QuickMenu()
        
        # Page 3: App Launcher
        app_launcher_page = AppLauncher()
        # Page 4: Wallpaper Picker
        wallpaper_picker_page = WallpaperPicker()
        # Page 5: Power Menu
        power_menu_page = PowerMenu()
        # Page 6: Notification Menu
        notification_menu_page = NotificationMenu()
        # Page 7: Notification Notch (compact view)
        notification_notch_page = NotificationNotch()

        # Create hover pane with all pages
        self.hover_pane = HoverPane(pages=[
            default_page,         # Page 0: Default
            hover_page,           # Page 1: Hover
            app_launcher_page,    # Page 2: App Launcher
            wallpaper_picker_page,# Page 3: Wallpaper Picker
            power_menu_page,      # Page 4: Power Menu
            notification_menu_page, # Page 5: Notification Menu
            notification_notch_page # Page 6: Notification Notch
        ])

        # Add commands for app launcher
        self.command_manager.add_command(
            "open-app-launcher", 
            lambda *_: self.open_app_launcher()
        )
        self.command_manager.add_command(
            "close-app-launcher", 
            lambda *_: self.close_app_launcher()
        )

        # Add commands for wallpaper picker
        self.command_manager.add_command(
            "wallpaper_pick",
            lambda *_: self.open_wallpaper_picker()
        )
        self.command_manager.add_command(
            "close-wallpaper-picker",
            lambda *_: self.close_wallpaper_picker()
        )

        # Add commands for power menu
        self.command_manager.add_command(
            "power_menu",
            lambda *_: self.open_power_menu()
        )
        self.command_manager.add_command(
            "close-power-menu",
            lambda *_: self.close_power_menu()
        )

        # Add commands for notification menu (both snake_case and kebab-case aliases)
        self.command_manager.add_command(
            "notification_menu",
            lambda *_: self.open_notification_menu()
        )
        self.command_manager.add_command(
            "notification-menu",
            lambda *_: self.open_notification_menu()
        )
        self.command_manager.add_command(
            "close-notification-menu",
            lambda *_: self.close_notification_menu()
        )

        # Switch to NotificationNotch on new popup, then auto-hide after delay
        try:
            self._notification_service = NotificationService.get_default()
            if self._notification_service:
                self._notification_service.connect("new_popup", lambda *_args: self._on_new_notification_popup())
        except Exception:
            self._notification_service = None

        center_box = widgets.Box(spacing=10, child=[
            EventNotch(
                child=[self.hover_pane],
                orientation="top",
                on_hover=lambda *args: self.on_notch_hover(),
                on_hover_lost=lambda *args: self.on_notch_hover_lost(),
            )
        ])

        # Add Escape handling to the notch area to return to default page
        key_controller = Gtk.EventControllerKey()
        key_controller.connect("key-pressed", self._on_key_pressed)
        center_box.add_controller(key_controller)

        left_box = widgets.Box(
            child=[
                widgets.Box(
                    vexpand=False,
                    valign="start",
                    spacing=8,
                    css_classes=["bar_widget"],
                    child=[widget]
                )
                for widget in [
                    Workspaces(monitor),
                ]
            ]
        )


        right_box = widgets.Box(
            child=[
                widgets.Box(
                    vexpand=False,
                    valign="start",
                    spacing=8,
                    css_classes=["bar_widget"],
                    child=[widget]
                )
                for widget in [
                    Wifi()
                ]
            ]
        )

        self.window = widgets.Window(
            namespace=f"bar-{monitor}",
            css_classes=["bar"],
            monitor=monitor,
            anchor=["left", "top", "right"],
            exclusivity="ignore",  # does not reserve space
            layer="overlay",
            kb_mode="on_demand",  # Enable keyboard input for text fields
            child=widgets.CenterBox(
                start_widget=left_box,
                center_widget=center_box,
                end_widget=right_box,
            ),
        )
    
    def open_app_launcher(self):
        """Open app launcher via command and prevent hover interference"""
        self.app_launcher_mode = True
        self.hover_pane.show_page_index(2)
        # Focus the search field
        if hasattr(self.hover_pane.pages[2], 'focus_search'):
            self.hover_pane.pages[2].focus_search()
    
    def on_notch_hover(self):
        """Handle hover over notch - only show hover page if not in app launcher mode"""
        if not self.app_launcher_mode:
            self.hover_pane.show_page_index(1)
    
    def on_notch_hover_lost(self):
        """Handle hover lost - only return to default if not in app launcher mode"""
        if not self.app_launcher_mode:
            self.hover_pane.show_default_page()
        # If we're in app launcher mode, we need to detect if the user really wants to close it
        # For now, add a click outside or escape key to close it
    
    def close_app_launcher(self):
        """Close app launcher and return to default page"""
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def open_wallpaper_picker(self):
        """Open wallpaper picker via command and prevent hover interference"""
        self.app_launcher_mode = True
        self.hover_pane.show_page_index(3)

    def close_wallpaper_picker(self):
        """Close wallpaper picker and return to default page"""
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def open_power_menu(self):
        """Open power menu via command and prevent hover interference"""
        self.app_launcher_mode = True
        self.hover_pane.show_page_index(4)

    def close_power_menu(self):
        """Close power menu and return to default page"""
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def open_notification_menu(self):
        """Open notification menu via command and prevent hover interference"""
        self.app_launcher_mode = True
        self.hover_pane.show_page_index(5)
        if hasattr(self.hover_pane.pages[5], 'focus_search'):
            self.hover_pane.pages[5].focus_search()

    def close_notification_menu(self):
        """Close notification menu and return to default page"""
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def _on_new_notification_popup(self):
        try:
            # Show compact notifications-only notch page
            self.hover_pane.show_page_index(6)
            # Auto-hide after 4 seconds
            GLib.timeout_add(4000, self._auto_hide_notification_notch)
        except Exception:
            pass

    def _auto_hide_notification_notch(self):
        try:
            # If still on the notifications notch, go back to default
            self.hover_pane.show_default_page()
        except Exception:
            pass
        return False  # stop the timeout

    def _on_key_pressed(self, controller, keyval, keycode, state):
        if keyval == Gdk.KEY_Escape:
            # Always go back to default when Escape is pressed within the notch
            self.app_launcher_mode = False
            self.hover_pane.show_default_page()
            return True
        return False
