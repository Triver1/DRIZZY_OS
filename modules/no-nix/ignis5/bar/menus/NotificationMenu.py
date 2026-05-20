from ignis import widgets
from ignis.services.notifications import NotificationService
from gi.repository import GLib


class NotificationMenu(widgets.Box):
    def __init__(self):
        self._service = NotificationService.get_default()

        header = widgets.Box(spacing=8, child=[
            widgets.Label(css_classes=["header"], label="Notifications"),
            widgets.Box(hexpand=True),
            widgets.Button(
                css_classes=["wordbutton"],
                child=widgets.Icon(image="edit-clear-symbolic", pixel_size=16),
                on_click=lambda *_: self._service.clear_all(),
                can_focus=False,
                tooltip_text="Clear all",
            ),
        ])

        notifications_scroll = widgets.Scroll(
            child=self._service.bind(
                "notifications",
                transform=lambda notes: widgets.Box(
                    vertical=True,
                    spacing=8,
                    child=[self._render_item(n) for n in notes] if notes else [widgets.Label(label="No notifications")]
                )
            ),
            min_content_width=450,
            min_content_height=220,
            max_content_height=500,
        )

        super().__init__(
            vertical=True,
            spacing=8,
            css_classes=["notification-menu"],
            child=[
                header,
                notifications_scroll,
            ],
        )

    def _render_item(self, notification):
        icon = widgets.Icon(
            image=notification.bind("icon", transform=lambda i: i or "dialog-information-symbolic"),
            pixel_size=20,
        )
        app_label = widgets.Label(label=notification.bind("app_name", transform=lambda a: a or ""))
        title_label = widgets.Label(css_classes=["title"], label=notification.bind("summary", transform=lambda t: t or "Notification"))
        body_label = widgets.Label(label=notification.bind("body", transform=lambda b: b or ""), visible=notification.bind("body", transform=lambda b: bool(b)))

        header_left = widgets.Box(spacing=6, child=[app_label, title_label])
        close_btn = widgets.Button(css_classes=["wordbutton"], child=widgets.Icon(image="window-close-symbolic", pixel_size=14), on_click=lambda *_: notification.close(), can_focus=False)
        header_row = widgets.Box(spacing=8, child=[header_left, widgets.Box(hexpand=True), close_btn])

        actions_row = widgets.Box(
            spacing=6,
            child=notification.bind(
                "actions",
                transform=lambda acts: [
                    widgets.Button(
                        css_classes=["wordbutton"],
                        label=getattr(a, "label", "Action"),
                        on_click=lambda *_ , action=a: action.invoke(),
                    ) for a in (acts or [])
                ]
            ),
            visible=notification.bind("actions", transform=lambda acts: bool(acts)),
        )

        right = widgets.Box(vertical=True, spacing=6, child=[header_row, body_label, actions_row])
        row = widgets.Box(spacing=8, child=[icon, right])
        container = widgets.Box(css_classes=["panel"], child=[row], style="padding: 0.6rem 0.8rem;")
        return container

    def focus_search(self):
        try:
            self.grab_focus()
        except Exception:
            pass


class NotificationNotch(widgets.Box):
    def __init__(self, auto_hide_ms: int = 0):
        self._service = NotificationService.get_default()
        self._auto_hide_ms = max(0, int(auto_hide_ms))

        # Build expanding content
        content = widgets.Box(
            vertical=True,
            spacing=8,
            child=self._service.bind(
                "popups",
                transform=lambda notes: [self._render_item(n) for n in (notes or [])] if notes else [widgets.Label(label="No notifications")]
            )
        )
        

        # Interpolating stack (empty -> content)
        self.stack = widgets.Stack(
            hhomogeneous=False,
            vhomogeneous=False,
            interpolate_size=True,
            transition_type="crossfade",
        )
        self.stack.add_named(widgets.Box(), "empty")
        self.stack.add_named(content, "content")
        self.stack.set_visible_child_name("empty")

        super().__init__(vertical=True, spacing=6, child=[self.stack])

        # On new popup: expand; optionally auto-collapse
        try:
            if self._service:
                self._service.connect("new_popup", lambda *_: self._on_new_popup())
        except Exception:
            pass

    def _on_new_popup(self):
        try:
            self.stack.set_visible_child_name("content")
        except Exception:
            pass
        if self._auto_hide_ms > 0:
            try:
                GLib.timeout_add(self._auto_hide_ms, self._collapse)
            except Exception:
                self._collapse()

    def _collapse(self):
        try:
            self.stack.set_visible_child_name("empty")
        except Exception:
            pass
        return False

    def _render_item(self, notification):
        icon = widgets.Icon(
            image=notification.bind("icon", transform=lambda i: i or "dialog-information-symbolic"),
            pixel_size=16,
        )
        title_label = widgets.Label(css_classes=["title"], label=notification.bind("summary", transform=lambda t: t or "Notification"))
        body_label = widgets.Label(label=notification.bind("body", transform=lambda b: b or ""), visible=notification.bind("body", transform=lambda b: bool(b)))
        right = widgets.Box(vertical=True, spacing=2, child=[title_label, body_label])
        return widgets.Box(spacing=6, child=[icon, right])

