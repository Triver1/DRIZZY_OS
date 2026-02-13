from ignis import widgets
from ignis.command_manager import CommandManager
from windows.generate_wallpapers import get_wallpaper_manager


class WallpaperPicker(widgets.Scroll):
    def __init__(self, column_count=3, spacing=8):
        self._wm = get_wallpaper_manager()
        self._command_manager = CommandManager.get_default()
        self._paths = self._wm.get_available_wallpapers() or []
        path_count = len(self._paths)

        rows = []
        for i in range(0, path_count, column_count):
            row_items = []
            for j in range(i, min(i + column_count, path_count)):
                path = self._paths[j]
                row_items.append(
                    widgets.Button(
                        css_classes=["wallpaper-picker-item"],
                        child=widgets.Picture(css_classes=["wallpaper_picture"],image=path, width=150, height=100),
                        on_click=lambda _, p=path: self._select_wallpaper(p)
                    )
                )
            row = widgets.Box(
                spacing=spacing,
                child=row_items,
            )
            rows.append(row)

        if not rows:
            rows = [widgets.Label(label="No wallpapers found")]

        super().__init__(
            child=widgets.Box(
                orientation="vertical",
                spacing=spacing,
                css_classes=["wallpaper-picker"],
                child=rows,
            ),
            min_content_width=530,
            min_content_height=260,
            max_content_height=500,
        )

    def _select_wallpaper(self, path: str):
        self._wm.set_wallpaper(path)
        # Keep picker open after selection

    def focus_search(self):
        # Called by HoverPane when this page becomes visible
        try:
            self.grab_focus()
        except Exception:
            pass
