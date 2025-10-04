from ignis import widgets
from ignis.services.audio import AudioService


class AudioButton(widgets.Box):
    def __init__(self):
        self.audio = AudioService.get_default()
        self.speaker = self.audio.speaker
        self.on_icon_click = None  # Will be set by the parent
        
        self.icon_button = widgets.Button(
            child=widgets.Icon(
                image=self.speaker.bind("icon_name"),
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
                    max=100,
                    hexpand=True,
                    value=self.speaker.bind("volume"),
                    on_change=lambda scale: self.speaker.set_volume(scale.value),
                )
            ]
        )


class AudioPage(widgets.Box):
    def __init__(self):
        self.audio = AudioService.get_default()
        self.speaker = self.audio.speaker
        self.microphone = self.audio.microphone

        def render_controls():
            
            # Output device selector
            output_label = widgets.Label(
                label="Output Device", 
                css_classes=["audio-control-label"],
                halign="start"
            )
            
            output_selector = widgets.DropDown(
                items=self.audio.bind(
                    "speakers",
                    transform=lambda speakers: [speaker.description for speaker in speakers]
                ),
                selected=self.audio.bind(
                    "speaker",
                    transform=lambda speaker: speaker.description if speaker else "None"
                ),
                on_selected=lambda dropdown: self.audio.set_default_speaker(
                    next((s for s in self.audio.speakers if s.description == dropdown.selected), None)
                )
            )
            
            # Input device selector
            input_label = widgets.Label(
                label="Input Device", 
                css_classes=["audio-control-label"],
                halign="start"
            )

            
            input_selector = widgets.DropDown(
                items=self.audio.bind(
                    "microphones",
                    transform=lambda mics: [mic.description for mic in mics]
                ),
                selected=self.audio.bind(
                    "microphone",
                    transform=lambda mic: mic.description if mic else "None"
                ),
                on_selected=lambda dropdown: self.audio.set_default_microphone(
                    next((m for m in self.audio.microphones if m.description == dropdown.selected), None)
                )
            )
            
            
            # Volume control section
            volume_label = widgets.Label(
                label="Volume", 
                css_classes=["audio-control-label"],
                halign="start"
            )
            
            volume_control = widgets.Box(
                spacing=10,
                child=[
                    widgets.Button(
                        child=widgets.Icon(
                            image=self.speaker.bind("icon_name"),
                            pixel_size=18,
                        ),
                        on_click=lambda *_: self.speaker.set_is_muted(not self.speaker.is_muted)
                    ),
                    widgets.Scale(
                        step=1,
                        min=0,
                        max=100,
                        hexpand=True,
                        value=self.speaker.bind("volume"),
                        on_change=lambda scale: self.speaker.set_volume(scale.value),
                    ),
                    widgets.Label(
                        label=self.speaker.bind(
                            "volume", 
                            transform=lambda vol: f"{vol:.0f}%"
                        )
                    )
                ]
            )
            
            # Microphone control section
            mic_label = widgets.Label(
                label="Microphone", 
                css_classes=["audio-control-label"],
                halign="start"
            )
            
            mic_control = widgets.Box(
                spacing=10,
                child=[
                    widgets.Button(
                        child=widgets.Icon(
                            image=self.microphone.bind("icon_name"),
                            pixel_size=18,
                        ),
                        on_click=lambda *_: self.microphone.set_is_muted(not self.microphone.is_muted)
                    ),
                    widgets.Scale(
                        step=1,
                        min=0,
                        max=100,
                        hexpand=True,
                        value=self.microphone.bind("volume"),
                        on_change=lambda scale: self.microphone.set_volume(scale.value),
                    ),
                    widgets.Label(
                        label=self.microphone.bind(
                            "volume", 
                            transform=lambda vol: f"{vol:.0f}%"
                        )
                    )
                ]
            )
            
            return [
                widgets.Box(
                    vertical=True,
                    spacing=8,
                    child=[
                        output_label,
                        output_selector,
                        input_label,
                        input_selector,
                        volume_label,
                        volume_control,
                        mic_label,
                        mic_control
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
