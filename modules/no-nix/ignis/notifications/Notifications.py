from ignis import widgets
from sharedwidgets import PopupWindow


class NotificationsPopup(PopupWindow):
    def __init__(self):
        contents = widgets.Box(
            vertical=True,
            spacing=15,
            child=[
                widgets.Box(child=[
                    widgets.Label(css_classes=["title"], label="Notifications"),
                ]),
                widgets.Box(
                    vertical=True,
                    spacing=10,
                    child=[
                        widgets.Label(label="No notifications yet"),
                    ],
                ),
            ],
        )

        super().__init__(
            child=widgets.Box(css_classes=["controlcenter"], child=[contents]),
            namespace="NotificationsCenter",
            popup=True,
            kb_mode="exclusive",
            layer="top",
            anchor=["top", "right"],
            background_color=None,
        ) 