{ config, pkgs, ... }:

{
  # ... other system configuration ...

  networking.firewall = {
    enable = true;
    allowedTCPPorts = [ 41269 ];  # Minecraft port
  };

  # ... rest of your system configuration ...
}
