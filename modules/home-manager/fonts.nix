
{ config, pkgs, lib, inputs, ... }:
{
  fonts.packages = with pkgs; [
    noto-fonts-color-emoji
    liberation_ttf
    fira-code
    fira-code-symbols
    mplus-outline-fonts.githubRelease
    dina-font
    proggyfonts
    inconsolata
    cascadia-code
    # Nerd Fonts for terminal applications
    nerd-fonts.fira-code
    nerd-fonts.jetbrains-mono
  ];
}
