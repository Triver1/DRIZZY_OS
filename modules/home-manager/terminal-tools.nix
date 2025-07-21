{ config, lib, pkgs, ... }:

{
  # Neovim configuration
  home.packages = with pkgs; [
    neovim
    stdenv.cc
    gemini-cli
  ];
  
  # Link your neovim config from outside the nix store
  # Points to your config in the workspace relative to home directory
  xdg.configFile.nvim.source = config.lib.file.mkOutOfStoreSymlink ../no-nix/nvim;

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

  programs.zsh = {
   enable = true;  # Enable ZSH theming integration with Stylix
   oh-my-zsh = {
     enable = true;
     theme = "cypher";
   };
  };

} 
