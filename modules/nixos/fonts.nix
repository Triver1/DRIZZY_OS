
{ config, pkgs, lib, inputs, ... }:
{
  fonts.packages = with pkgs; [
    google-fonts
    noto-fonts
    noto-fonts-cjk-sans
    noto-fonts-emoji
    liberation_ttf
    fira-code
    fira-code-symbols
    mplus-outline-fonts.githubRelease
    dina-font
    proggyfonts
    inconsolata
    montserrat
    # Nerd Fonts for terminal applications
    nerd-fonts.fira-code
    nerd-fonts.jetbrains-mono
  ];
}
