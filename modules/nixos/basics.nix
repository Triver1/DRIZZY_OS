{ pkgs, ...}: {
environment.systemPackages = with pkgs; [
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
  ];
programs.zsh.enable = true;
users.defaultUserShell = pkgs.zsh;
}
