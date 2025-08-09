from ignis import widgets
from bar import Bar
from launcher import Launcher
from ignis import utils
from ignis.icon_manager import IconManager
from theme_manager import ThemeManager
import os

# Load icons
icon_manager = IconManager.get_default()
icon_manager.add_icons(os.path.join(utils.get_current_dir(), "assets/icons"))

# initialize
Bar(0)
Launcher()



# Example usage:
theme_manager = ThemeManager()
theme_manager.apply_styling()

