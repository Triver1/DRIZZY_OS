{pkgs,...}:
  let 
    my-sddm-theme = pkgs.stdenv.mkDerivation rec {
        name = "astronaut-theme";
        src = pkgs.fetchgit{
          url = "https://github.com/keyitdev/sddm-astronaut-theme.git";
          sha256 = "";
          fetchSubmodules = false;
         };
        installPhase = ''
          mkdir -p $out/share/sddm/themes
          cp -r $src $out/share/sddm/themes/astronaut-theme
          '';
        };
  in {
  services.displayManager.sddm = {
    enable = true;
    wayland.enable = true;
    theme = "astronaut-theme";
  };
}
