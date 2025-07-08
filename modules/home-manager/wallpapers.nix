{ pkgs, ... }:
{
  services.wpaperd = {
   enable = true;
   settings = {
     DP-3 = {
       path = "/home/drizzy/NEWFLAKE/assets/wallpapers";
       sorting = "random";
       duration = "30m";
     };
   };
  };
}
