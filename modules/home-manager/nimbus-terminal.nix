# nimbus-terminal.nix — an Apple-style terminal to sit under the nimbus shell.
#
# Home-manager module. Activated ONLY from modules/nixos/niri-nimbus.nix
# (home-manager.sharedModules), so going back to niri.nix restores the old
# terminal look. The base config (terminal-tools.nix, stylix) is untouched; this
# module wins via lib.mkOverride 40 (stronger than the lib.mkForce = 50 that
# terminal-tools.nix already uses on starship.settings, font-family and
# background-opacity).
#
# Look: "Frosted" — an inky #08080a tint at 58% over niri's blur,
# white text, glassy pastel accents, Geist Mono (the shell's own mono), and
# a quiet prompt: bold folder, soft branch, Finder-tag dots, a white chevron.
{ config, lib, pkgs, ... }:

let
  win = lib.mkOverride 40;

  # Frosted glass: colours are tuned to read on a blurred wallpaper.
  c = {
    window = "#08080a"; # ink tint (58% opaque)
    raised = "#34343a"; # selected rows (white ~10% over the tint)
    sep = "#3a3a40"; # hairlines
    label = "#ffffff";
    secondary = "#c2c2c6"; # white ~70%
    tertiary = "#8d8d92"; # white ~50%
    selection = "#45454c"; # white ~18%
    blue = "#9fd0ff";
    blue_hi = "#bfe0ff";
    red = "#ff8a8a";
    green = "#7dffb2";
    yellow = "#ffd27d";
    orange = "#ffd27d";
    purple = "#ffb1e0";
    pink = "#ffb1e0";
    cyan = "#8ff0f0";
  };

  bare = s: lib.removePrefix "#" s;
