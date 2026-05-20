from ignis import widgets
from ignis.services.upower import UPowerService

class Battery(widgets.Box):
    def __init__(self):
        self.upower_battery = UPowerService.get_default().batteries[0]
        self.show_time = False  # Toggle state for displaying time vs percentage
        
        super().__init__(
            orientation="vertical",
            spacing=5,
            halign="center",
            child=[
                widgets.Icon(
                    image=self.upower_battery.bind("icon_name"),
                    pixel_size=12
                ),
                widgets.Label(
                    label=self.upower_battery.bind(
                        "percent",
                        transform=lambda percent: self.format_display(percent)
                    )
                )
            ]
        )

    def format_display(self, percent):
        """Format display text based on current mode"""
        if self.show_time:
            return self.format_time(self.upower_battery.time_remaining)
        else:
            return f"{percent:.0f}%"

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

    def toggle_display(self):
        """Toggle between showing percentage and remaining time"""
        self.show_time = not self.show_time
        # Force update the label
        self.children[1].label = self.format_display(self.upower_battery.percent)



