from ignis import widgets

class HoverPane(widgets.Box):
    def __init__(self, pages):
        self.pages = list(pages or [])
        self.page_names = [f"page{index+1}" for index in range(len(self.pages))]

        self.stack = widgets.Stack(
            interpolate_size=True,
            vhomogeneous=False,
            hhomogeneous=False,
            transition_type="crossfade"
        )

        for name, widget in zip(self.page_names, self.pages):
            self.stack.add_named(widget, name)

        if self.page_names:
            self.stack.set_visible_child_name(self.page_names[0])

        super().__init__(child=[self.stack])

    def show_default_page(self):
        if self.page_names:
            self.stack.set_visible_child_name(self.page_names[0])

    def show_page_index(self, index):
        if 0 <= index < len(self.page_names):
            self.stack.set_visible_child_name(self.page_names[index])
            
            # Auto-focus search if this is an AppLauncher
            if hasattr(self.pages[index], 'focus_search'):
                self.pages[index].focus_search()
