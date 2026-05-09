from ignis import widgets
from ignis.command_manager import CommandManager


class TodoButton(widgets.Button):
    def __init__(self):
        command_manager = CommandManager.get_default()
        super().__init__(
            label="Todos",
            on_click=lambda *_: command_manager.run_command("todo_menu"),
        )
