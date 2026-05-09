{ config, pkgs, lib, ... }:

{
  # System packages mirroring niri and basic hyprland dependencies
  environment.systemPackages = with pkgs; [
    hyprshot
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

    wayland.windowManager.hyprland = {
      enable = true;
      settings = {
        "$mod" = "SUPER";

        # Autostart Dank Material Shell (dms)
        exec-once = [
          "dms run"
        ];

        env = [
          "QT_QPA_PLATFORM,wayland"
        ];

	        layerrule = [
	          "blur on, match:namespace ^(dms)$"
	          "ignore_alpha 0, match:namespace ^(dms)$"
	        ];

        general = {
          gaps_in = 7;
          gaps_out = 7;
          border_size = 2;
          "col.active_border" = lib.mkForce "rgba(ffffff95) rgba(f0f8ff30) 135deg";
          "col.inactive_border" = lib.mkForce "rgba(64646430) rgba(50505010) 135deg";
          layout = "dwindle";
        };

	        decoration = {
	          rounding = 10;
	          blur = {
	            enabled = true;
	            size = 8;
	            passes = 3;
	            vibrancy = "0.1696";
	            ignore_opacity = true;
	          };
	          shadow = {
	            enabled = true;
	            range = 20;
	            render_power = 4;
	            color = lib.mkForce "rgba(00000080)";
	          };
	        };

        animations = {
          enabled = true;
          bezier = [
            "wind, 0.05, 0.9, 0.1, 1.05"
            "winIn, 0.1, 1.1, 0.1, 1.1"
            "winOut, 0.3, -0.3, 0, 1"
            "liner, 1, 1, 1, 1"
            "md3_decel, 0.05, 0.7, 0.1, 1"
          ];
          animation = [
            "windows, 1, 4, wind, slide"
            "windowsIn, 1, 4, winIn, slide"
            "windowsOut, 1, 4, winOut, slide"
            "windowsMove, 1, 4, wind, slide"
            "border, 1, 1, liner"
            "borderangle, 1, 30, liner, loop"
            "fade, 1, 10, default"
            "workspaces, 1, 4, md3_decel, slide"
          ];
        };

        input = {
          kb_layout = "us";
          kb_options = "caps:escape";
          touchpad = {
            natural_scroll = true;
            tap-to-click = true;
          };
        };

        # Keybinds mapping from Niri configuration
        bind = [
          "$mod, Return, exec, kitty"
          "$mod, B, exec, helium"
          "$mod, E, exec, kitty -e yazi"
          "$mod, Backspace, exec, hyprlock"
          
          # System
          "$mod, Space, exec, ignis run-command open-app-launcher"
          "$mod, P, exec, hyprshot -m region"

          # Window / Workspace Management
          "$mod, Q, killactive"
          "$mod, V, togglefloating"
          "$mod, F, fullscreen"

          "$mod, H, movefocus, l"
          "$mod, L, movefocus, r"
          "$mod, K, movefocus, u"
          "$mod, J, movefocus, d"

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
