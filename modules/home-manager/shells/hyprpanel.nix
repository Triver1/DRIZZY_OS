# *.nix
{ inputs, pkgs, ... }:
{

  home.packages = [
    pkgs.nerd-fonts.caskaydia-cove
    pkgs.nerd-fonts.fira-code
    pkgs.nerd-fonts.jetbrains-mono
    pkgs.nerd-fonts.hack
  ];

  imports = [
  inputs.hyprpanel.homeManagerModules.hyprpanel
  ];

  programs.hyprpanel = {

    # Enable the module.
    # Default: false
    enable = true;

    # Automatically restart HyprPanel with systemd.
    # Useful when updating your config so that you
    # don't need to manually restart it.
    # Default: false
    systemd.enable = true;

    # Add '/nix/store/.../hyprpanel' to your
    # Hyprland config 'exec-once'.
    # Default: false

    # Fix the overwrite issue with HyprPanel.
    # See below for more information.
    # Default: false

    # Import a theme from './themes/*.json'.
    # Default: ""

    # Override the final config with an arbitrary set.
    # Useful for overriding colors in your selected theme.
    # Default: {}

    # Configure bar layouts for monitors.
    # See 'https://hyprpanel.com/configuration/panel.html'.
    # Default: null

    # Configure and theme almost all options from the GUI.
    # Options that require '{}' or '[]' are not yet implemented,
    # except for the layout above.
    # See 'https://hyprpanel.com/configuration/settings.html'.
    # Default: <same as gui>
    settings = {
      theme.name = "everforest";
      theme.font = {
        name = "Fira Code Nerd Font";
        size = "13px";
      };
    };
  };
}