# niri-nimbus.nix — drop-in alternative to niri.nix, styled to match the
# "nimbus" shell (matte black tabs, Iris accent gradient, spring motion).
# Same packages, services, keybinds, outputs, input and startup lines as
# niri.nix; only the look (layout / rules / animations / overview) differs.
# Import this INSTEAD of niri.nix (not alongside it).
#
# Accent: Iris (#8fb3ff -> #c69bff), from ~/.config/nimbus/config `accent = 0`.
# To follow another nimbus accent, swap the two hexes in `active-gradient`
# (focus-ring, tab-indicator, insert-hint):
#   1 Mint  #7ee2b8 #5fd4d9   2 Coral #ff9a76 #ff6f91   3 Amber #ffc069 #ff9348
#   4 Ice   #d9e6ff #9fc2ff   5 Rose  #ffa3c7 #d59bff
{ config, pkgs, lib, ... }:

{
  imports = [ ./nimbus-files-default.nix ];

  environment.systemPackages = with pkgs; [
    xwayland-satellite
    fuzzel
    walker
    hyprlock
    brightnessctl
    gnome-bluetooth
    wf-recorder # nimbus: screen-record quick toggle
  ];

  services.xserver.enable = true;

  programs.niri = {
    enable = true;
    package = pkgs.niri;
  };
  environment.sessionVariables.NIXOS_OZONE_WL = "1";

  # nimbus's own screen locker (`nimbus lock`, Lock buttons in the shell):
  # a minimal pam_unix-only stack. Deliberately not the `su`/`login` stack:
  # pam_faillock there would lock the account everywhere after a few wrong
  # guesses at the lock screen.
  security.pam.services.nimbus = { };

  # Stylix (themes GTK, Qt, Firefox, editors, …) in nimbus's palette instead
  # of stylix.nix's activeTheme, so every app matches the shell. Forced here
  # so stylix.nix itself stays as is: importing niri.nix again restores it.
  stylix.polarity = lib.mkForce "dark";
  stylix.base16Scheme = lib.mkForce {
    base00 = "0e0e11"; # background (matte black body)
    base01 = "17171c"; # raised surfaces (cards, sidebars)
    base02 = "24242c"; # selection / hover
    base03 = "6e6e78"; # comments, disabled
    base04 = "8a8c97"; # muted text
    base05 = "f2f2f5"; # text
    base06 = "f5f5f7";
    base07 = "ffffff";
    base08 = "ff7b93"; # red (bad)
    base09 = "ffb38a"; # orange
    base0A = "ffd37a"; # yellow (warn)
    base0B = "7ee2b8"; # green (mint)
    base0C = "5fd4d9"; # cyan (teal)
    base0D = "8fb3ff"; # blue: Iris start, the accent GTK/Qt use
    base0E = "c69bff"; # purple: Iris end
    base0F = "ff9a76"; # coral
  };
  # The shell's UI font everywhere (was DejaVu).
  stylix.fonts = {
    sansSerif = lib.mkForce { package = pkgs.geist-font; name = "Geist"; };
    monospace = lib.mkForce { package = pkgs.geist-font; name = "Geist Mono"; };
    sizes = lib.mkForce { applications = 11; desktop = 10; popups = 10; terminal = 10; };
  };
  stylix.cursor.size = lib.mkForce 24;

  # nimbus is the session's polkit agent (src/polkit.rs): it shows the
  # password prompt when an app asks for admin rights. This stock agent is
  # only a fallback, so nothing starts it at login: polkit takes one agent
  # per session, the first to register wins, and a second one started
  # alongside would race nimbus for the slot. run.sh starts it when nimbus
  # has crashed for good. By hand:
  #   systemctl --user start polkit-gnome-agent
  # It keeps the slot until it stops; nimbus then registers within a minute.
  systemd.user.services.polkit-gnome-agent = {
    description = "polkit authentication agent (fallback for nimbus)";
    after = [ "graphical-session.target" ];
    serviceConfig = {
      ExecStart = "${pkgs.polkit_gnome}/libexec/polkit-gnome-authentication-agent-1";
      TimeoutStopSec = 10;
    };
  };



# nimbus-matching terminal (Ghostty, Starship, fzf, yazi, tmux). Only active
# with this module, so switching back to niri.nix restores the old look.
home-manager.sharedModules = [ ../home-manager/nimbus-terminal.nix { 
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
          command = "/home/drizzy/NEWFLAKE/modules/no-nix/nimbus/target/release/nimbus lock --daemonize || hyprlock";
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
      // The nimbus shell (bar, popout, launcher, notifications, tray,
      // wallpaper). The old QML shell: modules/no-nix/qml_shell/run.sh
      spawn-at-startup "sh" "/home/drizzy/NEWFLAKE/modules/no-nix/nimbus/run.sh"
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
      // nimbus draws its own shadows/rims: no compositor shadow. Blur
      // follows the region nimbus asks for (ext-background-effect: exactly
      // its ink panels), over the real windows below (no xray).
      layer-rule {
          match namespace="^nimbus"
          shadow {
              off
          }
          background-effect {
              xray false
          }
      }
      // nimbus's frosted copy of the wallpaper: the overview backdrop and
      // the gap between workspaces while switching.
      layer-rule {
          match namespace="^nimbus-backdrop$"
          place-within-backdrop true
      }
      // Radius grows with offset * 2^passes. Much past this it averages
      // to one flat colour and stops reading as glass.
      blur {
          passes 5
          offset 8
          noise 0.02
          saturation 1.35
      }
      cursor {
          xcursor-theme "capitaine-cursors"
          xcursor-size 24
      }
      // Spring-only motion, tuned to nimbus (anim.rs presets: stiffness =
      // (2*pi / response)^2, mass 1). Layout is critically damped (no wobble);
      // window-open and the little overlays get a touch of underdamped life.
      animations {
          slowdown 1.0

          workspace-switch {
              spring damping-ratio=1.0 stiffness=700 epsilon=0.0001
          }

          horizontal-view-movement {
              spring damping-ratio=1.0 stiffness=620 epsilon=0.0001
          }

          window-movement {
              spring damping-ratio=0.86 stiffness=640 epsilon=0.0001
          }

          window-resize {
              spring damping-ratio=1.0 stiffness=640 epsilon=0.0001
          }

          window-open {
              spring damping-ratio=0.74 stiffness=480 epsilon=0.001
          }

          window-close {
              spring damping-ratio=1.0 stiffness=900 epsilon=0.001
          }

          overview-open-close {
              spring damping-ratio=1.0 stiffness=560 epsilon=0.0001
          }

          recent-windows-close {
              spring damping-ratio=1.0 stiffness=800 epsilon=0.001
          }

          config-notification-open-close {
              spring damping-ratio=0.66 stiffness=900 epsilon=0.001
          }

          exit-confirmation-open-close {
              spring damping-ratio=0.7 stiffness=560 epsilon=0.01
          }

          screenshot-ui-open {
              spring damping-ratio=1.0 stiffness=900 epsilon=0.001
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
          // 9 px = nimbus TAB_MARGIN. The bar's 40 px exclusive zone already
          // clears the 32 px hanging tabs, so windows start 40 + 9 = 49 px
          // down; no extra top strut is needed.
          gaps 9
          struts {
              left 0
              right 0
              top 0
              bottom 0
          }
          // Transparent: the wallpaper (placed within the backdrop) shows
          // through and stays still while workspaces slide.
          background-color "transparent"
          default-column-width { proportion 0.5; }
          preset-column-widths {
              proportion 0.33333
              proportion 0.5
              proportion 0.66667
          }

          // Accent gradient ring around the focused window.
          focus-ring {
              on
              width 1.25
              active-gradient from="#8fb3ff" to="#c69bff" angle=135 in="oklab"
              // focused window on an unfocused monitor: a faint hairline
              inactive-color "#ffffff14"
              urgent-gradient from="#ff9a76" to="#ff6f91" angle=135 in="oklab"
          }

          // nimbus's top-lit 1 px rim (white ~9% -> 3%). Off: niri stacks
          // it inside the focus ring, which made the outline too heavy.
          border {
              off
              width 1
              active-gradient from="#ffffff17" to="#ffffff08" angle=180
              inactive-gradient from="#ffffff0f" to="#ffffff05" angle=180
              urgent-color "#ff6f9166"
          }

          // Deep, soft shadow like the shell's floating panels.
          shadow {
              on
              softness 30
              spread 2
              offset x=0 y=6
              draw-behind-window false
              color "#0000008c"
              inactive-color "#00000059"
          }

          tab-indicator {
              on
              hide-when-single-tab
              place-within-column
              gap 6
              width 3
              length total-proportion=1.0
              position "left"
              gaps-between-tabs 3
              corner-radius 2
              active-gradient from="#8fb3ff" to="#c69bff" angle=180 in="oklab"
              inactive-color "#ffffff1f"
              urgent-color "#ff6f91"
          }

          insert-hint {
              on
              gradient from="#8fb3ff40" to="#c69bff40" angle=135 in="oklab"
          }
      }


      // Overview
      overview {
          backdrop-color "#09090b"
          workspace-shadow {
              on
              softness 60
              spread 8
              offset x=0 y=18
              color "#00000080"
          }
      }

      hotkey-overlay {
          skip-at-startup
      }

      recent-windows {
          // off
          debounce-ms 750

          open-delay-ms 150

          highlight {
              // Iris mid-tone, translucent over the matte backdrop
              active-color "#aaa7ff66"
              urgent-color "#ff6f91b3"
              padding 20
              corner-radius 26
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
          Mod+Backspace { spawn-sh "/home/drizzy/NEWFLAKE/modules/no-nix/nimbus/target/release/nimbus lock || hyprlock"; }
          // Recovery: if the nimbus lock screen ever hangs, this starts
          // hyprlock, which takes over the lock (niri refuses it while a
          // live locker holds the session, so it can't bypass anything).
          Mod+Alt+L allow-when-locked=true { spawn "hyprlock"; }
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
      // Windows: radius 11, the same as the bar's tabs.
      // The border is a translucent rim, so never paint it as a fill
      // behind (possibly translucent) windows.
      window-rule {
          geometry-corner-radius 11
          clip-to-geometry true
          draw-border-with-background false

          background-effect {
              xray true
              blur true
              saturation 1.35
          }

          popups {
              geometry-corner-radius 8

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
          geometry-corner-radius 11
          clip-to-geometry true

          background-effect {
              xray true
              blur true
              saturation 1.0
              noise 0.0
          }

          popups {
              geometry-corner-radius 8

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
          geometry-corner-radius 11
          clip-to-geometry true

          background-effect {
              blur true
              saturation 1.25
              noise 0.015
          }

          popups {
              opacity 0.96
              geometry-corner-radius 8

              background-effect {
                  blur true
                  saturation 1.25
                  noise 0.015
              }
          }
      }

      // Floating windows read like nimbus's floating panels:
      // lifted higher with a deeper, softer shadow. Kept last so it wins
      // over the per-app radius above.
      window-rule {
          match is-floating=true
          geometry-corner-radius 11

          shadow {
              on
              softness 48
              spread 4
              offset x=0 y=14
              color "#000000b3"
              inactive-color "#00000080"
          }
      }

      '';
  }];
}
