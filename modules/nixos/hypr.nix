{ config, pkgs, lib, ... }:

{

  
  # Install hyprshot and its dependencies system-wide
  environment.systemPackages = with pkgs; [
    hyprshot
    wofi
  ];

  programs.hyprland = {
    enable = true; 
    xwayland.enable = true;
  };# enable Hyprland
  environment.sessionVariables.NIXOS_OZONE_WL = "1";
  
  # Home manager configuration for hyprland
  home-manager.sharedModules = [{
    wayland.windowManager.hyprland = {
      
      enable = true;
      settings = {
        # Monitor configuration
        monitor = ", highrr, auto, 1";
        env = "WLR_NO_HARDWARE_CURSORS=1";
        # General settings
        general = {
          gaps_in = 5;
          gaps_out = 10;
          border_size = 2;
          layout = "master";
        };

        # Decoration settings
        decoration = {
          rounding = 10;
          blur = {
            enabled = true;
            size = 3;
            passes = 1;
          };
        };

        # Animation settings
        animations = {
          enabled = true;
          bezier = "myBezier, 0.05, 0.9, 0.1, 1.05";
          animation = [
            "windows, 1, 7, myBezier"
            "windowsOut, 1, 7, default, popin 80%"
            "border, 1, 10, default"
            "fade, 1, 7, default"
            "workspaces, 1, 6, default"
          ];
        };

        # Input settings
        input = {
          kb_layout = "us";
          follow_mouse = 1;
          kb_options = caps:escape;
          touchpad = {
            natural_scroll = true;
          };
           # kb_options = "caps:swapescape";
        };

        # Key bindings
        "$mod" = "SUPER";
        bind = [
          "$mod, Return, exec, kitty"
          "$mod, Q, killactive,"
          "$mod, E, exec, nautilus"
          "$mod, V, togglefloating,"
          "$mod, Space, exec, wofi --show drun"        
          "$mod, J, togglesplit,"
          "$mod, B, exec, firefox"      
                    # Screenshot bindings
    "$mod, P, exec, hyprshot -m window --clipboard-only"  # Screenshot a window
    "$mod SHIFT, P, exec, hyprshot -m region --clipboard-only"  # Screenshot a region
          
          # Move focus with vim keys
          "$mod, h, movefocus, l"
          "$mod, l, movefocus, r" 
          "$mod, k, movefocus, u"
          "$mod, j, movefocus, d"
          
          # Switch workspaces
          "$mod, 1, workspace, 1"
          "$mod, 2, workspace, 2"
          "$mod, 3, workspace, 3"
          "$mod, 4, workspace, 4"
          "$mod, 5, workspace, 5"
          "$mod, 6, workspace, 6"
          "$mod, 7, workspace, 7"
          "$mod, 8, workspace, 8"
          "$mod, 9, workspace, 9"
          "$mod, 0, workspace, 10"
          
          # Move active window to workspace
          "$mod SHIFT, 1, movetoworkspace, 1"
          "$mod SHIFT, 2, movetoworkspace, 2" 
          "$mod SHIFT, 3, movetoworkspace, 3"
          "$mod SHIFT, 4, movetoworkspace, 4"
          "$mod SHIFT, 5, movetoworkspace, 5"
          "$mod SHIFT, 6, movetoworkspace, 6"
          "$mod SHIFT, 7, movetoworkspace, 7"
          "$mod SHIFT, 8, movetoworkspace, 8"
          "$mod SHIFT, 9, movetoworkspace, 9"
          "$mod SHIFT, 0, movetoworkspace, 10"
        ];

        # Mouse bindings
        bindm = [
          "$mod, mouse:272, movewindow"
          "$mod, mouse:273, resizewindow"
        ];
      };
    };
  }];
}
