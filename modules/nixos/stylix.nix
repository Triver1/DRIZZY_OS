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
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePineDawn-Linux";
        size = 36;
      };
    };

    gruvboxDarkMedium = {
      scheme = "${pkgs.base16-schemes}/share/themes/gruvbox-dark-medium.yaml";
      cursor = {
        package = pkgs.rose-pine-cursor;
        name = "BreezeX-RosePineDawn-Linux";
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
  };

  # Choose your active theme here - just change this variable!
  activeTheme = "claudePine"; # Options: rosePineDark, rosePineMoon, gruvboxDarkHard, etc.
  
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
    

  };
}

# Usage Examples:
# 1. Change activeTheme to any theme from the themes attrset
# 2. Add new themes to the themes attrset following the same pattern
# 3. Customize cursor sizes, fonts, opacity, etc.

# Popular theme combinations:
# - activeTheme = "rosePineDark"; # Cozy, warm dark theme
# - activeTheme = "gruvboxDarkHard"; # High contrast, retro feel - activeTheme = "tokyoNight"; # Modern, vibrant dark theme
# - activeTheme = "rosePineDawn"; # Elegant light theme
# - activeTheme = "customRosePineClaudeDark"; # Our custom blend 
