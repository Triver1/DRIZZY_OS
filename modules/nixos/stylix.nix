{ config, lib, pkgs, ... }:

let
  # Theme variants - easily switch between these
  themes = {
    # Dark themes
    rosePineDark = {
      scheme = "${pkgs.base16-schemes}/share/themes/rose-pine.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 36;
      };
    };

    rosePineMoon = {
      scheme = "${pkgs.base16-schemes}/share/themes/rose-pine-moon.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 36;
      };
    };

    gruvboxDarkHard = {
      scheme = "${pkgs.base16-schemes}/share/themes/gruvbox-dark-hard.yaml";
      cursor = {
        package = pkgs.capitaine-cursors;
        name = "capitaine-cursors";
        size = 36;
      };
    };

    gruvboxDarkMedium = {
      scheme = "${pkgs.base16-schemes}/share/themes/gruvbox-dark-medium.yaml";
      cursor = {
        package = pkgs.capitaine-cursors;
        name = "capitaine-cursors";
        size = 36;
      };
    };

    gruvboxModernDark = {
      scheme = {
        base00 = "1d2021";
        base01 = "282828";
        base02 = "3c3836";
        base03 = "665c54";
        base04 = "928374";
        base05 = "d4be98";
        base06 = "ddc7a1";
        base07 = "fbf1c7";
        base08 = "ea6962";
        base09 = "e78a4e";
        base0A = "d8a657";
        base0B = "a9b665";
        base0C = "89b482";
        base0D = "7daea3";
        base0E = "d3869b";
        base0F = "bd6f3e";
      };
      cursor = {
        package = pkgs.capitaine-cursors;
        name = "capitaine-cursors";
        size = 36;
      };
    };

    sonokaiAndromeda = {
      scheme = {
        base00 = "2b2d3a"; # background
        base01 = "333648"; # lighter background
        base02 = "363a4e"; # selection background
        base03 = "5a5e7a"; # comments
        base04 = "7e8294"; # muted foreground
        base05 = "e1e3e4"; # default foreground
        base06 = "e1e3e4"; # light foreground
        base07 = "e1e3e4"; # lightest foreground
        base08 = "fb617e"; # red
        base09 = "f89860"; # orange
        base0A = "edc763"; # yellow
        base0B = "9ed06c"; # green
        base0C = "6dcae8"; # cyan
        base0D = "6dcae8"; # blue
        base0E = "bb97ee"; # purple
        base0F = "f89860"; # brown/orange
      };
      cursor = {
        package = pkgs.capitaine-cursors;
        name = "capitaine-cursors";
        size = 36;
      };
    };

    ponokai = {
      scheme = {
        base00 = "2b2d3a"; # background
        base01 = "333648"; # lighter background
        base02 = "363a4e"; # selection background
        base03 = "9da1af"; # comments
        base04 = "c4c6cf"; # muted foreground
        base05 = "f2f2f3"; # default foreground
        base06 = "f2f2f3"; # light foreground
        base07 = "f2f2f3"; # lightest foreground
        base08 = "ff8c9a"; # red
        base09 = "f3bb9a"; # orange
        base0A = "f8e7b0"; # yellow
        base0B = "b4e49a"; # green
        base0C = "98d4e7"; # cyan
        base0D = "98d4e7"; # blue
        base0E = "bdb2ff"; # purple
        base0F = "f3bb9a"; # brown/orange
      };
      cursor = {
        package = pkgs.capitaine-cursors;
        name = "capitaine-cursors";
        size = 36;
      };
    };

    tokyoNight = {
      scheme = "${pkgs.base16-schemes}/share/themes/tokyo-night-storm.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 32;
      };
    };

    draculaTheme = {
      scheme = "${pkgs.base16-schemes}/share/themes/dracula.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 32;
      };
    };

    nord = {
      scheme = "${pkgs.base16-schemes}/share/themes/nord.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 32;
      };
    };

    oneDark = {
      scheme = "${pkgs.base16-schemes}/share/themes/one-dark.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 32;
      };
    };

    # Light themes
    rosePineDawn = {
      scheme = "${pkgs.base16-schemes}/share/themes/rose-pine-dawn.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePineDawn-Linux";
        size = 36;
      };
    };

    gruvboxLight = {
      scheme = "${pkgs.base16-schemes}/share/themes/gruvbox-light-hard.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePineDawn-Linux";
        size = 36;
      };
    };

    solarizedLight = {
      scheme = "${pkgs.base16-schemes}/share/themes/solarized-light.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePineDawn-Linux";
        size = 32;
      };
    };

    github = {
      scheme = "${pkgs.base16-schemes}/share/themes/github.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePineDawn-Linux";
        size = 32;
      };
    };

    # Specialty themes
    catppuccinMocha = {
      scheme = "${pkgs.base16-schemes}/share/themes/catppuccin-mocha.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 32;
      };
    };

    catppuccinLatte = {
      scheme = "${pkgs.base16-schemes}/share/themes/catppuccin-latte.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePineDawn-Linux";
        size = 32;
      };
    };

    # Custom theme - our dark Rosé Pine × Claude blend
    customRosePineClaudeDark = {
      scheme = {
        base00 = "191724";
        base01 = "1f1d2e";
        base02 = "26233a";
        base03 = "6e6a86";
        base04 = "908caa";
        base05 = "e0def4";
        base06 = "f6f4ff";
        base07 = "faf8ff";
        base08 = "eb6f92";
        base09 = "f6c177";
        base0A = "ebbcba";
        base0B = "31748f";
        base0C = "9ccfd8";
        base0D = "c4a7e7";
        base0E = "c4a7e7";
        base0F = "e0def4";
      };
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 36;
      };
    };

    # Custom theme - Another blend of Rosé Pine and Claude
    claudePine = {
      scheme = {
        base00 = "191724"; # Darker background for better contrast
        base01 = "1f1d2e"; # Slightly purple-tinted dark
        base02 = "26233a"; # Deep purple-gray
        base03 = "6e6a86"; # Muted purple gray
        base04 = "908caa"; # Light purple gray
        base05 = "e0def4"; # Soft lavender
        base06 = "f6f4ff"; # Bright lavender
        base07 = "faf8ff"; # Nearly white lavender
        base08 = "eb6f92"; # Soft pink
        base09 = "f6c177"; # Warm gold
        base0A = "ebbcba"; # Rose gold
        base0B = "9ccfd8"; # Soft cyan
        base0C = "c4a7e7"; # Light purple
        base0D = "8b7ec8"; # Medium purple
        base0E = "e0def4"; # Lavender
        base0F = "524f67"; # Deep purple gray
      };
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 36;
      };
    };

    # New theme: Kanagawa
    kanagawa = {
      scheme = "${pkgs.base16-schemes}/share/themes/kanagawa.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePine-Linux";
        size = 36;
      };
    };

    # Custom theme: Iceberg Terminal Color Scheme
