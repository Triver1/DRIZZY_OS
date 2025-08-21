{ config, pkgs, inputs,... }:
{
  programs.nix-ld.enable = true;
  programs.nix-ld.libraries = with pkgs; [

#Maple
    xorg.libXi
    dejavu_fonts
    corefonts
    javaPackages.openjfx21
    # gtk3
    glib

 #X11 dependencies (for GUI apps)
    xorg.libX11
    xorg.libXext
    xorg.libXrender
    xorg.libXtst

 #fonts
    fontconfig
    freetype

    ];
}
