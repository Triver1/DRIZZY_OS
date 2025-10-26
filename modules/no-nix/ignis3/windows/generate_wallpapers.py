from ignis import widgets 

from PIL import Image, ImageFilter
import os
import random
from modules.ConfigManager import ConfigManager


class WallpaperManager:
    def __init__(self, monitor: int, wallpaper_dir: str = None):
        self.monitor = monitor
        home_dir = os.path.expanduser("~")
        self.wallpaper_dir = wallpaper_dir or os.path.join(home_dir, "Pictures", "Wallpapers")
        self.blurred_dir = os.path.join(self.wallpaper_dir, "Blurred")
        os.makedirs(self.blurred_dir, exist_ok=True)
        self.config = ConfigManager.get_default()

        # Pre-create boxes and windows so we can update styles without stacking windows
        self._backdrop_box = widgets.Box(css_classes=["background-content"])
        self._background_box = widgets.Box(css_classes=["background-content"])

        widgets.Window(
            namespace=f"backdrop-{self.monitor}",
            layer="background",
            css_classes=["background"],
            monitor=self.monitor,
            anchor=["top", "left", "right", "bottom"],
            exclusivity="ignore",
            child=self._backdrop_box,
        )

        widgets.Window(
            namespace=f"background-{self.monitor}",
            layer="background",
            css_classes=["background"],
            monitor=self.monitor,
            anchor=["top", "left", "right", "bottom"],
            exclusivity="ignore",
            child=self._background_box,
        )
        
        self.load_saved_wallpaper()

    def get_available_wallpapers(self):
        if not os.path.isdir(self.wallpaper_dir):
            return []
        valid_exts = (".png", ".jpg", ".jpeg", ".bmp")
        return [
            os.path.join(self.wallpaper_dir, f)
            for f in os.listdir(self.wallpaper_dir)
            if f.lower().endswith(valid_exts)
        ]

    def _ensure_blurred(self, wallpaper_path: str) -> str:
        blurred_path = os.path.join(self.blurred_dir, os.path.basename(wallpaper_path))
        if not os.path.exists(blurred_path):
            with Image.open(wallpaper_path) as img:
                blurred = img.filter(ImageFilter.GaussianBlur(radius=20))
                blurred.save(blurred_path)
        return blurred_path

    def _apply_style_to_box(self, box: "widgets.Box", image_path: str):
        box.style = f"""
            background-image: url("file://{image_path}");
            background-size: cover;
            background-position: center;
        """

    def set_wallpaper(self, wallpaper_path: str):
        if not wallpaper_path or not os.path.isfile(wallpaper_path):
            return
        blurred_path = self._ensure_blurred(wallpaper_path)
        self._apply_style_to_box(self._backdrop_box, blurred_path)
        self._apply_style_to_box(self._background_box, wallpaper_path)
        self.config.write_config("wallpaper", "image", wallpaper_path)

    def set_random(self):
        wallpapers = self.get_available_wallpapers()
        if not wallpapers:
            return
        self.set_wallpaper(random.choice(wallpapers))
    
    def load_saved_wallpaper(self):
        saved_wallpaper = self.config.get_config("wallpaper", "image")
        if saved_wallpaper and os.path.isfile(saved_wallpaper):
            blurred_path = self._ensure_blurred(saved_wallpaper)
            self._apply_style_to_box(self._backdrop_box, blurred_path)
            self._apply_style_to_box(self._background_box, saved_wallpaper)
        else:
            self.set_random()


# --- Global accessor for a shared WallpaperManager instance ---
_GLOBAL_WALLPAPER_MANAGER = None


def set_wallpaper_manager(manager):
    global _GLOBAL_WALLPAPER_MANAGER
    _GLOBAL_WALLPAPER_MANAGER = manager


def get_wallpaper_manager():
    global _GLOBAL_WALLPAPER_MANAGER
    if _GLOBAL_WALLPAPER_MANAGER is None:
        # Lazy-init with monitor 0 if not set by config
        _GLOBAL_WALLPAPER_MANAGER = WallpaperManager(0)
    return _GLOBAL_WALLPAPER_MANAGER


