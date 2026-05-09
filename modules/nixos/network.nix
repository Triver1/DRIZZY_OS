{ config, pkgs, ... }:

{
  # ... other system configuration ...

  networking.firewall = {
    enable = false;
    allowedTCPPorts = [ 3000 50000 22 ];  # Minecraft port
allowedUDPPorts = [ 4000 ];
  };

  # ... rest of your system configuration ...
}
