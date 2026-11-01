# The greeter's niri: holds its first frame until the lock screen has one to show, so the
# splash's last frame is replaced directly by the greeter's first, with no black between.
# Only the greeter runs this; the desktop session keeps plain niri.
#
# If a niri update breaks the patch, set nimbus.splash.holdFirstFrame = false. The ring
# then blinks off for as long as nimbus takes to start, and nothing else changes.
{ niri }:

niri.overrideAttrs (old: {
  pname = "niri-hold-first-frame";
  patches = (old.patches or [ ]) ++ [ ./niri-hold-first-frame.patch ];
  doCheck = false;
})
