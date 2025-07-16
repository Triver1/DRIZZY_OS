{ pkgs, ... }:
{
  services.wpaperd = {
   enable = true;
   settings = {
     DP-3 = {
       path = "/home/drizzy/NEWFLAKE/assets/wallpapers/wqhd/";
       sorting = "random";
       duration = "30m";
     };
     eDP-1 = {
       path = "/home/drizzy/NEWFLAKE/assets/wallpapers/laptop/";
       sorting = "random";
       duration = "30m";
     };
   };
  };
}
