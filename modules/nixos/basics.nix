{ pkgs, ...}: {
environment.systemPackages = with pkgs; [
    google-chrome
    pulseaudio
    git
    kitty
    vim
    ripgrep
    alacritty
    lazygit
    python314
    conda
    wget
    wl-clipboard
    yazi
    nautilus
    onlyoffice-desktopeditors
    htop
    unzip
    xdg-utils
    fd
    vscode-fhs
    libsecret
    gearlever # Allows for running appimages (NOT RECOMMENDED)
  ];
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
