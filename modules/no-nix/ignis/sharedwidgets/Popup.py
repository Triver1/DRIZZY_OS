from ignis import widgets


class PopupWindow(widgets.Window):
    def __init__(
        self,
        *,
        child,
        namespace: str = "popup",
        anchor: list[str] = None,
        layer: str = "top",
        kb_mode: str = "on_demand",
        monitor: int = 0,
        popup: bool = True,
        background_color: str | None = None,
        css_classes: list[str] | None = None,
        close_on_outside_click: bool = True,
        content_anchor: list[str] | None = None,
        content_halign: str | None = None,
        content_valign: str | None = None,
    ):
        self._background_color = background_color

        # Determine where to place the popup content
        if content_anchor is None:
            content_anchor = anchor or []

        def _halign_from_anchor(anchors: list[str]) -> str:
            if content_halign:
                return content_halign
            if anchors and "left" in anchors:
                return "start"
            if anchors and "right" in anchors:
                return "end"
            return "center"

        def _valign_from_anchor(anchors: list[str]) -> str:
            if content_valign:
                return content_valign
            if anchors and "top" in anchors:
                return "start"
            if anchors and "bottom" in anchors:
                return "end"
            return "center"

        content_wrapper = widgets.Box(
            child=[child],
            halign=_halign_from_anchor(content_anchor),
            valign=_valign_from_anchor(content_anchor),
            hexpand=False,
            vexpand=False,
        )

        if close_on_outside_click:
            overlay_button_style = (
                f"background-color: {background_color};"
                if background_color
                else "background-color: transparent; border: none; box-shadow: none;"
            )
            overlay = widgets.Overlay(
                child=widgets.Button(
                    vexpand=True,
                    hexpand=True,
                    can_focus=False,
                    css_classes=["unset"],
                    style=overlay_button_style,
                    on_click=lambda x: setattr(self, "visible", False),
                ),
                overlays=[content_wrapper],
            )
            contents = overlay
            # Fullscreen window to capture outside clicks
            anchor = ["top", "right", "bottom", "left"]
        else:
            contents = content_wrapper
            if anchor is None:
                anchor = ["top", "left"]

        super().__init__(
            namespace=namespace,
            child=contents,
            anchor=anchor,
            layer=layer,
            kb_mode=kb_mode,
            monitor=monitor,
            popup=popup,
            visible=False,
            css_classes=css_classes or [],
        )

    def toggle_reveal(self):
        self.visible = not self.visible 