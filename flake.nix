{
  description = "Nixos config flake";

  inputs = {

    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    # astal = {
    #   url = "github:aylur/astal";
    #   inputs.nixpkgs.follows = "nixpkgs";
    # };
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    stylix =  {
       url = "github:danth/stylix";
       inputs.nixpkgs.follows = "nixpkgs";
    };

    spicetify-nix = {
       url = "github:Gerg-L/spicetify-nix";
       inputs.nixpkgs.follows = "nixpkgs";
    };

    quickshell = {
          # add ?ref=<tag> to track a tag
          url = "git+https://git.outfoxxed.me/outfoxxed/quickshell";

          # THIS IS IMPORTANT
          # Mismatched system dependencies will lead to crashes and other issues.
          inputs.nixpkgs.follows = "nixpkgs";
        };
    ignis = {
      url = "github:ignis-sh/ignis";
      inputs.nixpkgs.follows = "nixpkgs";
      };
    textfox = {
      url = "github:adriankarlen/textfox";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, home-manager,... }@inputs: {
    # use "nixos", or your hostname as the name of the configuration
    # it's a better practice than "default" shown in the video
    nixosConfigurations.desktop = nixpkgs.lib.nixosSystem {
      specialArgs = {inherit inputs;};
      modules = [
        ./hosts/desktop/configuration.nix
        inputs.home-manager.nixosModules.default
	      inputs.stylix.nixosModules.stylix
      ];
    };
    nixosConfigurations.laptop = nixpkgs.lib.nixosSystem {
      specialArgs = {inherit inputs;};
      modules = [
        ./hosts/laptop/configuration.nix
        inputs.home-manager.nixosModules.default
	      inputs.stylix.nixosModules.stylix
      ];
    };
  };
}
