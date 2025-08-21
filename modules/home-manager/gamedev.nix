{ config, lib, pkgs, ... }:
with lib;
let
  cfg = config.games.dev;
in
{
  options.games.dev.unity.enable = mkOption {
    type = types.bool;
    default = false;
    description = "Install Unity Hub for Unity game development.";
  };

  options.games.dev.godot.enable = mkOption {
    type = types.bool;
    default = false;
    description = "Install Godot game engine.";
  };

  config = {
    home.packages =
      (lib.optionals cfg.unity.enable [ pkgs.unityhub ])
      ++ (lib.optionals cfg.godot.enable [ (pkgs.godot_4 or pkgs.godot) ]);
  };
} 