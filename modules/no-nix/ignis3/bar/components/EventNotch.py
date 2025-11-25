from ignis import widgets
from gi.repository import Gtk, Gdk

class EventNotch(widgets.Box):
    def __get_corner__(self, orientation):
        """Corner widgets disabled."""
        return None

    def __init__(self, child=None, orientation='top', on_hover=None, on_hover_lost=None, **kwargs):
        
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
