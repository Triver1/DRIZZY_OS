from ignis import widgets
from ignis.services.niri import NiriService
from .MangoWorkspaces import MangoWorkspaces

class WorkspaceButton(widgets.Button):
    def __init__(self, workspace):
        self.workspace = workspace
        
        super().__init__(
            css_classes=["workspace"] + (["active"] if workspace.is_active else []),
            on_click=lambda x: workspace.switch_to(),
            hexpand=False,
            halign="start",
        )


class Workspaces(widgets.Box):
    def __init__(self, monitor_name=None):
        try:
            self.niri = NiriService.get_default()
        except Exception:
            self.niri = None
        self.monitor_name = monitor_name
        
        super().__init__(
            orientation="vertical",
            spacing=3,
            hexpand=False,
            halign="center",
            child=(
                self.niri.bind(
                    "workspaces",
                    transform=self._create_workspace_buttons
                ) if self.niri else MangoWorkspaces()
            )
        )
    
    def _create_workspace_buttons(self, workspaces):
        if workspaces and not workspaces[-1].is_active:
            workspaces = workspaces[:-1]
        
        return [WorkspaceButton(workspace) for workspace in workspaces]