iceberg = {
  scheme = {
    base00 = "161821"; # background
    base01 = "1e2132"; # lighter background
    base02 = "232637"; # selection background
    base03 = "6b7089"; # comments
    base04 = "8a8fa8"; # muted foreground
    base05 = "c6c8d1"; # default foreground
    base06 = "d2d4de"; # lighter foreground
    base07 = "e4e6ef"; # lightest foreground
    base08 = "e27878"; # red
    base09 = "e2a478"; # orange
    base0A = "b4be82"; # yellow
    base0B = "89b8c2"; # green/cyan
    base0C = "84a0c6"; # cyan/blue
    base0D = "a093c7"; # blue/purple
    base0E = "ada0d3"; # magenta
    base0F = "c0a36e"; # brown
  };
  cursor = {
    package = pkgs.capitaine-cursors;
    name = "capitaine-cursors";
    size = 42;
  };
};
  };

  # Choose your active theme here - just change this variable!
  activeTheme = "ponokai"; # Options: ponokai, sonokaiAndromeda, rosePineDark, gruvboxModernDark, etc.

  selectedTheme = themes.${activeTheme};

in
{
  stylix = {
    enable = true;
    autoEnable = true;

    # Theme configuration - use inline scheme or external file
    base16Scheme = selectedTheme.scheme;

    # Cursor configuration
    cursor = selectedTheme.cursor;

    # Optional: Additional stylix configurations

    # Optional: Image for wallpaper generation
    # image = ./path/to/your/wallpaper.jpg;

    # Optional: Polarity override (auto, light, dark)
    # polarity = "dark";

    # Optional: Opacity settings
    opacity = {
      applications = 0.95;
      terminal = 0.95;
      desktop = 1.0;
      popups = 0.95;
    };

    # Qt platform configuration
    targets.qt.enable = true;
    targets.qt.platform = lib.mkDefault "gnome";
  };

  qt.platformTheme = "gnome";
}

# Usage Examples:
# 1. Change activeTheme to any theme from the themes attrset
# 2. Add new themes to the themes attrset following the same pattern
# 3. Customize cursor sizes, fonts, opacity, etc.

# Popular theme combinations:
# - activeTheme = "ponokai"; # Pastel Monokai-style dark theme
# - activeTheme = "sonokaiAndromeda"; # Vivid Monokai-style dark theme
# - activeTheme = "rosePineDark"; # Cozy, warm dark theme
# - activeTheme = "gruvboxDarkHard"; # High contrast, retro feel
# - activeTheme = "tokyoNight"; # Modern, vibrant dark theme
# - activeTheme = "rosePineDawn"; # Elegant light theme
# - activeTheme = "customRosePineClaudeDark"; # Our custom blend
# - activeTheme = "iceberg"; # Iceberg terminal-inspired cool dark theme
