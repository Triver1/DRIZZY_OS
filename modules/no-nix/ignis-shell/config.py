from ignis.widgets import Widget
from bar import Bar
from ignis.css_manager import CssManager, CssInfoPath
import os
from ignis import utils
from ignis.icon_manager import IconManager

# Load css
css_manager = CssManager.get_default()

css_manager.apply_css(
    CssInfoPath(
        name="main",
        compiler_function=lambda path: utils.sass_compile(path=path),
        path=os.path.join(utils.get_current_dir(), "style.scss"),
    )
)


# Load icons
icon_manager = IconManager.get_default()
icon_manager.add_icons(os.path.join(utils.get_current_dir(), "assets/icons"))

# initialize
Bar(0)
