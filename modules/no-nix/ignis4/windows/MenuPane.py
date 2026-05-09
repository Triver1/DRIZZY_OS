from ignis import widgets
from gi.repository import Gtk, GLib
from services.todo_service import get_todo_service
from bar.menus.QuickMenu import QuickMenu


class TodoSection(widgets.Box):
    def __init__(self):
        self.svc = get_todo_service()

        self._entry = widgets.Entry(
            placeholder_text="Add todo...",
            css_classes=["themed-entry"],
            hexpand=True,
            on_accept=lambda entry: self._add_todo(entry),
        )
        add_btn = widgets.Button(
            child=widgets.Icon(image="list-add-symbolic", pixel_size=12),
            css_classes=["todo-action-btn"],
            on_click=lambda *_: self._add_todo(self._entry),
        )
        entry_row = widgets.Box(spacing=4, child=[self._entry, add_btn])

        self._list_box = widgets.Box(orientation="vertical", spacing=1)
        scrollable = widgets.Scroll(
            child=self._list_box,
            hexpand=True,
            vexpand=True,
            max_content_height=220,
        )

        self._timer_label = widgets.Label(
            label=self.svc.get_timer_display(),
            css_classes=["pane-timer-display"],
        )
        self._active_label = widgets.Label(
            label="No active todo",
            css_classes=["pane-active-label"],
            ellipsize="end",
            max_width_chars=18,
        )
        self._start_btn = widgets.Button(
            child=widgets.Icon(image="media-playback-start-symbolic", pixel_size=14),
            css_classes=["pane-timer-btn"],
            on_click=lambda *_: self.svc.toggle_timer(),
        )
        timer_5 = widgets.Button(
            child=widgets.Label(label="5"), css_classes=["pane-dur-btn"],
            on_click=lambda *_: self.svc.set_duration(5 * 60),
        )
        timer_15 = widgets.Button(
            child=widgets.Label(label="15"), css_classes=["pane-dur-btn"],
            on_click=lambda *_: self.svc.set_duration(15 * 60),
        )
        timer_25 = widgets.Button(
            child=widgets.Label(label="25"), css_classes=["pane-dur-btn"],
            on_click=lambda *_: self.svc.set_duration(25 * 60),
        )

        timer_row = widgets.Box(
            spacing=4, halign="center",
            child=[self._start_btn, self._timer_label,
                   widgets.Box(spacing=2, child=[timer_5, timer_15, timer_25])],
        )

        super().__init__(
            orientation="vertical",
            spacing=4,
            child=[
                widgets.Label(label="Todos", css_classes=["pane-section-title"], halign="start"),
                entry_row,
                scrollable,
                self._active_label,
                timer_row,
            ],
        )

        self.svc.add_listener(self._on_update)
        self._refresh_list()

    def _on_update(self):
        self._timer_label.label = self.svc.get_timer_display()
        active = self.svc.get_active_todo_text()
        self._active_label.label = f"▶ {active}" if active else "No active todo"
        if self.svc.timer_active:
            self._start_btn.child = widgets.Icon(image="media-playback-pause-symbolic", pixel_size=14)
        else:
            self._start_btn.child = widgets.Icon(image="media-playback-start-symbolic", pixel_size=14)
        self._refresh_list()

    def _add_todo(self, entry):
        text = entry.text.strip()
        if not text:
            return
        self.svc.add_todo(text)
        entry.set_text("")

    def _refresh_list(self):
        for child in list(self._list_box):
            self._list_box.remove(child)

        todos = self.svc.read_todos()
        if not todos:
            self._list_box.append(
                widgets.Label(label="No todos yet", css_classes=["todo-empty"], halign="center")
            )
            return

        for i, (text, done) in enumerate(todos):
            check_icon = "object-select-symbolic" if done else "radio-symbolic"
            check_btn = widgets.Button(
                child=widgets.Icon(image=check_icon, pixel_size=12),
                css_classes=["todo-check-btn", "todo-done"] if done else ["todo-check-btn"],
                on_click=lambda *_, idx=i: self.svc.toggle_done(idx),
            )
            label = widgets.Label(
                label=text,
                css_classes=["todo-item-label-done"] if done else ["todo-item-label"],
                halign="start", hexpand=True,
                ellipsize="end", max_width_chars=18,
            )
            remove_btn = widgets.Button(
                child=widgets.Icon(image="edit-delete-symbolic", pixel_size=10),
                css_classes=["todo-remove-btn"],
                on_click=lambda *_, idx=i: self.svc.remove_todo(idx),
            )
            row = widgets.Box(spacing=3, css_classes=["todo-row"], child=[check_btn, label, remove_btn])
            self._list_box.append(row)


class MenuPane:
    def __init__(self, monitor: int):
        self.visible = False
        self._hide_timeout_id = None
        self._pane_hovered = False

        quick_menu = QuickMenu()
        left_col = widgets.Box(orientation="vertical", spacing=4, child=[quick_menu])
        right_col = TodoSection()

        columns = widgets.Box(spacing=10, child=[left_col, right_col])

        pane_content = widgets.Box(
            orientation="vertical",
            css_classes=["menu-pane"],
            child=[columns],
        )

        self._revealer = widgets.Revealer(
            child=pane_content,
            transition_type="slide_right",
            transition_duration=120,
            reveal_child=False,
        )

        revealer_box = widgets.Box(
            child=[self._revealer],
            valign="center",
        )

        self.window = widgets.RevealerWindow(
            namespace=f"menu-pane-{monitor}",
            monitor=monitor,
            anchor=["left", "top", "bottom"],
            exclusivity="ignore",
            layer="overlay",
            kb_mode="on_demand",
            visible=False,
            css_classes=["menu-pane-window"],
            child=revealer_box,
            revealer=self._revealer,
        )

        motion = Gtk.EventControllerMotion()
        motion.connect("enter", lambda *_: self._on_pane_hover())
        motion.connect("leave", lambda *_: self._on_pane_hover_lost())
        self.window.add_controller(motion)

        key_ctrl = Gtk.EventControllerKey()
        key_ctrl.connect("key-pressed", self._on_key)
        self.window.add_controller(key_ctrl)

    def show(self):
        self._cancel_hide()
        if not self.visible:
            self.visible = True
            self.window.visible = True

    def hide(self):
        if self.visible:
            self.visible = False
            self._pane_hovered = False
            self.window.visible = False

    def schedule_hide(self):
        self._cancel_hide()
        self._hide_timeout_id = GLib.timeout_add(250, self._do_hide)

    def _do_hide(self):
        self._hide_timeout_id = None
        if not self._pane_hovered:
            self.hide()
        return False

    def _cancel_hide(self):
        if self._hide_timeout_id:
            GLib.source_remove(self._hide_timeout_id)
            self._hide_timeout_id = None

    def _on_pane_hover(self):
        self._pane_hovered = True
        self._cancel_hide()

    def _on_pane_hover_lost(self):
        self._pane_hovered = False
        self.schedule_hide()

    def _on_key(self, controller, keyval, keycode, state):
        from gi.repository import Gdk
        if keyval == Gdk.KEY_Escape:
            self.hide()
            return True
        return False
