import os
import json
import tempfile 
from ignis.css_manager import CssManager, CssInfoPath
from ignis import utils

css_manager = CssManager.get_default()


class ThemeManager:
    def __init__(self, config_location="./settings.json"):
        self.config_location = config_location
        self.configuration = self.load_configuration()

    def load_configuration(self):
        try:
            with open(self.config_location, "r") as file:
                return json.load(file)
        except (FileNotFoundError, json.JSONDecodeError) as e:
            print(f"Error loading configuration: {e}. Using empty config.")
            return {}

    def save_configuration(self):
        with open(self.config_location, "w") as file:
            json.dump(self.configuration, file, indent=2)

    def _generate_sass_variables(self, variables: dict) -> str:
        return "".join([f"${key}: {value};" for key, value in variables.items()])

    def apply_styling(self):
        theme_name = self.configuration.get("selected_theme")
        if not theme_name:
            print("Error: 'selected_theme' not found in configuration.")
            return

        color_option_name = self.configuration.get("selected_colors")
        colors = self.configuration.get("color_options", {}).get(color_option_name)

        if not colors:
            print(f"Warning: Color option '{color_option_name}' not found. No colors applied.")
            colors = {}

        css_manager.reset_css()

        def compile_theme_with_colors(original_path: str) -> str:
            variable_string = self._generate_sass_variables(colors)
            with open(original_path, "r") as f:
                scss_content = f.read()
            full_scss = variable_string + scss_content

            with tempfile.NamedTemporaryFile(mode='w', suffix='.scss', delete=True) as temp_file:
                temp_file.write(full_scss)
                temp_file.flush()  # Ensure data is on disk before compiling
                return utils.sass_compile(path=temp_file.name)

        theme_path = os.path.join(f"themes/{theme_name}.scss")

        css_manager.apply_css(
            CssInfoPath(
                name=f"{theme_name}-{color_option_name}",
                path=theme_path,
                compiler_function=lambda p: compile_theme_with_colors(p),
            )
        )
        print(f"Theme '{theme_name}' with colors '{color_option_name}' applied.")

    def update_theme(self, theme_name: str):
        if theme_name not in self.configuration.get("theme_options", []):
            print(f"Error: Theme '{theme_name}' is not a valid option.")
            return
        self.configuration["selected_theme"] = theme_name
        self.save_configuration()
        self.apply_styling()

    def update_colors(self, color_option_name: str):
        if color_option_name not in self.configuration.get("color_options", {}):
            print(f"Error: Color option '{color_option_name}' not found.")
            return
        self.configuration["selected_colors"] = color_option_name
        self.save_configuration()
        self.apply_styling()

    def get_themes(self):
        return self.configuration.get("theme_options",[])
    
    def get_color_options(self):
        return self.configuration.get("color_options", {}).items()
