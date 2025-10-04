from ignis import widgets
from ignis.services.niri import NiriService

class WorkspaceButton(widgets.Button):
    def __init__(self, workspace):
        self.workspace = workspace
        
        super().__init__(
            css_classes=["workspace"] + (["active"] if workspace.is_active else []),
            on_click=lambda x: workspace.switch_to(),
            vexpand=False,
            valign="start",
        )


class Workspaces(widgets.Box):
    def __init__(self, monitor_name=None):
        self.niri = NiriService.get_default()
        self.monitor_name = monitor_name
        
        super().__init__(
            spacing=3,
            vexpand=False,
            valign="start",
            child=self.niri.bind(
                "workspaces",
                transform=self._create_workspace_buttons
            )
        )
    
    def _create_workspace_buttons(self, workspaces):
        if workspaces and not workspaces[-1].is_active:
            workspaces = workspaces[:-1]
        
        return [WorkspaceButton(workspace) for workspace in workspaces]
