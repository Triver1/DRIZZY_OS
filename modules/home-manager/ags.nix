{ inputs, pkgs, ... }:
{
  # add the home manager module
  home.packages = with pkgs; [
    ags
  ];

  programs.ags = {
    enable = true;

    # symlink to ~/.config/ags
    configDir = ../../assets/ags;

    # additional packages and executables to add to gjs's runtime
    extraPackages = with pkgs; [
      inputs.astal.packages.${pkgs.system}.battery
      fzf
    ];
  };
}