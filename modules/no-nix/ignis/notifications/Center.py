from ignis import widgets
from sharedwidgets import PopupWindow
from ignis.services.notifications import NotificationService

from .Toasts import NotificationsToasts


class NotificationsCenter:
    def __init__(self):
        self._service = NotificationService.get_default()

        clear_btn = widgets.Button(
            css_classes=["wordbutton"],
            can_focus=False,
            child=widgets.Icon(image="edit-clear-symbolic", pixel_size=16),
            on_click=lambda _: self._service.clear_all(),
            tooltip_text="Clear all",
        )

        header = widgets.Box(spacing=8, child=[
            widgets.Label(css_classes=["title"], label="Notifications"),
            widgets.Box(hexpand=True),
            clear_btn,
        ])

        # Scrollable notifications list (separate from toasts)
        scroll = widgets.Scroll(
            child=self._service.bind(
                "notifications",
                transform=lambda notes: widgets.Box(
                    vertical=True,
                    spacing=8,
                    child=[self._render_history_item(n) for n in notes]
                )
            ),
            min_content_height=320,
            max_content_height=640,
        )

        content = widgets.Box(vertical=True, spacing=12, child=[header, scroll], style="min-width: 420px; max-width: 560px;")

        self.popup = PopupWindow(
            child=widgets.Box(css_classes=["controlcenter"], child=[content]),
            namespace="NotificationsCenter",
            popup=True,
            kb_mode="exclusive",
            layer="top",
            anchor=["top", "right"],
            background_color=None,
        )

        self.toasts = NotificationsToasts()

    def _render_history_item(self, notification):
        icon = widgets.Icon(
            image=notification.bind("icon", transform=lambda i: i or "dialog-information-symbolic"),
            pixel_size=20,
        )
        app_label = widgets.Label(label=notification.bind("app_name", transform=lambda a: a or ""))
        title_label = widgets.Label(css_classes=["title"], label=notification.bind("summary", transform=lambda t: t or "Notification"))
        body_label = widgets.Label(label=notification.bind("body", transform=lambda b: b or ""), visible=notification.bind("body", transform=lambda b: bool(b)))

        header_left = widgets.Box(spacing=6, child=[app_label, title_label])
        close_btn = widgets.Button(css_classes=["wordbutton"], child=widgets.Icon(image="window-close-symbolic", pixel_size=14), on_click=lambda _: notification.close(), can_focus=False)
        header_row = widgets.Box(spacing=8, child=[header_left, widgets.Box(hexpand=True), close_btn])

        actions_row = widgets.Box(
            spacing=6,
            child=notification.bind(
                "actions",
                transform=lambda acts: [
                    widgets.Button(
                        css_classes=["wordbutton"],
                        label=getattr(a, "label", "Action"),
                        on_click=lambda _, action=a: action.invoke(),
                    )
                    for a in acts
                ]
            ),
            visible=notification.bind("actions", transform=lambda acts: bool(acts)),
        )

        right = widgets.Box(vertical=True, spacing=6, child=[header_row, body_label, actions_row])
        row = widgets.Box(spacing=8, child=[icon, right])
        container = widgets.Box(css_classes=["panel"], child=[row], style="padding: 0.6rem 0.8rem;")
        return container

    def toggle_reveal(self):
        self.popup.toggle_reveal() 