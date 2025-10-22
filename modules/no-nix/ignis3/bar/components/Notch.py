from ignis import widgets

class Notch(widgets.Box):
    def __get_corner__(self, orientation):
        """Creates a corner widget with notchcorner color styling."""
        return widgets.Corner(
            orientation=orientation,
            width_request=15,
            height_request=15,
            vexpand=False,
            valign="start",
            css_classes=["notchcorner"]  # color styling
        )

    def __init__(self, child=None, orientation='top'):

        # Determine which border-radius class to apply to the child box
        css_class = ["notch"]
        if orientation == "top-left":
            css_class.append("notchcorner-left")   # left corner rounded
        elif orientation == "top-right":
            css_class.append("notchcorner-right")  # right corner rounded

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

        # Assemble children with corners
        children = []
        if orientation == 'top':
            children = [
                self.__get_corner__('top-right'),
                child_box,
                self.__get_corner__('top-left'),
            ]
        elif orientation == "top-left":
            children = [
                child_box,
                self.__get_corner__('top-left'),
            ]
        elif orientation == "top-right":
            children = [
                self.__get_corner__('top-right'),
                child_box,
            ]

        super().__init__(
            homogeneous=False,
            vexpand=False,
            valign="start",
            child=children,
        )
