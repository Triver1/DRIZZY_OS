{ config, pkgs, lib, ... }:

{
  # System packages mirroring niri and basic hyprland dependencies
  environment.systemPackages = with pkgs; [
    hyprshot
    hyprpicker
    wofi
    fuzzel
    walker
    hyprlock
    brightnessctl
    gnome-bluetooth
  ];

  programs.hyprland = {
    enable = true; 
    xwayland.enable = true;
  };
  
  environment.sessionVariables.NIXOS_OZONE_WL = "1";
  
  # Home manager configuration for hyprland
  home-manager.sharedModules = [{
    services = {
      swayidle = {
        enable = true;
        timeouts = [
          {
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
    programs.ghostty = {
      settings = {
        window-decoration = false;
        background-opacity = lib.mkDefault 0.9;
        background-blur-radius = 8;
      };
    };
    programs.hyprlock = {
      enable = true;
      settings = {
        general = {
        };
      };
    };

    wayland.windowManager.hyprland = {
      enable = true;
      settings = {
        "$mod" = "SUPER";

        # Start the Caelestia shell through its Home Manager systemd unit.
        exec-once = [
          "systemctl --user start caelestia.service"
        ];

        env = [
          "QT_QPA_PLATFORM,wayland"
          "GDK_BACKEND,wayland,x11"
          "ELECTRON_OZONE_PLATFORM_HINT,auto"
          "TERMINAL,ghostty"
        ];

        layerrule = [
          "ignore_alpha 0.35, match:namespace caelestia.*"
          "ignore_alpha 0.35, match:namespace quickshell.*"
          "blur on, match:namespace (rofi|wofi|walker|notifications)"
          "ignore_alpha 0.35, match:namespace (rofi|wofi|walker|notifications)"
        ];

        general = {
          gaps_in = 8;
          gaps_out = 16;
          border_size = 2;
          resize_on_border = true;
          extend_border_grab_area = 18;
          "col.active_border" = lib.mkForce "rgba(d8a657ee)";
          "col.inactive_border" = lib.mkForce "rgba(3c383680)";
          layout = "scrolling";
        };

        scrolling = {
          column_width = 0.62;
          explicit_column_widths = "0.333, 0.5, 0.62, 0.75, 1.0";
          focus_fit_method = 1;
          follow_focus = true;
          follow_min_visible = 0.35;
          fullscreen_on_one_column = true;
          direction = "right";
        };

        decoration = {
          rounding = 14;
          rounding_power = 2;
          active_opacity = 0.96;
          inactive_opacity = 0.86;
          fullscreen_opacity = 1.0;
          blur = {
            enabled = true;
            size = 6;
            passes = 2;
            vibrancy = "0.18";
            vibrancy_darkness = "0.25";
            contrast = "1.05";
            brightness = "0.95";
            noise = "0.01";
            popups = true;
            popups_ignorealpha = "0.65";
            ignore_opacity = true;
            new_optimizations = true;
            xray = false;
          };
          shadow = {
            enabled = true;
            range = 14;
            render_power = 2;
            color = lib.mkForce "rgba(00000055)";
            color_inactive = lib.mkForce "rgba(00000030)";
          };
        };

        animations = {
          enabled = true;
          bezier = [
            "easeOutQuint, 0.23, 1, 0.32, 1"
            "easeInOutCubic, 0.65, 0.05, 0.36, 1"
            "md3_decel, 0.05, 0.7, 0.1, 1"
            "linear, 0, 0, 1, 1"
            "overshot, 0.13, 0.99, 0.29, 1.1"
          ];
          animation = [
            "windows, 1, 2, md3_decel, popin 94%"
            "windowsIn, 1, 2, md3_decel, popin 94%"
            "windowsOut, 1, 2, easeInOutCubic, popin 96%"
            "windowsMove, 1, 3, easeOutQuint"
            "border, 0"
            "borderangle, 0"
            "fade, 1, 4, md3_decel"
            "fadeDim, 1, 3, md3_decel"
            "workspaces, 1, 3, md3_decel, slide"
          ];
        };

        dwindle = {
          pseudotile = true;
          preserve_split = true;
        };

        input = {
          kb_layout = "us";
          kb_options = "caps:escape";
          follow_mouse = 1;
          touchpad = {
            natural_scroll = true;
            tap-to-click = true;
          };
        };

        gesture = [
          "3, horizontal, workspace"
        ];

        misc = {
          disable_hyprland_logo = true;
          disable_splash_rendering = true;
          focus_on_activate = true;
          vfr = false;
          vrr = 0;
        };

        windowrule = [
          "match:class ^(ghostty)$, opacity 1.0 1.0"
          "match:class ^(kitty)$, opacity 0.82 0.82"
          "match:class ^(org.pulseaudio.pavucontrol)$, float true"
          "match:class ^(blueman-manager)$, float true"
          "match:class ^(nm-connection-editor)$, float true"
          "match:title ^(Picture-in-Picture)$, float true"
          "match:title ^(Picture-in-Picture)$, pin true"
          "match:title ^(Picture-in-Picture)$, size 900 600"
          "match:title ^(Picture-in-Picture)$, center true"
        ];

        # Keybinds mapping from Niri configuration
        bind = [
          "$mod, Return, exec, ghostty"
          "$mod, B, exec, helium"
          "$mod, E, exec, ghostty -e yazi"
          "$mod, Backspace, exec, hyprlock"
          
          # System
          "$mod, Space, global, caelestia:launcher"
          "$mod, C, global, caelestia:sidebar"
          "$mod, D, global, caelestia:showall"
          "$mod, X, global, caelestia:session"
          "$mod SHIFT, N, global, caelestia:clearNotifs"
          "$mod, W, exec, caelestia wallpaper -r"
          "$mod SHIFT, W, exec, caelestia wallpaper"
          "$mod SHIFT, R, exec, caelestia shell -d"
          "$mod, P, exec, hyprshot -m region"
          "$mod SHIFT, P, exec, hyprpicker -a"

          # Window / Workspace Management
          "$mod, Q, killactive"
          "$mod, V, togglefloating"
          "$mod, F, fullscreen"

          "$mod, H, layoutmsg, focus l"
          "$mod, L, layoutmsg, focus r"
          "$mod, K, layoutmsg, focus u"
          "$mod, J, layoutmsg, focus d"
          "$mod, comma, layoutmsg, move -col"
          "$mod, period, layoutmsg, move +col"
          "$mod SHIFT, comma, layoutmsg, swapcol l"
          "$mod SHIFT, period, layoutmsg, swapcol r"
          "$mod CTRL, comma, layoutmsg, colresize -conf"
          "$mod CTRL, period, layoutmsg, colresize +conf"
          "$mod, O, layoutmsg, fit active"
          "$mod SHIFT, O, layoutmsg, fit visible"

          "$mod CTRL, H, movewindow, l"
          "$mod CTRL, L, movewindow, r"
          "$mod CTRL, K, movewindow, u"
          "$mod CTRL, J, movewindow, d"

          "$mod, 1, workspace, 1"
          "$mod, 2, workspace, 2"
          "$mod, 3, workspace, 3"
          "$mod, 4, workspace, 4"
          "$mod, 5, workspace, 5"
          "$mod, 6, workspace, 6"
          "$mod, 7, workspace, 7"
          "$mod, 8, workspace, 8"
          "$mod, 9, workspace, 9"

          "$mod CTRL, 1, movetoworkspace, 1"
          "$mod CTRL, 2, movetoworkspace, 2"
          "$mod CTRL, 3, movetoworkspace, 3"
          "$mod CTRL, 4, movetoworkspace, 4"
          "$mod CTRL, 5, movetoworkspace, 5"
          "$mod CTRL, 6, movetoworkspace, 6"
          "$mod CTRL, 7, movetoworkspace, 7"
          "$mod CTRL, 8, movetoworkspace, 8"
          "$mod CTRL, 9, movetoworkspace, 9"

          # Volume Controls
          ", XF86AudioRaiseVolume, exec, wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+"
          ", XF86AudioLowerVolume, exec, wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"
          ", XF86AudioMute, exec, wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"
          ", XF86AudioMicMute, exec, wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle"
          
          # Media Controls
          ", XF86AudioPlay, exec, playerctl play-pause"
          ", XF86AudioNext, exec, playerctl next"
          ", XF86AudioPrev, exec, playerctl previous"
          ", XF86AudioStop, exec, playerctl stop"
          
          # Brightness Controls
          ", XF86MonBrightnessUp, exec, brightnessctl --class=backlight set +10%"
          ", XF86MonBrightnessDown, exec, brightnessctl --class=backlight set 10%-"
        ];

        bindm = [
          "$mod, mouse:272, movewindow"
          "$mod, mouse:273, resizewindow"
        ];
      };
    };
  }];
}
