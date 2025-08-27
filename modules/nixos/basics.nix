{ pkgs, inputs, options, ...}: {
environment.systemPackages =  [
    # pkgs.google-chrome
    pkgs.pulseaudio
    pkgs.git
    pkgs.kitty
    pkgs.vim
    pkgs.ripgrep
    pkgs.alacritty
    pkgs.lazygit
    pkgs.python314
    pkgs.conda
    pkgs.wget
    pkgs.wl-clipboard
    pkgs.yazi
    pkgs.nautilus
    pkgs.onlyoffice-desktopeditors
    pkgs.htop
    pkgs.unzip
    pkgs.xdg-utils
    pkgs.fd
    pkgs.vscode-fhs
    pkgs.libsecret
    pkgs.gst_all_1.gstreamer
    pkgs.gearlever # Allows for running appimages (NOT RECOMMENDED)
    pkgs.libnotify
    pkgs.vlc
  ];
networking.networkmanager.enable = true;
programs.zsh.enable = true;
users.defaultUserShell = pkgs.zsh;
  xdg.mime.defaultApplications = {
    "text/html" = "firefox.desktop";
    "x-scheme-handler/http" = "firefox.desktop";
    "x-scheme-handler/https" = "firefox.desktop";
    "x-scheme-handler/about" = "firefox.desktop";
    "x-scheme-handler/unknown" = "firefox.desktop";
  }; 
  environment.sessionVariables = {
    BROWSER = "firefox";
  };

}
