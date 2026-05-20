from ignis import widgets
from ignis.services.mpris import MprisService


class NowPlaying(widgets.Box):
    def __init__(self):
        self.mpris = MprisService.get_default()

        def render_player(player):
            if player is None:
                return [
                    widgets.Icon(pixel_size=12, image="audio-x-generic-symbolic"),
                    widgets.Label(label="—"),
                ]
            return [
                widgets.Icon(pixel_size=12, image=player.bind("art_url")),
                widgets.Label(
                    label=player.bind(
                        "title",
                        transform=lambda t: (t or "—")[:14],
                    )
                ),
            ]

        super().__init__(
            orientation="vertical",
            spacing=5,
            halign="center",
            css_classes=["nowplaying-indicator"],
            child=self.mpris.bind("player", render_player),
        )
