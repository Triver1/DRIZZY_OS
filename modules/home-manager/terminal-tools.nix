{ config, lib, pkgs, ... }:

let
  types = lib.types;
  triver = config.triverhome or {};
  nvimCfg = triver.nvim or {};
  nvimEnable = nvimCfg.enable or true;
in
{
  options.triverhome.nvim.enable = lib.mkOption {
    type = types.bool;
    default = true;
    description = "Enable Neovim and link external config.";
  };
  options.triverhome.terminals.ghostty.enable = lib.mkOption {
    type = types.bool;
    default = true;
    description = "Enable Ghostty terminal and Stylix theming.";
  };

  options.triverhome.terminals.kitty.enable = lib.mkOption {
    type = types.bool;
    default = true;
    description = "Enable Kitty terminal and Stylix theming.";
  };

  config = {
  # Neovim configuration
  home.packages = with pkgs; [
    stdenv.cc
    gemini-cli
    starship
    ] ++ lib.optionals nvimEnable [ neovim ];
  
  # Link your neovim config from outside the nix store
  # Points to your config in the workspace relative to home directory
    xdg.configFile = lib.mkIf nvimEnable {
      nvim.source = config.lib.file.mkOutOfStoreSymlink ../no-nix/nvim;
    };

  # Tmux configuration
  programs.tmux = {
    enable = true;
    clock24 = true;
    plugins = with pkgs; [ 
            tmuxPlugins.yank
            tmuxPlugins.vim-tmux-navigator
            tmuxPlugins.tmux-powerline
            tmuxPlugins.power-theme
            tmuxPlugins.better-mouse-mode
          ];
    extraConfig = ''
      # Enable mouse control
      set -g mouse on

      # Enable vim-style controls
      setw -g mode-keys vi
      bind-key h select-pane -L
      bind-key j select-pane -D 
      bind-key k select-pane -U
      bind-key l select-pane -R
      set -g @tmux_power_theme 'gold'
    '';
  };

programs.starship = {
  enable = true;
  settings = {
    format = "╭─(bold blue) $directory \n╰─(bold blue)$character";
    
    directory = {
      format = "[$path]($style)";
      style = "bold blue";
      truncation_length = 0;
      truncate_to_repo = false;
    };
    
    character = {
      success_symbol = "[❯](bold blue)";
      error_symbol = "[❯](bold red)";
      vimcmd_symbol = "[❮](bold blue)";
    };
    
    # Disable unwanted modules
    username.disabled = true;
    hostname.disabled = true;
    git_branch.disabled = true;
    git_status.disabled = true;
    cmd_duration.disabled = true;
    package.disabled = true;
    python.disabled = true;
    nodejs.disabled = true;
    rust.disabled = true;
    java.disabled = true;
    golang.disabled = true;
    docker_context.disabled = true;
    kubernetes.disabled = true;
  };
};

  programs.zsh = {
   enable = true;  # Enable ZSH theming integration with Stylix
   # oh-my-zsh = {
   #   enable = true;
   #   theme = "cypher";
   # };
  };

    programs.ghostty.enable = lib.mkIf (config.triverhome.terminals.ghostty.enable or true) true;
    programs.kitty.enable = lib.mkIf (config.triverhome.terminals.kitty.enable or true) true;
  };
} 
