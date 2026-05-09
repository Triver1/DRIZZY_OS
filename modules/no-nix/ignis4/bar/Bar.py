from PIL import Image, ImageFilter
from ignis import widgets
from ignis import utils
import datetime
from ignis.command_manager import CommandManager
from .components.Notch import Notch
from .components.EventNotch import EventNotch
from .menus.NotchHoverPane import HoverPane
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
from widgets.MangoWorkspaces import MangoWorkspaces
from services.todo_service import get_todo_service
from windows.MenuPane import MenuPane


class TodoTimerWidget(widgets.Box):
    def __init__(self):
        self.svc = get_todo_service()
        self._timer_label = widgets.Label(
            label="",
            css_classes=["todo-bar-timer"],
        )
        super().__init__(
            orientation="vertical",
            halign="center",
            child=[self._timer_label],
        )
        self.svc.add_listener(self._on_update)
        self._on_update()

    def _on_update(self):
        if self.svc.timer_active:
            self._timer_label.label = self.svc.get_timer_display()
            self.visible = True
        else:
            self.visible = False


class NotchContents(widgets.Box):
    def __init__(self, monitor, *, child=None, css_classes=None, **kwargs):
        super().__init__(
            orientation="vertical",
            spacing=8,
            halign="center",
            child=[
                widgets.Box(
                    orientation="vertical",
                    spacing=2,
                    halign="center",
                    child=[
                        widgets.Label(
                            label=utils.Poll(
                                1_000,
                                lambda self: datetime.datetime.now().strftime("%H"),
                            ).bind("output"),
                            css_classes=["clock-hour"]
                        ),
                        widgets.Label(
                            label=utils.Poll(
                                1_000,
                                lambda self: datetime.datetime.now().strftime("%M"),
                            ).bind("output"),
                            css_classes=["clock-minute"]
                        ),
                    ]
                ),
                TodoTimerWidget(),
                Battery(),
                Wifi(),
                Workspaces(),
            ]
        )


class Bar:
    def __init__(self, monitor: int):
        # --- Command Manager Setup ---
        self.command_manager = CommandManager.get_default()
        self.app_launcher_mode = False

        # --- Invisible exclusive window to reserve space ---
        self.reserver = widgets.Window(
            namespace=f"bar-reserve-{monitor}",
            css_classes=["bar-reserve"],
            monitor=monitor,
            anchor=["left", "top", "bottom"],
            exclusivity="exclusive",
        )

        # --- Notch pages (for command-triggered menus) ---
        default_page = widgets.Box(
            orientation="vertical",
            spacing=6,
            child=[NotchContents(monitor)],
        )

        app_launcher_page = AppLauncher()
        wallpaper_picker_page = WallpaperPicker()
        power_menu_page = PowerMenu()
        notification_menu_page = NotificationMenu()
        notification_notch_page = NotificationNotch()

        self.hover_pane = HoverPane(pages=[
            default_page,           # Page 0: Default
            app_launcher_page,      # Page 1: App Launcher
            wallpaper_picker_page,  # Page 2: Wallpaper Picker
            power_menu_page,        # Page 3: Power Menu
            notification_menu_page, # Page 4: Notification Menu
            notification_notch_page # Page 5: Notification Notch
        ])

        # --- Floating menu pane (shown on hover) ---
        self.menu_pane = MenuPane(monitor)

        # --- Commands ---
        self.command_manager.add_command(
            "open-app-launcher",
            lambda *_: self.open_app_launcher()
        )
        self.command_manager.add_command(
            "close-app-launcher",
            lambda *_: self.close_app_launcher()
        )
        self.command_manager.add_command(
            "wallpaper_pick",
            lambda *_: self.open_wallpaper_picker()
        )
        self.command_manager.add_command(
            "close-wallpaper-picker",
            lambda *_: self.close_wallpaper_picker()
        )
        self.command_manager.add_command(
            "power_menu",
            lambda *_: self.open_power_menu()
        )
        self.command_manager.add_command(
            "close-power-menu",
            lambda *_: self.close_power_menu()
        )
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
        self.command_manager.add_command(
            "todo_menu",
            lambda *_: self.menu_pane.show()
        )
        self.command_manager.add_command(
            "todo-menu",
            lambda *_: self.menu_pane.show()
        )

        # Notification popup handling
        try:
            self._notification_service = NotificationService.get_default()
            if self._notification_service:
                self._notification_service.connect("new_popup", lambda *_args: self._on_new_notification_popup())
        except Exception:
            self._notification_service = None

        # --- Main notch ---
        main_notch = EventNotch(
            child=[self.hover_pane],
            orientation="left",
            on_hover=lambda *args: self.on_notch_hover(),
            on_hover_lost=lambda *args: self.on_notch_hover_lost(),
        )

        key_controller = Gtk.EventControllerKey()
        key_controller.connect("key-pressed", self._on_key_pressed)
        main_notch.add_controller(key_controller)

        vertical_layout = widgets.Box(
            orientation="vertical",
            vexpand=True,
            child=[
                widgets.Box(
                    orientation="vertical",
                    vexpand=True,
                    valign="center",
                    child=[main_notch]
                ),
            ]
        )

        self.window = widgets.Window(
            namespace=f"bar-{monitor}",
            css_classes=["bar"],
            monitor=monitor,
            anchor=["left", "top", "bottom"],
            exclusivity="ignore",
            layer="overlay",
            kb_mode="on_demand",
            child=vertical_layout,
        )

    # --- Hover: show/hide floating menu pane ---

    def on_notch_hover(self):
        if not self.app_launcher_mode:
            self.menu_pane.show()

    def on_notch_hover_lost(self):
        if not self.app_launcher_mode:
            self.menu_pane.schedule_hide()

    # --- Command-triggered menus (still in notch stack) ---

    def open_app_launcher(self):
        self.app_launcher_mode = True
        self.menu_pane.hide()
        self.hover_pane.show_page_index(1)
        if hasattr(self.hover_pane.pages[1], 'focus_search'):
            self.hover_pane.pages[1].focus_search()

    def close_app_launcher(self):
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def open_wallpaper_picker(self):
        self.app_launcher_mode = True
        self.menu_pane.hide()
        self.hover_pane.show_page_index(2)

    def close_wallpaper_picker(self):
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def open_power_menu(self):
        self.app_launcher_mode = True
        self.menu_pane.hide()
        self.hover_pane.show_page_index(3)

    def close_power_menu(self):
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def open_notification_menu(self):
        self.app_launcher_mode = True
        self.menu_pane.hide()
        self.hover_pane.show_page_index(4)
        if hasattr(self.hover_pane.pages[4], 'focus_search'):
            self.hover_pane.pages[4].focus_search()

    def close_notification_menu(self):
        self.app_launcher_mode = False
        self.hover_pane.show_default_page()

    def _on_new_notification_popup(self):
        try:
            self.hover_pane.show_page_index(5)
            GLib.timeout_add(4000, self._auto_hide_notification_notch)
        except Exception:
            pass

    def _auto_hide_notification_notch(self):
        try:
            self.hover_pane.show_default_page()
        except Exception:
            pass
        return False

    def _on_key_pressed(self, controller, keyval, keycode, state):
        if keyval == Gdk.KEY_Escape:
            self.app_launcher_mode = False
            self.menu_pane.hide()
            self.hover_pane.show_default_page()
            return True
        return False
