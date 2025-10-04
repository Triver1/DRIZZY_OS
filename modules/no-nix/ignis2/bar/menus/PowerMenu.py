from ignis import widgets
import subprocess


class PowerMenuButton(widgets.Button):
    def __init__(self, icon_name: str, label: str, command: str):
        self._command = command
        super().__init__(
            css_classes=["power-menu-button"],
            child=widgets.Box(

                spacing=8,
                child=[
                    widgets.Icon(
                        image=icon_name,
                        pixel_size=30,
                        css_classes=["power-menu-icon"]
                    )
                ]
            ),
            on_click=self._execute_command
        )

    def _execute_command(self, *_):
        try:
            subprocess.run(self._command, shell=True, check=True)
        except subprocess.CalledProcessError as e:
            print(f"Command failed: {e}")


class PowerMenu(widgets.Box):
    def __init__(self):
        # Create power option buttons
        shutdown_button = PowerMenuButton(
            icon_name="system-shutdown-symbolic",
            label="Shutdown", 
            command="systemctl poweroff"
        )
        
        reboot_button = PowerMenuButton(
            icon_name="system-reboot-symbolic",
            label="Restart",
            command="systemctl reboot"
        )
        
        logout_button = PowerMenuButton(
            icon_name="system-log-out-symbolic", 
            label="Logout",
            command="pkill -KILL -u $USER"  # Alternative: loginctl terminate-user $USER
        )
        
        lock_button = PowerMenuButton(
            icon_name="system-lock-screen-symbolic",
            label="Lock",
            command="loginctl lock-session"
        )

        # Arrange buttons in a 2x2 grid
        top_row = widgets.Box(
            spacing=16,
            child=[shutdown_button, reboot_button, logout_button, lock_button]
        )
        

        super().__init__(
            orientation="vertical",
            spacing=16,
            css_classes=["power-menu"],
            child=[top_row],
            halign="center",
            valign="center"
        )

    def focus_search(self):
        # Called by HoverPane when this page becomes visible
        try:
            self.grab_focus()
        except Exception:
            pass
