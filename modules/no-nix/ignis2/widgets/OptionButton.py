from ignis import widgets


class OptionButton(widgets.Box):
    def __init__(self, default_label, menu_options):
        # Header button that always remains visible
        def toggle_menu(*_):
            print(f"toggle_menu: {id(self)} - current: {self.stack.get_visible_child_name()}")
            current = self.stack.get_visible_child_name()
            if current == "empty":
                self.stack.set_visible_child_name("default")
            else:
                self.stack.set_visible_child_name("empty")
            
        self.header_button = widgets.Button(
            label=default_label,
            on_click=toggle_menu
        )
        
        # Stack for the menu options below the header  
        self.stack = widgets.Stack(
            interpolate_size=True, 
            vhomogeneous=False, 
            hhomogeneous=False, 
            transition_type="crossfade"
        )
        
        # Add empty page (default state)
        self.stack.add_named(
            child=widgets.Box(child=[], vertical=True, vexpand=False, valign="start"),  # Empty page
            name="empty"
        )

        # Build pages from dict-of-dicts structure, e.g.:
        # {
        #   "default": { "option1": {"on_click": <callable>, "next_menu": "menu2"} },
        #   "menu2":   { "back":    {"on_click": <callable>, "next_menu": "close"} }
        # }
        for name, options in (menu_options.items() if hasattr(menu_options, "items") else []):
            option_buttons = []
            for key, opt in (options.items() if hasattr(options, "items") else []):
                # Each opt is a dict; read keys safely
                def _make_handler(opt_dict, option_button_instance):
                    def _handler(*_args):
                        cb = opt_dict.get("on_click")
                        if callable(cb):
                            cb()
                        nxt = opt_dict.get("next_menu")
                        if nxt:
                            option_button_instance.open_page(nxt)
                    return _handler
                option_buttons.append(widgets.Button(label=key, hexpand=True,on_click=_make_handler(opt, self)))

            self.stack.add_named(
                child=widgets.Box(child=option_buttons, valign="start",  vertical=True, hexpand=True, halign="fill"),
                name=name
            )

        # Set initial visible child to "empty" (collapsed state)
        self.stack.set_visible_child_name("empty")
        
        # Arrange header button and stack vertically
        super().__init__(
            css_classes=["quickmenu_button"], 
            child=[self.header_button, self.stack],
            vertical=True,
            vexpand=False,
            hexpand=True,
            halign="fill",
            valign="start"
        )

    def open_page(self, page):
        if page == "close":
            self.stack.set_visible_child_name("empty")
            return
        self.stack.set_visible_child_name(page)
