from ignis import widgets
from ignis.services.backlight import BacklightService


class BrightnessButton(widgets.Box):
    def __init__(self):
        self.backlight = BacklightService.get_default()
        self.on_icon_click = None  # Will be set by the parent
        
        self.icon_button = widgets.Button(
            child=widgets.Icon(
                image="display-brightness-symbolic",
                pixel_size=16
            ),
            on_click=lambda *_: self.on_icon_click() if self.on_icon_click else None
        )
        
        super().__init__(
            spacing=5,
            hexpand=True,
            halign="fill",
            child=[
                self.icon_button,
                widgets.Scale(
                    step=1,
                    min=0,
                    max=self.backlight.bind("max_brightness"),
                    hexpand=True,
                    value=self.backlight.bind("brightness"),
                    on_change=lambda scale: self.backlight.set_brightness(scale.value),
                )
            ]
        )


class BrightnessPage(widgets.Box):
    def __init__(self):
        self.backlight = BacklightService.get_default()
        # Keyboard brightness - simplified implementation
        self.kbd_brightness = 50  # Default value, could be connected to actual kbd backlight service
        
        def render_controls():
            
            # Display brightness control section
            display_label = widgets.Label(
                label="Display Brightness", 
                css_classes=["brightness-control-label"],
                halign="start"
            )
            
            display_control = widgets.Box(
                spacing=10,
                child=[
                    widgets.Button(
                        child=widgets.Icon(
                            image="display-brightness-symbolic",
                            pixel_size=18,
                        ),
                        on_click=lambda *_: None  # Could add brightness preset functionality
                    ),
                    widgets.Scale(
                        step=1,
                        min=0,
                        max=self.backlight.bind("max_brightness"),
                        hexpand=True,
                        value=self.backlight.bind("brightness"),
                        on_change=lambda scale: self.backlight.set_brightness(scale.value),
                    ),
                    widgets.Label(
                        label=self.backlight.bind(
                            "brightness", 
                            transform=lambda brightness: f"{int((brightness / self.backlight.max_brightness) * 100) if self.backlight.max_brightness > 0 else 0}%"
                        )
                    )
                ]
            )
            
            # Keyboard brightness control section
            kbd_label = widgets.Label(
                label="Keyboard Brightness", 
                css_classes=["brightness-control-label"],
                halign="start"
            )
            
            def update_kbd_brightness(value):
                self.kbd_brightness = value
                # Here you could add actual keyboard backlight control
                # For now, just store the value
            
            kbd_scale = widgets.Scale(
                step=1,
                min=0,
                max=100,
                hexpand=True,
                value=self.kbd_brightness,
                on_change=lambda scale: update_kbd_brightness(scale.value),
            )
            
            kbd_percentage_label = widgets.Label(label=f"{self.kbd_brightness}%")
            
            def update_kbd_label(*_):
                kbd_percentage_label.label = f"{int(kbd_scale.value)}%"
            
            kbd_scale.on_change = lambda scale: (
                update_kbd_brightness(scale.value),
                update_kbd_label()
            )
            
            kbd_control = widgets.Box(
                spacing=10,
                child=[
                    widgets.Button(
                        child=widgets.Icon(
                            image="keyboard-brightness-symbolic",
                            pixel_size=18,
                        ),
                        on_click=lambda *_: None
                    ),
                    kbd_scale,
                    kbd_percentage_label
                ]
            )
            
            return [
                widgets.Box(
                    vertical=True,
                    spacing=8,
                    child=[
                        display_label,
                        display_control,
                        kbd_label,
                        kbd_control
                    ]
                )
            ]

        super().__init__(
            hexpand=True,
            halign="fill",
            vertical=True,
            spacing=8,
            child=render_controls(),
        )
