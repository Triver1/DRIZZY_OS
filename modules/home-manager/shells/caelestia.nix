{ config, lib, inputs, ... }:

let
  wallpaperDir = "/home/drizzy/NEWFLAKE/assets/wallpapers/rice";
in
{
  options.triverhome.shells.caelestia.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable the Caelestia Quickshell desktop shell.";
  };

  imports = [
    inputs.caelestia-shell.homeManagerModules.default
  ];

  config = lib.mkIf config.triverhome.shells.caelestia.enable {
    programs.caelestia = {
      enable = true;
      cli.enable = true;

      systemd = {
        enable = true;
        target = "hyprland-session.target";
        environment = [
          "TERMINAL=ghostty"
          "QT_QPA_PLATFORM=wayland"
          "QT_QPA_PLATFORMTHEME=gtk3"
        ];
      };

      settings = {
        enabled = true;

        appearance = {
          transparency = {
            enabled = true;
            base = 0.78;
            layers = 0.42;
          };
          anim.durations.scale = 0.85;
          rounding.scale = 1.05;
          spacing.scale = 1.0;
          font.family = {
            clock = "Rubik";
            material = "Material Symbols Rounded";
            mono = "CaskaydiaCove NF";
            sans = "Rubik";
          };
        };

        general = {
          logo = "caelestia";
          showOverFullscreen = false;
          apps = {
            terminal = [ "ghostty" ];
            audio = [ "pavucontrol" ];
            playback = [ "mpv" ];
            explorer = [ "nautilus" "yazi" ];
          };
        };

        background = {
          enabled = true;
          desktopClock.enabled = false;
          visualiser = {
            enabled = false;
            blur = true;
          };
        };

        bar = {
          persistent = true;
          showOnHover = false;
          activeWindow.showOnHover = true;
          workspaces = {
            shown = 9;
            activeIndicator = true;
            activeTrail = false;
            showWindows = true;
            perMonitorWorkspaces = true;
          };
          status = {
            showAudio = true;
            showBattery = true;
            showBluetooth = true;
            showNetwork = true;
            showWifi = true;
            showLockStatus = true;
            showMicrophone = false;
            showKbLayout = false;
          };
          tray = {
            background = false;
            compact = false;
            recolour = false;
          };
        };

        border = {
          rounding = 18;
          smoothing = 12;
          thickness = 6;
        };

        dashboard = {
          enabled = true;
          showDashboard = true;
          showMedia = true;
          showPerformance = true;
          showWeather = true;
          showOnHover = true;
        };

        launcher = {
          useFuzzy = {
            apps = true;
            actions = true;
            schemes = true;
            variants = true;
            wallpapers = true;
          };
          vimKeybinds = false;
          favouriteApps = [ "zen" "ghostty" "org.gnome.Nautilus" ];
        };

        notifs = {
          expire = true;
          fullscreen = false;
          defaultExpireTimeout = 5000;
        };

        paths = {
          wallpaperDir = wallpaperDir;
        };

        services = {
          defaultPlayer = "spotify";
          audioIncrement = 5;
          brightnessIncrement = 10;
          visualiserBars = 48;
          weatherLocation = "Ghent";
          useTwelveHourClock = false;
        };
      };

      cli.settings = {
        theme.enable = true;
        wallpaper = {
          dir = wallpaperDir;
          backend = "hyprpaper";
        };
      };
    };
  };
}
