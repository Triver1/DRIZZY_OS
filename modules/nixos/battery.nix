{ config, lib, pkgs, ... }:

{
  # Enable TLP for laptop battery management
  services.tlp = {
    enable = true;
    settings = {
      # Battery charge thresholds to preserve battery health
      START_CHARGE_THRESH_BAT0 = 40;
      STOP_CHARGE_THRESH_BAT0 = 90;
  
      # CPU scaling governor
      CPU_SCALING_GOVERNOR_ON_AC = "performance";
      CPU_SCALING_GOVERNOR_ON_BAT = "powersave";

      # WiFi power saving
      WIFI_PWR_ON_AC = "off";
      WIFI_PWR_ON_BAT = "on";
    };
  };

  services.upower = {
      enable = true;
      # Optional: additional configuration
      percentageLow = 10;
      percentageCritical = 3;
      percentageAction = 2;
      criticalPowerAction = "Hibernate";
    };
  # Disable power-profiles-daemon (conflicts with TLP)
  services.power-profiles-daemon.enable = false;

  # Basic power management
  powerManagement.enable = true;
} 
