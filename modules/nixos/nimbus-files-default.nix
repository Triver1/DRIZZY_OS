# Make nimbus Files the system file manager. Add to the NixOS module that
# already sets `xdg.mime` (modules/nixos/basics.nix) or to niri-nimbus.nix.
#
#   xdg-open <folder>, "open folder"   -> nimbus-files.desktop -> this wrapper
#   "show in folder" (browsers, Electron) -> org.freedesktop.FileManager1,
#                                            which nimbus owns while it runs
#
# nimbus not running: `nimbus msg` exits 1 and the wrapper opens Nautilus;
# for D-Bus, the bus activates Nautilus's own FileManager1 service. So keep
# pkgs.nautilus installed (basics.nix has it): the launcher hides it.
{ pkgs, ... }:
let
  # What run.sh and the niri binds use. Once nimbus is packaged, point this
  # at "${pkgs.nimbus}/bin/nimbus" (or wherever package.nix lands).
  nimbus = "/home/drizzy/NEWFLAKE/modules/no-nix/nimbus/target/release/nimbus";

  # Takes a path or a file:// URI (a folder opens, a file is revealed).
  # Only the first argument is shown: nimbus has one Files window.
  nimbusFilesOpen = pkgs.writeShellScript "nimbus-files-open" ''
    if [ "$#" -eq 0 ]; then
      ${nimbus} msg files 2>/dev/null && exit 0
    else
      ${nimbus} msg files "$1" 2>/dev/null && exit 0
    fi
    exec ${pkgs.nautilus}/bin/nautilus --new-window "$@"
  '';

  nimbusFilesDesktop = pkgs.makeDesktopItem {
    name = "nimbus-files";
    desktopName = "Files";
    comment = "Browse your files with nimbus";
    exec = "${nimbusFilesOpen} %U";
    icon = "system-file-manager";
    mimeTypes = [ "inode/directory" ];
    # The launcher shows nimbus's built-in Files; this one is only a handler.
    noDisplay = true;
  };
in
{
  environment.systemPackages = [ nimbusFilesDesktop ];

  xdg.mime.defaultApplications."inode/directory" = "nimbus-files.desktop";
}
