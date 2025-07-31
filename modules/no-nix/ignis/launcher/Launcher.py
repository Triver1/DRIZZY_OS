
from ignis import widgets
from ignis.services.applications import ApplicationsService

from rapidfuzz import process

applications = ApplicationsService.get_default()


class AppsList(widgets.Box):

    def app_launch(self, app):
        app.launch()
        self.close_callback()

    def __init__(self, close_callback):
        self.apps = applications.apps
        super().__init__(
            vertical=True,
                         child=[]
        )
        self.filter("")
        self.close_callback = close_callback


    def app_widget(self, app): 
        return widgets.Button(
            css_classes=["launcherapp"],
            child=widgets.Box(spacing=10, child=[
                widgets.Icon(image=app.icon, pixel_size=20),
                widgets.Label(label=app.name)
            ]),
            on_click=lambda x: app_launch(app)
        )
         
    def filter(self, filter_string):
        if not filter_string:
            apps = []
        else:
            matches = process.extract(filter_string, [app.name for app in self.apps], limit=10, score_cutoff=40)
            app_dict = {app.name: app for app in self.apps}
            apps = [app_dict[match[0]] for match in matches]
        
        self.child = [self.app_widget(app) for app in apps]
    
    def launch_first(self, filter_string):
        apps = []
        if filter_string:
            matches = process.extract(filter_string, [app.name for app in self.apps], limit=1, score_cutoff=40)
            app_dict = {app.name: app for app in self.apps}
            apps = [app_dict[match[0]] for match in matches]
        
        if len(apps) > 0:
            apps[0].launch()


class Launcher(widgets.Window):
    def __init__(self):
        self.apps_list = AppsList(lambda x: self.set_visible(False))

        self.input = widgets.Entry(
            placeholder_text="Filter apps",
            on_accept=lambda x: self.launch(x.text),
            on_change=lambda x: self.apps_list.filter(x.text),
            css_classes=["launcherinput"],
            hexpand=True
        )

        search_box = widgets.Box(
            css_classes=["launcher-search-box"],
            child=[
                widgets.Icon(
                    icon_name="system-search-symbolic",
                    pixel_size=24,
                    style="margin-right: 0.5rem;",
                ),
                self.input,
            ],
        )

        main_box = widgets.Box(
            vertical=True,
            valign="start",     # Expand downward
            halign="center",    # Center horizontally
            spacing=10,
            css_classes=["launcher"],
            child=[
                search_box,
                self.apps_list
            ]
        )

        contents = widgets.Overlay(
            child=widgets.Button(  # Transparent fullscreen layer
                vexpand=True,
                hexpand=True,
                can_focus=False,
                css_classes=["unset"],
                style="background-color: rgba(0, 0, 0, 0.3);",
                on_click=lambda x: setattr(self, "visible", False),
            ),
            overlays=[main_box]
        )

        super().__init__(
            css_classes=["launcherwindow"],
            setup=lambda self: self.connect("notify::visible", self.__on_open),
            namespace="launcher",
            child=contents,
            monitor=0,
            anchor=["top", "right", "bottom", "left"],  # Fullscreen
            layer="top",
            kb_mode="exclusive",
            popup=True
        )

        self.input.grab_focus()

    def launch(self, input):
        self.apps_list.launch_first(input)
        self.visible = False

    def __on_open(self, *args) -> None:
        self.input.text = ""
        self.input.grab_focus()

