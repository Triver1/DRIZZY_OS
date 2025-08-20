{ config, pkgs, lib, ... }:
{
  programs.steam.gamescopeSession.enable = true;
  programs.gamescope.enable = true;
} 