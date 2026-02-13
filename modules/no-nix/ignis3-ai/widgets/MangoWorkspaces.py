from ignis import widgets
from ignis import utils

from services.mango import MangoService


class MangoWorkspaces(widgets.Box):
    def __init__(self):
        self._mango = MangoService.get_default()

        def render(tags):
            if not tags:
                return [widgets.Label(label="-")]
            return [
                widgets.Label(label=str(tag), css_classes=["workspace-tag"])
                for tag in tags
            ]

        super().__init__(
            orientation="vertical",
            spacing=3,
            halign="center",
            child=utils.Poll(
                500,
                lambda _self: self._mango.get_tags(),
            ).bind("output", transform=render),
        )


