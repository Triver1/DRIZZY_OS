import os

from windows.generate_wallpapers import WallpaperManager, set_wallpaper_manager
from windows.LockScreen import LockScreen 
from bar.Bar import Bar
from ignis import widgets
from ignis import utils
from ignis.css_manager import CssManager, CssInfoPath

css_manager = CssManager.get_default()
css_manager.apply_css(
    CssInfoPath(
        name="main",
        path=os.path.join(utils.get_current_dir(), "style.scss"),
        compiler_function=lambda path: utils.sass_compile(path=path),
    )
)

wallpaper_manager = WallpaperManager(0)
set_wallpaper_manager(wallpaper_manager)
Bar(0)
