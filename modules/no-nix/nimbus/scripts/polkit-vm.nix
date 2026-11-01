# The throw-away VM for src/polkit/vm.rs: real polkit, real pkexec, two test
# users. Built with the flake's own nixpkgs, so the same polkit as the host.
{ harness }:
let
  flake = builtins.getFlake "/home/drizzy/NEWFLAKE";
  pkgs = flake.nixosConfigurations.laptop.pkgs;
in
pkgs.testers.runNixOSTest {
  name = "nimbus-polkit";
  nodes.machine = { pkgs, ... }: {
    virtualisation.memorySize = 1536;
    virtualisation.cores = 2;
    security.polkit.enable = true;
    security.polkit.enablePkexecWrapper = true;
    environment.etc."nimbus-polkit-vm".text = "throw-away test VM\n";
    users.users.alice = { isNormalUser = true; extraGroups = [ "wheel" ]; password = "alice-vm-password"; };
    users.users.bob = { isNormalUser = true; extraGroups = [ "wheel" ]; password = "bob-vm-password"; };
    services.getty.autologinUser = "alice";
    environment.systemPackages = [ pkgs.polkit ];
  };
  testScript = ''
    harness = "${harness}"
    run = f"{harness} --ignored --exact --nocapture --test-threads=1 polkit::vm::e2e"

    def stage(name, expect_rc=0):
        machine.succeed(f"rm -f /tmp/{name}.rc /tmp/{name}.log")
        machine.send_chars(f"NIMBUS_POLKIT_VM={name} {run} > /tmp/{name}.log 2>&1; echo $? > /tmp/{name}.rc\n")
        machine.wait_for_file(f"/tmp/{name}.rc", timeout=180)
        out = machine.succeed(f"cat /tmp/{name}.log")
        print(f"--- {name} ---\n{out}\n---")
        rc = int(machine.succeed(f"cat /tmp/{name}.rc").strip())
        assert rc == expect_rc, f"{name}: exit {rc}, wanted {expect_rc}"
        return out

    machine.wait_for_unit("multi-user.target")
    machine.wait_until_tty_matches("1", "alice@")
    print(machine.succeed("ls -ld /run/polkit /run/polkit/agent-helper.socket; ls /run/wrappers/bin; polkitd --version 2>&1 || true"))
    print(machine.succeed("loginctl list-sessions --no-legend"))

    # Refused anywhere but in this VM.
    main_out = stage("main")
    assert "PASS correct password" in main_out
    assert "PASS wrong then correct" in main_out
    assert "PASS dismissed" in main_out
    assert "PASS CancelAuthentication mid-prompt" in main_out
    assert "PASS identity picker" in main_out
    assert "PASS two requests queue" in main_out
    assert "PASS impostor refused" in main_out
    # Nothing identifying in what the agent logs.
    for secret in ("alice-vm-password", "bob-vm-password"):
        assert secret not in main_out, "password in a log"

    # The process dying with a prompt open fails the app, promptly.
    stage("abort", expect_rc=134)
    machine.wait_for_file("/tmp/pk-abort.rc", timeout=20)
    rc = machine.succeed("cat /tmp/pk-abort.rc").strip()
    print(f"pkexec after agent abort: exit {rc}: " + machine.succeed("cat /tmp/pk-abort.out"))
    assert rc != "0"
    assert "uid=0" not in machine.succeed("cat /tmp/pk-abort.out")
    # No helper instance is left behind.
    machine.wait_until_succeeds("! systemctl list-units --no-legend 'polkit-agent-helper@*' | grep -q running", timeout=20)
    # And a restart registers at once.
    stage("correct")

    # SIGTERM: the app fails, the process dies of the signal (143 in a
    # shell, which run.sh reads as "stay down"), and it can register again.
    stage("sigterm", expect_rc=143)
    machine.wait_for_file("/tmp/pk-term.rc", timeout=20)
    assert machine.succeed("cat /tmp/pk-term.rc").strip() != "0"
    stage("correct")

    # The helper down: pkexec fails, fast, and nothing hangs.
    machine.succeed("systemctl stop polkit-agent-helper.socket")
    stage("helper-down")
    machine.succeed("systemctl start polkit-agent-helper.socket")
    stage("correct")

    # polkitd restarting under a live agent, as nixos-rebuild does.
    machine.send_chars(f"NIMBUS_POLKIT_VM=restart {run} > /tmp/restart.log 2>&1; echo $? > /tmp/restart.rc\n")
    machine.wait_for_file("/tmp/ready-for-restart", timeout=60)
    machine.succeed("systemctl restart polkit.service")
    machine.succeed("touch /tmp/restarted")
    machine.wait_for_file("/tmp/restart.rc", timeout=120)
    restart_out = machine.succeed("cat /tmp/restart.log")
    print(f"--- restart ---\n{restart_out}\n---")
    assert machine.succeed("cat /tmp/restart.rc").strip() == "0"
    assert "PASS survives a polkitd restart" in restart_out

    # The passwords reached nothing persistent.
    journal = machine.succeed("journalctl -b --no-pager")
    for secret in ("alice-vm-password", "bob-vm-password"):
        assert secret not in journal, "password in the journal"
    print("ALL STAGES PASSED")
  '';
}
