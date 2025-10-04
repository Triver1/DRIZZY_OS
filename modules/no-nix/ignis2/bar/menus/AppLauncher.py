from ignis import widgets
from widgets.NotchPage import NotchPage
from ignis.services.applications import ApplicationsService
from ignis.command_manager import CommandManager
from gi.repository import Gdk
from gi.repository import Gtk

applications = ApplicationsService.get_default()
command_manager = CommandManager.get_default()

class AppLauncher(widgets.Box):
    def __init__(self):
        """
        Simple app launcher that displays all applications.
        Based on ignis1 AppsList pattern.
        """
        self.applications = applications.apps
        self.filtered_apps = []
        
        # Create search entry with escape key handling
        self.search_entry = widgets.Entry(
            placeholder_text="Search applications...",
            on_change=lambda entry: self._filter_apps(entry.text),
            on_accept=lambda entry: self._launch_first_app(),
            setup=self._setup_key_handler,
            css_classes=["themed-entry"],
        )
        
        # Create scrollable container for apps
        self.apps_container = widgets.Box(
            vertical=True,
            spacing=4,
            child=[]
        )
        
        scroll = widgets.Scroll(
            child=self.apps_container,
            min_content_height=400,
            max_content_height=500,
            min_content_width=300,
        )
        
        body = widgets.Box(vertical=True, spacing=6, child=[self.search_entry, scroll])
        page = NotchPage(title="Applications", body_child=body)
        # place the NotchPage inside the AppLauncher container for css scoping
        super().__init__(vertical=True, spacing=6, css_classes=["app-launcher"], child=[page])
        
        # Populate with apps immediately
        self._populate_apps()
    
    def _setup_key_handler(self, entry):
        """Set up key event handling for the search entry"""
        
        key_controller = Gtk.EventControllerKey()
        key_controller.connect("key-pressed", self._on_key_pressed)
        entry.add_controller(key_controller)
    
    def _on_key_pressed(self, controller, keyval, keycode, state):
        """Handle key press events"""
        if keyval == Gdk.KEY_Escape:
            # Close app launcher on Escape
            command_manager.run_command("close-app-launcher")
            return True
        return False
    
    def _launch_first_app(self):
        """Launch the first app in the filtered list when Enter is pressed"""
        if self.filtered_apps:
            self.filtered_apps[0].launch()
            command_manager.run_command("close-app-launcher")
    
    def focus_search(self):
        """Focus the search entry when the launcher becomes visible."""
        self.search_entry.text = ""  # Clear the entry
        self.search_entry.grab_focus()
        self._populate_apps()  # Reset to show all apps
    
    def _create_app_button(self, app):
        """Create a button widget for an application."""
        return widgets.Button(
            css_classes=["app-launcher-item"],
            child=widgets.Box(
                spacing=8,
                child=[
                    widgets.Icon(
                        image=app.icon,
                        pixel_size=18
                    ),
                    widgets.Label(
                        label=app.name,
                        hexpand=True,
                        halign="start"
                    )
                ]
            ),
            on_click=lambda _: app.launch()
        )
    
    def _filter_apps(self, search_text):
        """Filter apps based on search text."""
        search_text = search_text.lower().strip()
        if not search_text:
            self._populate_apps()  # Show all apps
            return
        
        # Filter apps by name
        filtered = []
        for app in self.applications:
            if not app.name or len(app.name.strip()) < 1:
                continue
            if app.name.startswith('.'):
                continue
            if search_text in app.name.lower():
                filtered.append(app)
        
        # Sort and display filtered apps
        self.filtered_apps = sorted(filtered, key=lambda app: app.name.lower())
        app_buttons = [self._create_app_button(app) for app in self.filtered_apps]
        self.apps_container.child = app_buttons

    def _populate_apps(self):
        """Populate the launcher with all applications."""
        # Filter out apps with empty names only
        valid_apps = []
        for app in self.applications:
            if not app.name or len(app.name.strip()) < 1:
                continue
            # Skip hidden apps that start with dot
            if app.name.startswith('.'):
                continue
            valid_apps.append(app)
        
        # Sort by name and show ALL apps
        self.filtered_apps = sorted(valid_apps, key=lambda app: app.name.lower())
        
        # Create buttons for each app
        app_buttons = [self._create_app_button(app) for app in self.filtered_apps]
        self.apps_container.child = app_buttons