from ignis import widgets
from ignis.services.notifications import NotificationService

notifications_service = NotificationService.get_default()


class Toast(widgets.Button):
    def __init__(self, notification):
        icon = widgets.Icon(
            image=notification.bind("icon", transform=lambda i: i or "dialog-information-symbolic"),
            pixel_size=20,
        )

        text_column = widgets.Box(
            vertical=True,
            spacing=2,
            child=[
                widgets.Label(label=notification.bind("app_name", transform=lambda a: a or "")),
                widgets.Label(
                    css_classes=["title"],
                    label=notification.bind("summary", transform=lambda t: t or "Notification"),
                ),
                widgets.Label(
                    label=notification.bind("body", transform=lambda b: b or ""),
                    visible=notification.bind("body", transform=bool),
                ),
            ],
        )

        super().__init__(
            css_classes=["toast", "panel"],
            child=widgets.Box(spacing=8, child=[icon, text_column]),
            on_click=lambda _: self.unparent(),
            style="padding: 0.6rem 0.8rem;",
        )


class NotificationsToasts(widgets.Window):
    def __init__(self):
        self.stack = widgets.Box(vertical=True, spacing=8, child=[])

        super().__init__(
            namespace="NotificationsToasts",
            css_classes=["toasts"],
            visible=False,  # start hidden
            popup=False,
            kb_mode="on_demand",
            layer="top",
            anchor=["top", "right"],
            child=widgets.Box(child=[self.stack], style="margin-top: 52px; margin-right: 8px;"),
        )

        if notifications_service:
            try:
                notifications_service.connect("new_popup", lambda _, n: self._add_popup(n))
            except Exception:
                pass

    def _add_popup(self, notification):
        def remove_popup(popup):
            popup.unparent()
            if not self.stack.child:
                self.visible = False  # hide window when empty

        try:
            toast = Toast(notification)
            for signal in ("closed", "dismissed"):
                try:
                    notification.connect(signal, lambda *_: remove_popup(toast))
                except Exception:
                    pass

            self.stack.append(toast)
            self.visible = True  # ensure window is shown when adding a toast
        except Exception:
            pass
