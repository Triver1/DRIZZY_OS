from ignis import widgets
from sharedwidgets import PopupWindow
from ignis.services.mpris import MprisService, MprisPlayer


mpris = MprisService.get_default()

def seconds_to_minutes(seconds):
    return f"{seconds // 60}:{seconds % 60}"

class PlayerPopup(PopupWindow):
    def __init__(self):
        self._player_container = widgets.Box(vertical=True, spacing=12, child=[])

        no_media = widgets.Label(
            label="No media",
            visible=mpris.bind("players", transform=lambda players: len(players) == 0)
        )

        content = widgets.Box(
            vertical=True,
            spacing=12,
            child=[
                widgets.Box(halign="center", child=[self._player_container]),
                no_media,
            ],
            setup=lambda box: self._setup_updates(),
        )

        super().__init__(
            child=widgets.Box(css_classes=["controlcenter"], child=[content]),
            namespace="PlayerPopup",
            popup=True,
            kb_mode="exclusive",
            layer="top",
            anchor=["top"],
            background_color=None,
            content_halign="center",
            content_valign="start",
        )

    def _setup_updates(self):
        self._update_current()
        try:
            mpris.connect("player-added", lambda *_: self._update_current())
        except Exception:
            pass

    def _update_current(self):
        players = getattr(mpris, "players", []) or []
        self._player_container.child = []
        if players:
            player = players[0]
            ui = self._render_player(player)
            self._player_container.child = [ui]
            try:
                player.connect("closed", lambda *_: self._update_current())
            except Exception:
                pass

    def _render_player(self, player: MprisPlayer):
        art = widgets.Icon(image=player.bind("art_url"), pixel_size=200)
        title = widgets.Label(label=player.bind("title", transform=lambda t: t or ""))

        def on_seek(value):
            try:
                if getattr(player, "length", 0) > 0:
                    player.set_position(value)
            except Exception:
                pass

        position_text = widgets.Label(
            label= player.bind("position", lambda pos : seconds_to_minutes(pos))
        )
        length_text = widgets.Label(label=player.bind("length", lambda len : seconds_to_minutes(len)))


        progress = widgets.Scale(
            min=0,
            max=player.bind("length", transform=lambda l: l or 0),
            value=player.bind("position", transform=lambda p: p or 0),
            hexpand=True,
            on_change=lambda s: on_seek(s.value),
        )

        def on_play_click(_):
            try:
                # Prefer play_pause if available
                play_pause = getattr(player, "play_pause", None)
                if callable(play_pause):
                    play_pause()
                else:
                    status = getattr(player, "playback_status", None)
                    if status == "Playing":
                        getattr(player, "pause", lambda: None)()
                    else:
                        getattr(player, "play", lambda: None)()
            except Exception:
                pass

        play_icon = player.bind(
            "playback_status",
            transform=lambda s: "media-playback-pause-symbolic" if s == "Playing" else "media-playback-start-symbolic",
        )

        controls = widgets.Box(spacing=12, halign="center",child=[
            widgets.Button(child=widgets.Icon(image="media-skip-backward-symbolic"), on_click=lambda _: getattr(player, "previous", lambda: None)()),
            widgets.Button(child=widgets.Icon(image=play_icon), on_click=on_play_click),
            widgets.Button(child=widgets.Icon(image="media-skip-forward-symbolic"), on_click=lambda _: getattr(player, "next", lambda: None)()),
        ])

        return widgets.Box(vertical=True, spacing=12, child=[
            widgets.Box(halign="center", child=[art]),
            title,
            widgets.Box(child=[position_text, progress, length_text]),
            progress,
            controls,
        ]) 
