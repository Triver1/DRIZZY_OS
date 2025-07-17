{ config, pkgs, ... }:
{
  home.packages = with pkgs; [
    ripdrag
  ];
  programs.yazi = {
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
}
