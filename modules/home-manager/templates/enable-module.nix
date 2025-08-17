{ config, lib, pkgs, ... }:
{
  options.triverhome.example.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Enable example feature (template).";
  };

  config = lib.mkIf (config.triverhome.example.enable or false) {
    # Add configuration here when enabled
  };
} 