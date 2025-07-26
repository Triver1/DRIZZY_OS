from ignis import widgets
from ignis import utils
import datetime

# # Services
# from ignis.services.audio import AudioService
from ignis.services.system_tray import SystemTrayService, SystemTrayItem
from ignis.services.niri import NiriService, NiriWorkspace
from ignis.services.notifications import NotificationService
from ignis.services.mpris import MprisService, MprisPlayer





# audio = AudioService.get_default()
system_tray = SystemTrayService.get_default()
niri = NiriService.get_default()
notifications = NotificationService.get_default()
mpris = MprisService.get_default()



# Modules:
# Clock, Battery, Time, Launcher 

class Panel(widgets.Button):
    def __init__(self, *, child=None, on_click=None, css_classes=None, **kwargs):
        all_classes = ["panel"] + (css_classes or [])

        super().__init__(
            child=child,
            on_click=on_click,
            css_classes=all_classes,
            **kwargs
        )


class Launcher(Panel):
    def __init__(self):
        super().__init__(
            child=widgets.Icon(
                image="nix",
                pixel_size=20
            )
        )



class ClockCalendar(Panel):
    def __init__(self):
        super().__init__(
            css_classes=["clock"],
            label=utils.Poll(
                1_000, lambda self: datetime.datetime.now().strftime("%H:%M")
            ).bind("output"),
        )


class NiriWorkspaceButton(Panel):
    def __init__(self, workspace: NiriWorkspace):
        super().__init__(
            css_classes=["workspace"],
            on_click=lambda x: workspace.switch_to(),
            child=widgets.Label(label=str(workspace.id))
        )
        if workspace.is_active:
            self.add_css_class("active")

class NiriWorkspaces(widgets.EventBox):

    def __init__(self,monitor_name):
            super().__init__(

                on_scroll_up=lambda x: self.scroll_workspace(1),
                on_scroll_down=lambda x: self.scroll_workspace(1),
                spacing=5,
                child=niri.bind(
                    "workspaces",
                    transform = lambda value: [
                        NiriWorkspaceButton(i) for i in value 
                    ]
                )
            )


    def scroll_workspace(self,dir):
        current = list(
        filter(lambda w: w.is_active, niri.workspaces) )[0].idx
        target = (current + dir) % (len(niri.workspaces) + 1)
        print(target)
        niri.switch_to_workspace(target) 






class Bar(widgets.Window):  # inheriting from widgets.Window
    __gtype_name__ = "MyBar"  # optional, this will change the widget's display name in the GTK inspector.

    def __init__(self, monitor: int):
        # button2 = widgets.Button(
        #     child=widgets.Label(label="Close window"),
        #     on_click=lambda x: self.set_visible(False),  # you can use "self" - the window object itself
        # )

        super().__init__(  # calling the constructor of the parent class (widgets.Window)
            namespace=f"some-window-{monitor}",
            css_classes=["bar"],
            monitor=monitor,
            anchor=["left", "top", "right"],
            exclusivity="exclusive",
            child=widgets.Box(
                spacing=10,
                child=[
                    widgets.Label(label="WOOPIEFONT Test dit is de current font"),
                    Launcher(),
                    NiriWorkspaces(monitor),
                    ClockCalendar(),
                ],
            ),
        )

    def some_func(self) -> None:
        print("Custom function on self!")
