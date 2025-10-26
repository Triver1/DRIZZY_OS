import os

from windows.generate_wallpapers import WallpaperManager, set_wallpaper_manager
from windows.LockScreen import LockScreen 
from bar.Bar import Bar
from ignis import widgets
from ignis import utils
from ignis.css_manager import CssManager, CssInfoPath
from gi.repository import Gdk

css_manager = CssManager.get_default()
css_manager.apply_css(
    CssInfoPath(
        name="main",
        path=os.path.join(utils.get_current_dir(), "style.scss"),
        compiler_function=lambda path: utils.sass_compile(path=path),
    )
)

# Get the number of monitors
display = Gdk.Display.get_default()
n_monitors = display.get_monitors().get_n_items()

# Create Bar and WallpaperManager instances for each monitor
wallpaper_managers = []
bars = []

for monitor in range(n_monitors):
    wallpaper_manager = WallpaperManager(monitor)
    wallpaper_managers.append(wallpaper_manager)
    
    # Set the first monitor's wallpaper manager as the global one
    # (for backwards compatibility with code that uses get_wallpaper_manager())
    if monitor == 0:
        set_wallpaper_manager(wallpaper_manager)
    
    bar = Bar(monitor)
    bars.append(bar)
