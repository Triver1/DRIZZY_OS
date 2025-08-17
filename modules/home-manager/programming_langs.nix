{ pkgs, ...}: {
  home.packages = with pkgs; [
    nodejs_24
    libgcc
    cargo        # for Rust projects
    rustc        # Rust compiler
    nodejs_20    # for Node.js bindings in algemaploom-rs
    python3      # for Python bindings in algemaploom-rs
    gcc          # C compiler for native dependencies
    pkg-config   # for finding libraries
    libffi       # for foreign function interface
    git          # version control
    pkgsCross.mingwW64.buildPackages.gcc  # for Windows cross-compilation
  ];

  programs.zsh.enable = true;
}
