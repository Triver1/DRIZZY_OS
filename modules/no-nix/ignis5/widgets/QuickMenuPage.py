from ignis import widgets


class QuickMenuPage(widgets.Box):
    def __init__(self, title: str, body_child):
        title_label = widgets.Label(
            label=title,
            css_classes=["header", "box-header"],
            halign="start",
        )

        # Wrap provided body in a styled container and place title inside
        body_box = widgets.Box(
            vertical=True,
            spacing=6,
            css_classes=["box", "quickmenu-card"],
            child=[title_label, body_child],
            hexpand=True,
            halign="fill",
        )

        super().__init__(
            vertical=True,
            spacing=8,
            child=[body_box],
            hexpand=True,
            halign="fill",
        )


