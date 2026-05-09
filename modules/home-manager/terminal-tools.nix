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
    home.packages = with pkgs; [
      stdenv.cc
      gemini-cli
      starship
      fzf
      rich-cli
      luarocks
      lua5_1
    ] ++ lib.optionals nvimEnable [
      neovim
    ];

    xdg.configFile = lib.mkIf nvimEnable {
      nvim.source = config.lib.file.mkOutOfStoreSymlink ../no-nix/nvim;
      noter-nvim.source = config.lib.file.mkOutOfStoreSymlink ../no-nix/noter-nvim;
    };

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
        set -g mouse on
        setw -g mode-keys vi

        bind-key h select-pane -L
        bind-key j select-pane -D
        bind-key k select-pane -U
        bind-key l select-pane -R

        set -g @tmux_power_theme 'gold'
        set -sg escape-time 0
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
      enable = true;
      enableCompletion = true;

      shellAliases = {
        nnvim = "NVIM_APPNAME=noter-nvim nvim";
      };

      initContent = ''
        # Show completion menu after pressing Tab
        zstyle ':completion:*' menu select

        # Better cd completion
        zstyle ':completion:*:*:cd:*' tag-order \
          local-directories directory-stack path-directories

        # Include hidden directories in path completion
        _comp_options+=(globdots)

        export LD_LIBRARY_PATH=${pkgs.libGL}/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}

        # Ctrl+R: fuzzy history search using fzf
        autoload -Uz history-incremental-search-backward
        zle -N history-incremental-search-backward

        __triver_fzf_history_widget() {
          if ! command -v fzf >/dev/null 2>&1; then
            zle history-incremental-search-backward
            return
          fi

          local selected
          selected=$(fc -rl 1 | awk '{ $1=""; sub(/^ /, ""); print }' | fzf --height 40% --reverse --query "$LBUFFER")

          if [[ -n "$selected" ]]; then
            LBUFFER="$selected"
          fi

          zle redisplay
        }

        zle -N __triver_fzf_history_widget
        bindkey -M emacs '^R' __triver_fzf_history_widget 2>/dev/null || true
        bindkey -M viins '^R' __triver_fzf_history_widget 2>/dev/null || true

        md2pdf() {
          if [ -z "$1" ]; then
            echo "Usage: md2pdf <input.md> [output.pdf]"
            return 1
          fi

          local input="$1"
          local output="''${2:-''${input%.md}.pdf}"

          pandoc "$input" -o file.html --embed-resources --standalone && wkhtmltopdf file.html "$output"
          echo "Created: $output"
        }
      '';
    };

    programs.ghostty.enable =
      lib.mkIf (config.triverhome.terminals.ghostty.enable or true) true;

    programs.kitty.enable =
      lib.mkIf (config.triverhome.terminals.kitty.enable or true) true;
  };
}
