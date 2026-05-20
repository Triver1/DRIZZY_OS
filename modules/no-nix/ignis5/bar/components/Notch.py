from ignis import widgets

class Notch(widgets.Box):
    def __get_corner__(self, orientation):
        """Corner widgets disabled."""
        return None

    def __init__(self, child=None, orientation='top'):

        # Corner styling disabled
        css_class = ["notch"]

        # Properly handle child content - if it's a list, pass it as is, otherwise wrap in list
        if child is None:
            child_content = []
        elif isinstance(child, list):
            child_content = child
        else:
            child_content = [child]
            
        child_box = widgets.Box(
            child=child_content, 
            css_classes=css_class,
            vexpand=False,
            valign="start"
        )

        # Assemble children without corners
        children = [child_box]

        super().__init__(
            homogeneous=False,
            vexpand=False,
            valign="start",
            child=children,
        )
