# The First Light boot splash: a Plymouth plugin and its theme file. The picture is
# computed at boot, so there are no images to build or ship.
{
  lib,
  stdenv,
  plymouth,
  pkg-config,
  libevdev,
  libxkbcommon,
  udev,
  xkeyboard_config,
  landSeconds ? 3.2,
}:

stdenv.mkDerivation {
  pname = "first-light-splash";
  version = "1";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./fl.h
      ./fl_int.h
      ./fl_core.c
      ./fl_rows.c
      ./plugin.c
      ./still.c
      ./vt-mode.c
      ./first-light.plymouth
    ];
  };

  nativeBuildInputs = [ pkg-config ];
  # Only for what plymouth's .pc files name; nothing here links them.
  buildInputs = [
    plymouth
    libevdev
    libxkbcommon
    udev
    xkeyboard_config
  ];

  # fl_rows.c is built twice from one source: x86-64-v3 and baseline, picked at runtime.
  # -ffast-math is only for these loops, which never see a NaN.
  buildPhase = ''
    runHook preBuild
    rows="-O3 -ffast-math -fno-strict-aliasing -fno-math-errno -fvisibility=hidden -fPIC"
    $CC $rows -march=x86-64-v3 -mtune=znver4 \
      -mtune-ctrl=use_gather_8parts,use_gather_4parts,use_gather_2parts \
      -DFL_ROWS=fl_rows_v3 -c fl_rows.c -o rows_v3.o
    $CC $rows -DFL_ROWS=fl_rows_base -c fl_rows.c -o rows_base.o
    $CC -O2 -Wall -fPIC -fvisibility=hidden -pthread -c fl_core.c -o core.o
    $CC -O2 -Wall -fPIC -pthread $(pkg-config --cflags ply-splash-core ply-splash-graphics) -c plugin.c -o plugin.o
    $CC -shared -pthread -o first-light.so plugin.o core.o rows_v3.o rows_base.o \
      $(pkg-config --libs ply-splash-core) -lm
    $CC -O2 still.c core.o rows_v3.o rows_base.o -pthread -lm -o first-light-still
    $CC -O2 -Wall vt-mode.c -o first-light-vt-mode
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    install -Dm755 first-light.so $out/lib/plymouth/first-light.so
    install -Dm755 first-light-still $out/bin/first-light-still
    install -Dm755 first-light-vt-mode $out/bin/first-light-vt-mode
    sed 's/@landSeconds@/${toString landSeconds}/' first-light.plymouth \
      > first-light.plymouth.out
    install -Dm644 first-light.plymouth.out \
      $out/share/plymouth/themes/first-light/first-light.plymouth
    runHook postInstall
  '';
}
