{inputs, config, lib, pkgs, ... }:

{
   imports = [
      inputs.mango.nixosModules.mango
  ];
  programs.mango.enable = true;

  
}
