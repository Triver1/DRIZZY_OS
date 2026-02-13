from ignis import widgets
from ignis.command_manager import CommandManager
from ignis import utils
from pam import pam
import getpass
import datetime
from windows.generate_wallpapers import get_wallpaper_manager

class LockScreen(widgets.Window):
    def __init__(self):
        # Get wallpaper manager for background
        wm = get_wallpaper_manager()
        
        self.password_entry = widgets.Entry(
            placeholder_text="Enter password...",
            on_accept=lambda entry: self._authenticate(entry.text),
            visibility=False,  # This should hide the text (show dots)
            css_classes=["lockscreen-password"],
        )

        # Create clock with polling
        self.clock_label = widgets.Label(
            label=utils.Poll(
                1_000,
                lambda self: datetime.datetime.now().strftime("%H:%M"),
            ).bind("output"),
            css_classes=["lockscreen-clock"],
        )

        # Background box that will have the wallpaper
        self.background_box = widgets.Box(
            css_classes=["lockscreen-background"],
        )
        
        # Apply wallpaper to background if available
        if wm and hasattr(wm, '_background_box') and wm._background_box.style:
            self.background_box.style = wm._background_box.style

        super().__init__(
            namespace="lockscreen",
            css_classes=["lockscreen"],
            anchor=["left", "top", "right", "bottom"],
            layer="overlay",  # Use overlay layer like the bar
            exclusivity="ignore",  # Don't interfere with bar space
            popup=True,  # This should ensure it appears above other overlay elements
            kb_mode="exclusive",  # grabs keyboard exclusively
            child=widgets.Overlay(
                child=self.background_box,
                overlays=[
                    widgets.Box(
                        vertical=True,
                        halign="center",
                        valign="center",
                        spacing=30,
                        child=[
                            self.clock_label,
                            self.password_entry,
                        ],
                    ),
                ],
            ),
        )

        # Auto-focus password entry
        self.password_entry.grab_focus()

    def _authenticate(self, password):
        user = getpass.getuser()
        if pam().authenticate(user, password, service="login"):
            print("Authentication successful! Unlocking…")
            self.close()  # <<< closes this window
        else:
            print("Wrong password")
            self.password_entry.set_text("")  # clear input field

# Register command
command_manager = CommandManager.get_default()
command_manager.add_command("lockscreen", lambda *_: LockScreen())
