{ config, pkgs, lib, ... }:

{
  environment.systemPackages = with pkgs; [
    xwayland-satellite
    fuzzel
    walker
    hyprlock
    brightnessctl
  ];

  programs.niri = {
    enable = true;
    package = pkgs.niri;
  };
  environment.sessionVariables.NIXOS_OZONE_WL = "1";


home-manager.sharedModules = [{ 
  programs.kitty = {
      settings = {
        hide_window_decorations = "yes";
      };
    };
  programs.hyprlock = {
      enable = true;
      settings = {
            general = {
             };
        };
    };
# In your home-manager configuration

  home.file.".config/niri/config.kdl".text = ''
      spawn-at-startup "ignis" "init" "-c" "/home/drizzy/NEWFLAKE/modules/no-nix/ignis/config.py"
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
    touchpad {
        // off
        tap
        dwt
        dwtp
        // drag false
        // drag-lock
        natural-scroll
        tap-button-map "left-right-middle"
        // accel-speed 0.2
        // accel-profile "flat"
        // scroll-method "two-finger"
        // disabled-on-external-mouse
    }
      warp-mouse-to-focus
      }

      layout {
          gaps 7 
          focus-ring {
              width 2
              active-gradient from="rgba(255, 255, 255, 0.9)" to="rgba(240, 248, 255, 0.3)" angle=135
          }
      }
      

          hotkey-overlay {
              skip-at-startup
          }

      binds {
          // Programs
          Mod+Return { spawn "kitty"; }
          Mod+B { spawn "firefox";}
          Mod+E { spawn "kitty" "-e" "yazi";}
          Mod+Backspace {spawn "hyprlock";}
          Mod+C { spawn "cursor"; }


          // System  
          Mod+Space { spawn "ignis" "toggle-window" "launcher"; }
          Mod+P { screenshot; }

          // Volume controls
          XF86AudioRaiseVolume allow-when-locked=true { spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "5%+"; }
          XF86AudioLowerVolume allow-when-locked=true { spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "5%-"; }
          XF86AudioMute allow-when-locked=true { spawn "wpctl" "set-mute" "@DEFAULT_AUDIO_SINK@" "toggle"; }
          XF86AudioMicMute allow-when-locked=true { spawn "wpctl" "set-mute" "@DEFAULT_AUDIO_SOURCE@" "toggle"; }

          // Media controls
          XF86AudioPlay allow-when-locked=true { spawn "playerctl" "play-pause"; }
          XF86AudioNext allow-when-locked=true { spawn "playerctl" "next"; }
          XF86AudioPrev allow-when-locked=true { spawn "playerctl" "previous"; }
          XF86AudioStop allow-when-locked=true { spawn "playerctl" "stop"; }

          // Brightness controls
          XF86MonBrightnessUp allow-when-locked=true { spawn "brightnessctl" "--class=backlight" "set" "+10%"; }
          XF86MonBrightnessDown allow-when-locked=true { spawn "brightnessctl" "--class=backlight" "set" "10%-"; }

          // Windows/workspaces
          Mod+T { toggle-column-tabbed-display; }
          Mod+Q { close-window; }
          Mod+O repeat=false { toggle-overview; }

          Mod+H { focus-column-left; }
          Mod+L { focus-column-right; }
          Mod+K { focus-window-or-workspace-up; }
          Mod+J { focus-window-or-workspace-down; }

          Mod+Ctrl+H { move-column-left; }
          Mod+Ctrl+L { move-column-right; }
          Mod+Ctrl+K { move-window-to-workspace-up; }
          Mod+Ctrl+J { move-window-to-workspace-down; }

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
