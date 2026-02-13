{ config, pkgs, lib, inputs, ... }:
{
  options.triverhome.shells.ignis.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable Ignis shell environment.";
  };

  imports = [
    inputs.ignis.homeManagerModules.default
  ];

  config = lib.mkIf config.triverhome.shells.ignis.enable {

    programs.ignis = {
      enable = true;

      # Add Ignis to the Python environment (useful for LSP support)
      addToPythonEnv = true;


      # Enable dependencies required by certain services.
      # NOTE: This won't affect your NixOS system configuration.
      # For example, to use NetworkService, you must also enable
      # NetworkManager in your NixOS configuration:
      #   networking.networkmanager.enable = true;
      services = {
        bluetooth.enable = true;
        recorder.enable = true;
        audio.enable = true;
        network.enable = true;
      };

      # Enable Sass support
      sass = {
        enable = true;
        useDartSass = true;
      };

      # Extra packages available at runtime
      # These can be regular packages or Python packages
      extraPackages = with pkgs; [
          python313Packages.rapidfuzz
          python313Packages.google-genai
          python313Packages.pillow
          python313Packages.python-pam
      ];
    };

  };
}
