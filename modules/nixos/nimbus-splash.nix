# nimbus-splash.nix: First Light, the boot splash, as a Plymouth plugin.
#
#   hosts/laptop/configuration.nix:  (folders.m + "/nimbus-splash.nix")
#
# Dawn spreads along the edge of a dark planet and holds there, the limb breathing,
# for as long as the boot takes. Just before the display manager starts, the planet
# pulls back into the login screen's avatar ring. See ../no-nix/first-light.
#
# How the hand-over works, so that the ring never leaves the screen and no console shows:
#
#   nimbus-splash-land    asks for the landing and waits for it to finish.
#   greetd                patched to leave the VT alone during the hand-over: going to
#                         text mode on the foreground VT makes the kernel console take
#                         the panel, even from a DRM master (a special case for Xorg
#                         in drm_fb_helper_set_par). Holding the device can't stop that.
#   greetd's PAM session  just before the session starts: `plymouth deactivate`.
#                         plymouthd lets go of the device but keeps running, which is
#                         what keeps its last frame on screen.
#   niri                  takes the device. It's patched to present nothing until the
#                         lock screen has a frame, so the first thing to replace the
#                         ring is the greeter's own ring, pixel for pixel.
#   nimbus-splash-reap    only now ends plymouthd.
#
# Those last two are why this isn't `plymouth quit --retain-splash` before the display
# manager: plymouthd's framebuffer dies with it, so the panel would go dark until
# something else drew. With GDM, GDM does the deactivate and the quit itself.
#
# Roll out with `nixos-rebuild boot` and a reboot, never `switch`.
# Recovery: an older generation in GRUB, or `plymouth.enable=0` on the kernel line.
# Measurements from the real panel, after a boot: cat /run/first-light.log
{
  config,
  lib,
  options,
  pkgs,
  ...
}:

let
  cfg = config.nimbus.splash;
  plymouth = config.boot.plymouth.package;
  plymouthBin = lib.getExe' plymouth "plymouth";

  keepVt = "/run/first-light.keep-vt";

  theme = pkgs.callPackage ../no-nix/first-light/package.nix {
    inherit plymouth;
    inherit (cfg) landSeconds;
  };

  # amdgpu asks for the firmware of every GPU it supports, 34.8 MiB of initrd per
  # generation on a 1 GB /boot. Only this GPU's goes in; the real root keeps it all.
  initrdFirmware = pkgs.runCommand "initrd-firmware" { } ''
    src=${config.hardware.firmware}/lib/firmware
    mkdir -p $out/lib/firmware/amdgpu
    for entry in "$src"/*; do
      [ "''${entry##*/}" = amdgpu ] || ln -s "$(readlink -f "$entry")" "$out/lib/firmware/''${entry##*/}"
    done
    for f in "$src"/amdgpu/*; do
      case "''${f##*/}" in
        ${lib.concatStringsSep "|" (map (p: "${p}*") cfg.amdgpuFirmware)})
          ln -s "$(readlink -f "$f")" "$out/lib/firmware/amdgpu/''${f##*/}"
          ;;
      esac
    done
    for p in ${lib.escapeShellArgs cfg.amdgpuFirmware}; do
      ls $out/lib/firmware/amdgpu/"$p"* > /dev/null \
        || { echo "no firmware matches $p: is this still the right list?" >&2; exit 1; }
    done
  '';

  # A firmware file this list missed would fail amdgpu in the initrd, where nothing
  # else can supply it. The real root has them all, so try again from there.
  gpuRescue = pkgs.writeShellScript "nimbus-splash-gpu-rescue" ''
    for dev in /sys/bus/pci/devices/*; do
      [ "$(cat "$dev/vendor")" = 0x1002 ] || continue
      case "$(cat "$dev/class")" in 0x03*) ;; *) continue ;; esac
      [ -e "$dev/driver" ] && continue
      echo "''${dev##*/} has no driver, binding amdgpu from the real root"
      echo "''${dev##*/}" > /sys/bus/pci/drivers/amdgpu/bind || true
    done
  '';

  land = pkgs.writeShellScript "nimbus-splash-land" ''
    export PATH=${
      lib.makeBinPath [
        pkgs.coreutils
        pkgs.procps
      ]
    }:$PATH
    ${plymouthBin} --ping || exit 0
    # For the patched greetd: see the header. nimbus-splash-reap takes it away.
    ${lib.optionalString config.services.greetd.enable "touch ${keepVt}"}
    ${plymouthBin} update --status=land || exit 0
    # The plugin leaves this when the last frame is up.
    tick=0
    while [ "$tick" -lt ${toString ((builtins.ceil cfg.landSeconds + 6) * 20)} ]; do
      [ -e /run/first-light.landed ] && exit 0
      sleep 0.05
      tick=$((tick + 1))
    done
    # The display manager still has to get the display, so plymouthd has to go.
    echo "plymouth didn't land in time, ending it"
    timeout 2 ${plymouthBin} quit || pkill -KILL -x plymouthd
    exit 0
  '';

  # Runs as root from greetd's PAM session: after greetd has set up the VT, before
  # anything of the session exists. Also for every later login, where there's no
  # plymouthd to talk to.
  release = pkgs.writeShellScript "nimbus-splash-release" ''
    [ "$PAM_TYPE" = open_session ] || exit 0
    ${plymouthBin} --ping 2> /dev/null || exit 0
    ${pkgs.coreutils}/bin/timeout 3 ${plymouthBin} deactivate
    exit 0
  '';

  # greetd only. Ends plymouthd once the compositor has the display, or once it's clear
  # that one won't, in which case the VT goes back to text for the text greeter.
  reap = pkgs.writeShellScript "nimbus-splash-reap" ''
    export PATH=${
      lib.makeBinPath [
        pkgs.coreutils
        pkgs.procps
        config.systemd.package
      ]
    }:$PATH
    trap 'rm -f ${keepVt}' EXIT
    ${plymouthBin} --ping || exit 0
    tick=0
    while [ "$tick" -lt ${toString (cfg.handoverTimeout * 10)} ]; do
      if [ -e ${cfg.greeterReadyFile} ]; then
        # With software rendering nimbus says ready as it commits, a moment before
        # niri has presented, and plymouthd's frame mustn't leave first.
        sleep 0.2
        ${plymouthBin} quit --retain-splash
        exit 0
      fi
      systemctl -q is-active display-manager.service || break
      pgrep -x 'tuigreet|agreety' > /dev/null && break
      sleep 0.1
      tick=$((tick + 1))
    done
    # Not the plain quit, even for a text greeter: that writes the whole boot log onto
    # the VT, over the greeter, which has no reason to draw itself again.
    ${plymouthBin} quit --retain-splash
    pgrep -x niri > /dev/null || ${theme}/bin/first-light-vt-mode text
  '';
