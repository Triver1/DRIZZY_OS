{ config, pkgs, lib, inputs, ... }: {
  options.triverhome.shells.ignis.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable Ignis shell environment.";
  };
  config = {
    home.packages = lib.mkIf (config.triverhome.shells.ignis.enable or true) [
      (inputs.ignis.packages.${pkgs.stdenv.hostPlatform.system}.ignis.override {
        extraPackages = [
          pkgs.python313Packages.rapidfuzz
          pkgs.python313Packages.pygobject3
          pkgs.python313Packages.dbus-python
          pkgs.networkmanager
          pkgs.gnome-bluetooth
          pkgs.glib
          pkgs.dart-sass
          pkgs.python313Packages.google-genai
        ];
      })
    ];
  };
}
