{ config, pkgs, ... }:

{
  # ... other system configuration ...

  networking.firewall = {
    enable = true;
    allowedTCPPorts = [ 3000 ];  # Minecraft port
  };

  # ... rest of your system configuration ...
}
