{ config, lib, pkgs, ... }:

{
  # Enable TLP for laptop battery management
  services.tlp = {
    enable = true;
    settings = {
      # Battery charge thresholds to preserve battery health
      START_CHARGE_THRESH_BAT1 = 40;
      STOP_CHARGE_THRESH_BAT1 = 95;
  
      # Keep the CPU responsive enough for smooth compositor animations.
      CPU_SCALING_GOVERNOR_ON_AC = "schedutil";
      CPU_SCALING_GOVERNOR_ON_BAT = "schedutil";
      CPU_ENERGY_PERF_POLICY_ON_AC = "balance_performance";
      CPU_ENERGY_PERF_POLICY_ON_BAT = "balance_power";
      CPU_BOOST_ON_AC = 1;
      CPU_BOOST_ON_BAT = 0;

    };
  };

  services.upower = {
      enable = true;
      # Optional: additional configuration
      percentageLow = 10;
      percentageCritical = 5;
      percentageAction = 2;
      criticalPowerAction = "Hibernate";
    };
  # Disable power-profiles-daemon (conflicts with TLP)
  services.power-profiles-daemon.enable = false;

  # Basic power management
  powerManagement.enable = true;
} 
