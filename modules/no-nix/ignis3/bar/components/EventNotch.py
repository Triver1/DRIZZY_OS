from ignis import widgets
from gi.repository import Gtk, Gdk

class EventNotch(widgets.Box):
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

    def __init__(self, child=None, orientation='top', on_hover=None, on_hover_lost=None, **kwargs):
        
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
        elif orientation == "left":
            children = [
                child_box,
            ]

        # Create the notch structure inside a Box
        notch_content = widgets.Box(
            homogeneous=False,
            vexpand=False,
            child=children,
        )

        # Initialize Box with the notch structure (instead of EventBox)
        super().__init__(child=[notch_content], **kwargs)
        
        # Store callbacks
        self._on_hover = on_hover
        self._on_hover_lost = on_hover_lost
        
        # Add motion controller for hover detection if callbacks are provided
        if on_hover or on_hover_lost:
            self._setup_hover_detection()
    
    def _setup_hover_detection(self):
        """Set up EventControllerMotion for hover detection without blocking input."""
        motion_controller = Gtk.EventControllerMotion()
        
        # Connect enter and leave events
        if self._on_hover:
            motion_controller.connect("enter", lambda controller, x, y: self._on_hover())
        if self._on_hover_lost:
            motion_controller.connect("leave", lambda controller: self._on_hover_lost())
        
        # Add controller to the widget - this allows text inputs to still work
        self.add_controller(motion_controller)
