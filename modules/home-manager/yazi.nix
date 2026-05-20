{ config, pkgs, lib, ... }:

let
  cfg = config.triverhome.yazi;
in
{
  options.triverhome.yazi.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable Yazi file manager and custom keymaps.";
  };

  config = lib.mkIf cfg.enable {
    home.packages = with pkgs; [
      ripdrag
    ];

    programs.yazi = {
      enable = true;

      plugins = {
        yatline = "${pkgs.yaziPlugins.yatline}";
        ouch = "${pkgs.yaziPlugins.ouch}";
        bookmarks = "${pkgs.yaziPlugins.bookmarks}";
      };

      initLua = ''
        require("yatline"):setup()

        require("bookmarks"):setup({
          persist = "vim",
        })
      '';

      keymap = {
        mgr.prepend_keymap = [
          {
            on = [ "t" ];
            run = "shell --orphan -- kitty";
            desc = "Open terminal here";
          }
          {
            on = [ "g" ];
            run = ''
              shell --orphan -- sh -c 'if [ "$#" -gt 1 ]; then shift; exec ripdrag --target --keep --all "$@"; elif [ -n "$1" ]; then exec ripdrag --target --keep "$1"; else exec ripdrag --target --keep; fi' sh %h %s
            '';
            desc = "Open ripdrag for drag-in and drag-out";
          }

          # bookmarks.yazi keybinds
          {
            on = [ "m" ];
            run = "plugin bookmarks save";
            desc = "Save current position as a bookmark";
          }
          {
            on = [ "'" ];
            run = "plugin bookmarks jump";
            desc = "Jump to a bookmark";
          }
          {
            on = [ "b" "d" ];
            run = "plugin bookmarks delete";
            desc = "Delete a bookmark";
          }
          {
            on = [ "b" "D" ];
            run = "plugin bookmarks delete_all";
            desc = "Delete all bookmarks";
          }
        ];
      };
    };
  };
}
