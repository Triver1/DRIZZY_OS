{ config, pkgs, lib, ... }:

{
  environment.systemPackages = with pkgs; [
    xwayland-satellite
    fuzzel
    walker
  ];

  programs.niri = {
    enable = true;
    package = pkgs.niri;
  };
  environment.sessionVariables.NIXOS_OZONE_WL = "1";

  # Home manager configuration for hyprland
home-manager.sharedModules = [{ 
  programs.kitty = {
      settings = {
        hide_window_decorations = "yes";
      };
    };
# In your home-manager configuration

  home.file.".config/niri/config.kdl".text = ''
      spawn-at-startup "quickshell -p ../../assets/quickshell/"
      output "DP-3" {
          mode "3440x1440@180.0"
          focus-at-startup
      }
      input {
          keyboard {
              xkb {
                  layout "us"
                options "caps:escape"
              }
          }
      }

      layout {
          gaps 16
          focus-ring {
              width 2
              active-gradient from="rgba(255, 255, 255, 0.9)" to="rgba(240, 248, 255, 0.3)" angle=135
          }
      }
      


      binds {
          // Programs
          Mod+Return { spawn "kitty"; }
          Mod+B { spawn "firefox";}


          // System  
          Mod+Space { spawn "walker"; }
          Mod+P { screenshot; }
          
          // Windows/workspaces
          Mod+Q { close-window; }
          Mod+O repeat=false { toggle-overview; }

          Mod+H { focus-column-left; }
          Mod+L { focus-column-right; }
          Mod+K { focus-window-or-workspace-up; }
          Mod+J { focus-window-or-workspace-down; }

          Mod+Comma  { consume-window-into-column; }
          Mod+Period { expel-window-from-column; }

          Mod+1 { focus-workspace 1; }
          Mod+2 { focus-workspace 2; }
          Mod+3 { focus-workspace 3; }
          Mod+4 { focus-workspace 4; }
          Mod+5 { focus-workspace 5; }
          Mod+6 { focus-workspace 6; }
          Mod+7 { focus-workspace 7; }
          Mod+8 { focus-workspace 8; }
          Mod+9 { focus-workspace 9; }


          Mod+Minus { set-column-width "-10%"; }
          Mod+Equal { set-column-width "+10%"; }
          Mod+Shift+Minus { set-window-height "-10%"; }
          Mod+Shift+Equal { set-window-height "+10%"; }

          Mod+Ctrl+F { expand-column-to-available-width; }
          Mod+W { toggle-column-tabbed-display; }
          Mod+V { toggle-window-floating;}

          
      }
      window-rule {
          geometry-corner-radius 7
          clip-to-geometry true
      }

      '';
  }];
}
