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
    pkgs.conda
    (pkgs.python313.withPackages (ps: with ps; [
      pyqt6
      jupyterlab
      pip
      pyside6
    ]))
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
    #pkgs.gearlever # Allows for running appimages (NOT RECOMMENDED)
    pkgs.libnotify
    pkgs.vlc
    pkgs.virt-manager
  ];

networking.networkmanager.enable = true;
programs.zsh.enable = true;
users.defaultUserShell = pkgs.zsh;

xdg.mime = {
  enable = true;

  defaultApplications = {
    # Browser
    "text/html" = "helium.desktop";
    "x-scheme-handler/http" = "helium.desktop";
    "x-scheme-handler/https" = "helium.desktop";
    "x-scheme-handler/about" = "helium.desktop";
    "x-scheme-handler/unknown" = "helium.desktop";

    # PDFs
    "application/pdf" = "helium.desktop";
    "application/x-pdf" = "helium.desktop";
  };
};

environment.sessionVariables = {
  BROWSER = "helium";
};

}
