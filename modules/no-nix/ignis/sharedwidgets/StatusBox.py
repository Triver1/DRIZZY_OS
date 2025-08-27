from ignis import widgets


public_status_box = widgets.Box(
    spacing=5,
    child=[],
)


_open_menu_callback = None


def set_open_menu_callback(callback):
    global _open_menu_callback
    _open_menu_callback = callback


def open_menu(name: str):
    if _open_menu_callback:
        _open_menu_callback(name)