in
{
  fonts.fontconfig.enable = true;

  # ── Ghostty ──────────────────────────────────────────────────────────────
  programs.ghostty.themes.nimbus = {
    background = bare c.window;
    foreground = bare c.label;
    cursor-color = bare c.label;
    cursor-text = bare c.window;
    selection-background = bare c.selection;
    selection-foreground = bare c.label;
    palette = [
      "0=#2a2a30"   # black
      "1=#ff8a8a"   # red
      "2=#7dffb2"   # green
      "3=#ffd27d"   # yellow
      "4=#9fd0ff"   # blue
      "5=#ffb1e0"   # magenta
      "6=#8ff0f0"   # cyan
      "7=#e6e6ea"   # white
      "8=#8d8d92"   # bright black   comments, hints
      "9=#ffa8a8"   # bright red
      "10=#a5ffcb"  # bright green
      "11=#ffe0a3"  # bright yellow
      "12=#bfe0ff"  # bright blue
      "13=#ffc9e9"  # bright magenta
      "14=#b3f6f6"  # bright cyan
      "15=#ffffff"  # bright white
    ];
  };

  programs.ghostty.settings = {
    theme = win "nimbus";
    font-family = win [ "Geist Mono" "JetBrainsMono Nerd Font Mono" ];
    font-size = win 10.5;
    adjust-cell-height = win "12%";
    # Mostly glass: niri's blur (window-rule in niri-nimbus.nix) does the rest.
    background-opacity = win 0.58;
    window-padding-x = win 18;
    window-padding-y = win 14;
    window-padding-balance = win true;
    window-padding-color = win "extend";
    window-decoration = win false;
    cursor-style = win "bar";
    cursor-style-blink = win true;
    minimum-contrast = win 1.1;
    unfocused-split-opacity = win 0.9;
    unfocused-split-fill = win c.window;
    split-divider-color = win c.sep;
  };

  # ── Starship: quiet, one line of information and a chevron ───────────────
  #   NEWFLAKE  hyprland_scrolling ●                         2.3s
  #   ›
  programs.starship.settings = win {
    "$schema" = "https://starship.rs/config-schema.json";
    add_newline = true;
    palette = "nimbus";
    palettes.nimbus = {
      inherit (c) label secondary tertiary blue red green yellow orange purple;
    };

    format = lib.concatStrings [
      "$username$hostname"
      "$directory"
      "$git_branch$git_state$git_status"
      "$nix_shell"
      "$fill"
      "$cmd_duration"
      "$line_break"
      "$character"
    ];
    continuation_prompt = "[·](fg:tertiary) ";
    fill.symbol = " ";

    # Only shown over SSH / as root, like the macOS prompt's user@host.
    username = {
      format = "[$user]($style)[@](fg:tertiary)";
      style_user = "fg:secondary";
      style_root = "fg:red bold";
    };
    hostname = {
      format = "[$hostname]($style) ";
      style = "fg:secondary";
    };

    directory = {
      style = "fg:label bold";
      format = "[$path]($style)[$read_only]($read_only_style)";
      read_only = " ⌀";
      read_only_style = "fg:tertiary";
      truncation_length = 3;
      truncation_symbol = "";
      home_symbol = "~";
      truncate_to_repo = true;
    };

    git_branch = {
      format = "  [$branch]($style)";
      style = "fg:secondary";
    };
    git_status = {
      # Finder-tag dots: green staged, orange changed, grey untracked.
      format = "( [$staged](fg:green)[$modified$deleted$renamed](fg:orange)[$untracked](fg:tertiary))( [$conflicted](fg:red))( [$ahead_behind](fg:secondary))";
      staged = "●";
      modified = "●";
      deleted = "●";
      renamed = "●";
      untracked = "●";
      conflicted = "!";
      ahead = "↑$count";
      behind = "↓$count";
      diverged = "↕";
    };
    git_state = {
      format = " [$state( $progress_current/$progress_total)](fg:orange)";
    };

    nix_shell = {
      format = "  [nix]($style)";
      style = "fg:tertiary";
    };

    cmd_duration = {
      min_time = 2000;
      format = "[$duration](fg:tertiary)";
    };

    line_break.disabled = false;

    character = {
      success_symbol = "[›](bold fg:label)";
      error_symbol = "[›](bold fg:red)";
      vimcmd_symbol = "[‹](bold fg:green)";
      vimcmd_replace_one_symbol = "[‹](bold fg:purple)";
      vimcmd_replace_symbol = "[‹](bold fg:purple)";
      vimcmd_visual_symbol = "[‹](bold fg:yellow)";
    };

    # Language versions stay out of the prompt (Apple-quiet).
    python.disabled = true;
    rust.disabled = true;
    golang.disabled = true;
    nodejs.disabled = true;
    c.disabled = true;
    java.disabled = true;
    haskell.disabled = true;
    time.disabled = true;
  };

  # ── zsh: fzf as a frosted list (+ ready-made highlight colours) ──────
  programs.zsh.initContent = lib.mkOrder 1600 ''
    export FZF_DEFAULT_OPTS="--color=bg:-1,bg+:${c.raised},fg:${c.secondary},fg+:${c.label},hl:${c.blue_hi},hl+:${c.blue_hi},info:${c.tertiary},prompt:${c.label},pointer:${c.label},marker:${c.green},spinner:${c.tertiary},header:${c.tertiary},border:${c.sep},gutter:-1,query:${c.label},separator:${c.sep},scrollbar:${c.sep} --border=rounded --prompt='› ' --pointer=' ' --marker='•' --info=inline-right --no-separator"
  '';
  # Syntax highlighting / autosuggestions are NOT enabled by the base config;
  # these colours only apply if you turn them on later.
  programs.zsh.syntaxHighlighting.styles = {
    command = "fg=${c.label}";
    builtin = "fg=${c.label}";
    alias = "fg=${c.label}";
    function = "fg=${c.label}";
    precommand = "fg=${c.purple}";
    unknown-token = "fg=${c.red}";
    reserved-word = "fg=${c.pink}";
    path = "fg=${c.label},underline";
    single-quoted-argument = "fg=${c.orange}";
    double-quoted-argument = "fg=${c.orange}";
    dollar-quoted-argument = "fg=${c.orange}";
    globbing = "fg=${c.cyan}";
    commandseparator = "fg=${c.tertiary}";
    redirection = "fg=${c.tertiary}";
    comment = "fg=${c.tertiary}";
    arg0 = "fg=${c.label}";
  };
  programs.zsh.autosuggestion.highlight = "fg=${c.tertiary}";

  # ── Yazi: drop stylix's ponokai theme and follow the terminal palette ────
  stylix.targets.yazi.enable = lib.mkForce false;
  programs.yazi.theme = {
    mgr = {
      cwd = { fg = c.secondary; };
      hovered = { bg = c.raised; fg = c.label; };
      preview_hovered = { underline = true; };
      border_symbol = "│";
      border_style = { fg = c.sep; };
    };
  };

  # ── tmux: tmux-power in frosted blue instead of stylix base16 on top ─────
  # (the existing `@tmux_power_theme 'gold'` is set *after* the plugin runs,
  # so it never applied; set the colour and re-run tmux-power last.)
  stylix.targets.tmux.enable = lib.mkForce false;
  programs.tmux.extraConfig = lib.mkAfter ''
    set -g @tmux_power_theme '${c.blue}'
    run-shell ${pkgs.tmuxPlugins.power-theme.rtp}
  '';
}
