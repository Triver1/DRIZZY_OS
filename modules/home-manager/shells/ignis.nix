{ config, pkgs, lib, inputs, ... }: {
  home.packages = [
    inputs.ignis.packages.${pkgs.system}.ignis
    # (python3.withPackages(ps: with ps; [
    #   (inputs.ignis.packages.${pkgs.stdenv.hostPlatform.system}.ignis.override {
    #     extraPackages = [
    #       # Add extra packages if needed
    #     ];
    #   })
    # ]))
  ];
}
