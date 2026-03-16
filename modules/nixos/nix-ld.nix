{ config, pkgs, inputs, ... }:
{
  programs.nix-ld.enable = true;

  programs.nix-ld.libraries = with pkgs; [
    # Needed for: libzstd.so.1
    zstd

    # Maple / GUI deps
    xorg.libXi
    xorg.libX11
    xorg.libXext
    xorg.libXrender
    xorg.libXtst

    dejavu_fonts
    corefonts
    javaPackages.openjfx21

    glib
    dbus
    fontconfig
    freetype
    libGL
    libxkbcommon
    wayland
  ];
}
