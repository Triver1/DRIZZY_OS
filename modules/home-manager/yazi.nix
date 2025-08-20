{ config, pkgs, lib, ... }:
{
  options.triverhome.yazi.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable Yazi file manager and custom keymaps.";
  };

  config = {
    home.packages = with pkgs; lib.optionals (config.triverhome.yazi.enable or true) [
    ripdrag
  ];
    programs.yazi = lib.mkIf (config.triverhome.yazi.enable or true) {
    enable = true;
    keymap = {
      manager.prepend_keymap = [
        {
          on = [ "t" ];
          run = "shell 'kitty --directory \"$0\" &' --confirm";
          desc = "Open terminal here";
        }
        {
          on = [ "g" ];
          run = "shell 'ripdrag \"$0\" &' --confirm";
          desc = "Drag files in/out";
        }
      ];
      };
    };
  };
}
