
import json
import os
from pathlib import Path

config_manager = None

class ConfigManager:
    def __init__(self, config_location):
        self.config_location = os.path.expanduser(config_location)
        self.config_data = {}
        self._load_config()
    
    @staticmethod
    def get_default():
        global config_manager
        if config_manager is None:
            config_manager = ConfigManager("~/.config/DRIZ_SHELL/config.json")
        return config_manager
    
    def _load_config(self):
        try:
            if os.path.exists(self.config_location):
                with open(self.config_location, 'r') as f:
                    self.config_data = json.load(f)
            else:
                os.makedirs(os.path.dirname(self.config_location), exist_ok=True)
                self.config_data = {}
                self._save_config()
        except (json.JSONDecodeError, IOError) as e:
            print(f"Error loading config from {self.config_location}: {e}")
            self.config_data = {}
    
    def _save_config(self):
        try:
            with open(self.config_location, 'w') as f:
                json.dump(self.config_data, f, indent=2)
        except IOError as e:
            print(f"Error saving config to {self.config_location}: {e}")
    
    def get_config(self, section, key, default=None):
        return self.config_data.get(section, {}).get(key, default)
    
    def write_config(self, section, key, value):
        if section not in self.config_data:
            self.config_data[section] = {}
        self.config_data[section][key] = value
        self._save_config()
