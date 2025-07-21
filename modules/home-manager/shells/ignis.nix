{ config, pkgs, lib, inputs, ... }: {
  home.packages = [
    (inputs.ignis.packages.${pkgs.stdenv.hostPlatform.system}.ignis.override {
      extraPackages = [
        # Add extra dependencies here
        # For example:
        pkgs.python312Packages.psutil
        pkgs.python312Packages.jinja2
        pkgs.python312Packages.pillow
        pkgs.python312Packages.materialyoucolor
      ];
    })
  ];
}
