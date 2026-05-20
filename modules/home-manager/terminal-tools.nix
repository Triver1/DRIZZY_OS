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
      nvim.source = config.lib.file.mkOutOfStoreSymlink "${config.home.homeDirectory}/NEWFLAKE/modules/no-nix/nvim";
      noter-nvim.source = config.lib.file.mkOutOfStoreSymlink "${config.home.homeDirectory}/NEWFLAKE/modules/no-nix/noter-nvim";
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

      settings = lib.mkForce (builtins.fromTOML ''
        "$schema" = 'https://starship.rs/config-schema.json'

        format = """
        [](color_orange)\
        $os\
        $username\
        [](bg:color_yellow fg:color_orange)\
        $directory\
        [](fg:color_yellow bg:color_aqua)\
        $git_branch\
        $git_status\
        [](fg:color_aqua bg:color_blue)\
        $c\
        $cpp\
        $rust\
        $golang\
        $nodejs\
        $bun\
        $php\
        $java\
        $kotlin\
        $haskell\
        $python\
        [](fg:color_blue bg:color_bg3)\
        $docker_context\
        $conda\
        $pixi\
        [](fg:color_bg3 bg:color_bg1)\
        $time\
        [ ](fg:color_bg1)\
        $line_break$character"""

        palette = 'gruvbox_dark'

        [palettes.gruvbox_dark]
        color_fg0 = '#fbf1c7'
        color_bg1 = '#3c3836'
        color_bg3 = '#665c54'
        color_blue = '#458588'
        color_aqua = '#689d6a'
        color_green = '#98971a'
        color_orange = '#d65d0e'
        color_purple = '#b16286'
        color_red = '#cc241d'
        color_yellow = '#d79921'

        [os]
        disabled = false
        style = "bg:color_orange fg:color_fg0"

        [os.symbols]
        Windows = "󰍲"
        Ubuntu = "󰕈"
        SUSE = ""
        Raspbian = "󰐿"
        Mint = "󰣭"
        Macos = "󰀵"
        Manjaro = ""
        Linux = "󰌽"
        Gentoo = "󰣨"
        Fedora = "󰣛"
        Alpine = ""
        Amazon = ""
        Android = ""
        AOSC = ""
        Arch = "󰣇"
        Artix = "󰣇"
        EndeavourOS = ""
        CentOS = ""
        Debian = "󰣚"
        Redhat = "󱄛"
        RedHatEnterprise = "󱄛"
        Pop = ""

        [username]
        show_always = true
        style_user = "bg:color_orange fg:color_fg0"
        style_root = "bg:color_orange fg:color_fg0"
        format = '[ $user ]($style)'

        [directory]
        style = "fg:color_fg0 bg:color_yellow"
        format = "[ $path ]($style)"
        truncation_length = 3
        truncation_symbol = "…/"

        [directory.substitutions]
        "Documents" = "󰈙 "
        "Downloads" = " "
        "Music" = "󰝚 "
        "Pictures" = " "
        "Developer" = "󰲋 "

        [git_branch]
        symbol = ""
        style = "bg:color_aqua"
        format = '[[ $symbol $branch ](fg:color_fg0 bg:color_aqua)]($style)'

        [git_status]
        style = "bg:color_aqua"
        format = '[[($all_status$ahead_behind )](fg:color_fg0 bg:color_aqua)]($style)'

        [nodejs]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [bun]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [c]
        symbol = " "
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [cpp]
        symbol = " "
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [rust]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [golang]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [php]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [java]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [kotlin]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [haskell]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [python]
        symbol = ""
        style = "bg:color_blue"
        format = '[[ $symbol( $version) ](fg:color_fg0 bg:color_blue)]($style)'

        [docker_context]
        symbol = ""
        style = "bg:color_bg3"
        format = '[[ $symbol( $context) ](fg:#83a598 bg:color_bg3)]($style)'

        [conda]
        style = "bg:color_bg3"
        format = '[[ $symbol( $environment) ](fg:#83a598 bg:color_bg3)]($style)'

        [pixi]
        style = "bg:color_bg3"
        format = '[[ $symbol( $version)( $environment) ](fg:color_fg0 bg:color_bg3)]($style)'

        [time]
        disabled = false
        time_format = "%R"
        style = "bg:color_bg1"
        format = '[[  $time ](fg:color_fg0 bg:color_bg1)]($style)'

        [line_break]
        disabled = false

        [character]
        disabled = false
        success_symbol = '[](bold fg:color_green)'
        error_symbol = '[](bold fg:color_red)'
        vimcmd_symbol = '[](bold fg:color_green)'
        vimcmd_replace_one_symbol = '[](bold fg:color_purple)'
        vimcmd_replace_symbol = '[](bold fg:color_purple)'
        vimcmd_visual_symbol = '[](bold fg:color_yellow)'
      '');
    };

    programs.zsh = {
      enable = true;
      enableCompletion = true;

      shellAliases = {
        nnvim = "NVIM_APPNAME=noter-nvim nvim";
        codex-main = "CODEX_HOME=\"$HOME/.codex-main\" codex";
        codex-alt  = "CODEX_HOME=\"$HOME/.codex-alt\" codex";
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

    home.activation.ensureWritableCodexHomes = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      for dir in "$HOME/.codex-main" "$HOME/.codex-alt"; do
        $DRY_RUN_CMD mkdir -p "$dir"

        config_file="$dir/config.toml"
        if [ -L "$config_file" ]; then
          $DRY_RUN_CMD rm "$config_file"
        fi

        if [ ! -e "$config_file" ]; then
          $DRY_RUN_CMD ${pkgs.bash}/bin/bash -c 'printf "%s\n" "cli_auth_credentials_store = \"file\"" > "$1"' _ "$config_file"
          $DRY_RUN_CMD chmod 600 "$config_file"
        fi
      done
    '';

    programs.ghostty = lib.mkIf (config.triverhome.terminals.ghostty.enable or true) {
      enable = true;
      settings = {
        gtk-single-instance = false;
        window-padding-x = 5;
        window-padding-y = 5;
        window-padding-balance = true;
        window-decoration = false;
        gtk-titlebar = false;
        resize-overlay = "never";
        confirm-close-surface = false;
        clipboard-read = "allow";
        clipboard-write = "allow";
        copy-on-select = true;
        background-opacity = lib.mkForce 0.88;
        font-family = lib.mkForce "MonaspiceNe Nerd Font Mono";
        unfocused-split-opacity = 1;
        unfocused-split-fill = "#282828";
        mouse-hide-while-typing = true;
        shell-integration = "zsh";
        window-subtitle = false;
      };
    };

    programs.kitty.enable =
      lib.mkIf (config.triverhome.terminals.kitty.enable or true) true;
  };
}
