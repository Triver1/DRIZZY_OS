from ignis import widgets


class NotchPage(widgets.Box):
    def __init__(self, title: str, body_child):
        header = widgets.Box(
            spacing=8,
            child=[
                widgets.Label(css_classes=["header", "notchpage-header"], label=title),
                widgets.Box(hexpand=True),
            ],
        )

        super().__init__(
            vertical=True,
            spacing=6,
            child=[header, body_child],
        )


