{ config, pkgs, lib, ... }:

{
  environment.systemPackages = with pkgs; [
    xwayland-satellite
    fuzzel
    walker
    hyprlock
    brightnessctl
    gnome-bluetooth
  ];

  services.xserver.enable = true;

  programs.niri = {
    enable = true;
    package = pkgs.niri;
  };
  environment.sessionVariables.NIXOS_OZONE_WL = "1";



home-manager.sharedModules = [{ 
  services = {
    swayidle = {
      enable = true;
      timeouts = [        {
          timeout = 2000;
          command = "${pkgs.systemd}/bin/systemctl suspend";
        }
        

      ];
      events = [
        {
          event = "before-sleep";
          command = "hyprlock";
        }
      ];
    };
  };      
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
      spawn-at-startup "sh" "/home/drizzy/NEWFLAKE/modules/no-nix/qml_shell/run.sh"
        layer-rule {
          match namespace="backdrop"
          place-within-backdrop true
      }
      layer-rule {
          match namespace=r#"-blur$"#
          background-effect {
              xray true
              blur true
              saturation 1.35
          }
      }
      blur {
          passes 4
          offset 3.5
          noise 0.02
          saturation 1.35
      }
      animations {
          slowdown 1.08

          window-open {
              duration-ms 185
              curve "cubic-bezier" 0.12 0.92 0.18 1.0
          }

          window-close {
              duration-ms 175
              curve "cubic-bezier" 0.28 0.0 0.24 1.0
          }
      }
      environment {
          DISPLAY ":0"
      }
      // Laptop display configuration
      output "eDP-1" {
          // Use preferred mode (auto-detect)
          focus-at-startup
      }

      output "HDMI-A-1" {
          mode "3440x1440@100"
      }

      output "DP-2" {
          mode "3440x1440@100"
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
          gaps 9
          focus-ring {
              width 2
              active-gradient from="#f8e7b0ee" to="#b4e49a80" angle=135
              inactive-gradient from="#9da1af80" to="#33364840" angle=135
          }
      }
      

      // Overview
      overview {
          backdrop-color "#2b2d3a"
      }

      hotkey-overlay {
          skip-at-startup
      }

      recent-windows {
          // off
          debounce-ms 750

          open-delay-ms 150

          highlight {
              active-color "#f2f2f3ff"
              urgent-color "#ff8c9aff"
              padding 30
              corner-radius 8
          }

          previews {
              max-height 480
              max-scale 0.5
          }

          binds {
              Alt+Tab         { next-window; }
              Alt+Shift+Tab   { previous-window; }
              Alt+grave       { next-window     filter="app-id"; }
              Alt+Shift+grave { previous-window filter="app-id"; }

              Mod+Tab         { next-window; }
              Mod+Shift+Tab   { previous-window; }
              Mod+grave       { next-window     filter="app-id"; }
              Mod+Shift+grave { previous-window filter="app-id"; }
          }
      }
      // Keybinds
      binds {
          // Programs
          Mod+Return { spawn "ghostty"; }
          Mod+B { spawn "helium";}
          Mod+E { spawn "ghostty" "-e" "yazi";}
          Mod+Backspace {spawn "hyprlock";}
          Mod+C { center-column; }


          // System  
          Mod+Space { spawn "sh" "-c" "date +%s > /tmp/qs-launcher-trigger"; }
          Ctrl+Mod+S { spawn "sh" "-c" "date +%s > /tmp/qs-settings-trigger"; }
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

          Mod+WheelScrollDown      cooldown-ms=150 { focus-workspace-down; }
          Mod+WheelScrollUp        cooldown-ms=150 { focus-workspace-up; }

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

          // Move workspace to monitor
          Mod+Ctrl+Shift+H {  move-column-to-monitor-previous; }
          Mod+Ctrl+Shift+L { move-column-to-monitor-next; }
          Mod+Shift+H { focus-monitor-previous; }
          Mod+Shift+L { focus-monitor-next; }

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
          geometry-corner-radius 16
          clip-to-geometry true

          background-effect {
              xray true
              blur true
              saturation 1.35
          }

          popups {
              geometry-corner-radius 12

              background-effect {
                  xray true
                  blur true
                  saturation 1.35
              }
          }
      }

      window-rule {
          match app-id=r#"^(com\.mitchellh\.ghostty|ghostty)$"#
          draw-border-with-background false
          geometry-corner-radius 16
          clip-to-geometry true

          background-effect {
              xray true
              blur true
              saturation 1.0
              noise 0.0
          }

          popups {
              geometry-corner-radius 12

              background-effect {
                  xray true
                  blur true
                  saturation 1.0
                  noise 0.0
              }
          }
      }

      window-rule {
          match app-id=r#"^(helium|Helium|net\.imput\.helium)$"#
          opacity 0.96
          geometry-corner-radius 16
          clip-to-geometry true

          background-effect {
              blur true
              saturation 1.25
              noise 0.015
          }

          popups {
              opacity 0.96
              geometry-corner-radius 12

              background-effect {
                  blur true
                  saturation 1.25
                  noise 0.015
              }
          }
      }

      '';
  }];
}
