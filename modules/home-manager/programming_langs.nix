{ pkgs, ...}: {
  home.packages = with pkgs; [
    nodejs_24
    libgcc
    # IDlab
    openjdk17    # for rmlmapper-java and algemaploom-rs Java bindings
    openjdk21    # for algebraic-mapping-operators and algemaploom-java
    
    # Build Tools
    maven        # for all Java projects
    cargo        # for Rust projects
    rustc        # Rust compiler
    
    # Language Runtimes
    nodejs_20    # for Node.js bindings in algemaploom-rs
    python3      # for Python bindings in algemaploom-rs
    
    # System Development Tools
    gcc          # C compiler for native dependencies
    pkg-config   # for finding libraries
    libffi       # for foreign function interface
    git          # version control
    
    # Optional for cross-compilation
    pkgsCross.mingwW64.buildPackages.gcc  # for Windows cross-compilation
  ];

  programs.zsh.enable = true;
}