in
{
  options.nimbus.splash = {
    landSeconds = lib.mkOption {
      type = lib.types.float;
      default = 3.2;
      description = ''
        How long the pull-back takes. The approved film took 6.4 s for this stretch;
        all of its timing scales with this.
      '';
    };
    landAfter = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "systemd-user-sessions.service" ];
      description = ''
        What the pull-back waits for. Those are also what the display manager waits
        for, so by default the landing is added to the time to the login screen.
        [ "network.target" ] overlaps it with the end of the boot instead, about
        2.7 s sooner here, at the price of the ring sitting still for as long as the
        rest of the boot takes.
      '';
    };
    holdFirstFrame = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Run the greeter's niri with ../no-nix/first-light/niri-hold-first-frame.patch.
        Without it niri draws black until nimbus is up, and the ring blinks off for that
        long. Turn it off if a niri update stops the patch from applying.
      '';
    };
    amdgpuFirmware = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      # Radeon 780M, from /sys/class/drm/card1/device/ip_discovery: GC 11.0.1,
      # SDMA 6.0.1, VCN 4.0.2, DCN 3.1.4, PSP 13.0.4.
      default = [
        "dcn_3_1_4_"
        "gc_11_0_1_"
        "psp_13_0_4_"
        "sdma_6_0_1"
        "vcn_4_0_2"
      ];
      description = "File name prefixes under amdgpu/ that go into the initrd.";
    };
    greeterReadyFile = lib.mkOption {
      type = lib.types.str;
      default = "/run/nimbus-greeter/ready";
      description = "Appears when the greeter has its first frame on screen.";
    };
    handoverTimeout = lib.mkOption {
      type = lib.types.ints.positive;
      default = 30;
      description = "Seconds plymouthd waits for the greeter before it just quits.";
    };
  };

  config = lib.mkMerge [
    # Stylix themes Plymouth too, and would otherwise claim boot.plymouth.theme.
    (lib.optionalAttrs (options ? stylix) { stylix.targets.plymouth.enable = false; })
    # Works without the greeter module too: then there's no greeter's niri to patch.
    (lib.optionalAttrs (options ? nimbus && options.nimbus ? greeter) {
      nimbus.greeter.niriPackage = lib.mkIf cfg.holdFirstFrame (
        lib.mkDefault (pkgs.callPackage ../no-nix/first-light/niri-hold.nix { })
      );
    })
    {
      boot.plymouth = {
        enable = true;
        theme = "first-light";
        themePackages = [ theme ];
        # Without this, 3200x2000 counts as HiDPI and Plymouth talks in halved units.
        extraConfig = "DeviceScale=1";
      };

      # The plymouth module only links its own plugins into the real root, so a plymouthd
      # started there for shutdown wouldn't find this one.
      environment.etc."plymouth/plugins".source = lib.mkForce (
        pkgs.symlinkJoin {
          name = "plymouth-plugins";
          paths = [
            "${plymouth}/lib/plymouth"
            "${theme}/lib/plymouth"
          ];
        }
      );

      boot.consoleLogLevel = 3;
      boot.initrd.verbose = false;
      boot.kernelParams = [
        "quiet"
        "systemd.show_status=false"
        "rd.systemd.show_status=false"
        "rd.udev.log_level=3"
        "udev.log_priority=3"
        "vt.global_cursor_default=0"
      ];

      # Native resolution from the first frame.
      boot.initrd.kernelModules = [ "amdgpu" ];
      system.build.modulesClosure = lib.mkForce (
        pkgs.makeModulesClosure {
          rootModules = config.boot.initrd.availableKernelModules ++ config.boot.initrd.kernelModules;
          kernel = config.system.modulesTree;
          firmware = initrdFirmware;
          allowMissing = config.boot.initrd.allowMissingModules;
          inherit (config.boot.initrd) extraFirmwarePaths;
        }
      );

      assertions = [
        {
          # The firmware filter above only reaches the systemd initrd's closure.
          assertion = config.boot.initrd.systemd.enable;
          message = "nimbus-splash needs boot.initrd.systemd.enable, the default.";
        }
      ];

      systemd.services.nimbus-splash-gpu-rescue = {
        description = "Bind amdgpu if the initrd couldn't";
        wantedBy = [ "sysinit.target" ];
        after = [ "systemd-modules-load.service" ];
        before = [
          "sysinit.target"
          "nimbus-splash-land.service"
        ];
        unitConfig.DefaultDependencies = false;
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "-${gpuRescue}";
        };
      };

      systemd.services.nimbus-splash-land = {
        description = "Land the boot splash on the login ring";
        # Not graphical.target: a boot without a display manager has to land too.
        wantedBy = [ "multi-user.target" ];
        after = [ "plymouth-start.service" ] ++ cfg.landAfter;
        before = [
          "display-manager.service"
          "greetd.service"
        ];
        restartIfChanged = false;
        # Nothing that goes wrong here may keep the display manager from starting.
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "-${land}";
          TimeoutStartSec = builtins.ceil cfg.landSeconds + 12;
        };
      };

      services.greetd.package = lib.mkDefault (
        pkgs.greetd.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ../no-nix/first-light/greetd-keep-vt.patch ];
        })
      );

      security.pam.services.greetd = lib.mkIf config.services.greetd.enable {
        rules.session.nimbus-splash = {
          control = "optional";
          modulePath = "${config.security.pam.package}/lib/security/pam_exec.so";
          args = [
            "quiet"
            "${release}"
          ];
          order = config.security.pam.services.greetd.rules.session.login.order + 10;
        };
      };

      # greetd is Type=idle: it would sit on the finished frame for up to 5 s while the
      # rest of the boot's jobs run down.
      systemd.services.greetd = lib.mkIf config.services.greetd.enable {
        serviceConfig.Type = lib.mkForce "simple";
      };

      # With greetd nothing else ends plymouthd. plymouth-quit-wait holds back
      # multi-user.target until this has run, so it must not wait on that target.
      services.greetd.greeterManagesPlymouth = lib.mkDefault true;
      systemd.services.plymouth-quit = lib.mkIf config.services.greetd.enable {
        wantedBy = lib.mkForce [ ];
      };
      systemd.services.nimbus-splash-reap = lib.mkIf config.services.greetd.enable {
        description = "End plymouthd once the greeter has the display";
        wantedBy = [ "multi-user.target" ];
        after = [
          "nimbus-splash-land.service"
          "greetd.service"
        ];
        restartIfChanged = false;
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "-${reap}";
          TimeoutStartSec = cfg.handoverTimeout + 10;
        };
      };
    }
  ];
}
