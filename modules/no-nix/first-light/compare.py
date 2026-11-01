#!/usr/bin/env python3
"""Checks the C renderer against Chrome's render of the approved shader.

    compare.py <still> <dir of Chrome PNGs named c<T>.png> [T ...]

Both images carry their own dither (different noise on purpose), so a raw pixel diff
mostly measures two noise fields. "smooth" compares 9x9 box means instead; "raw" is the
plain diff, where anything above about 2.2 steps is more than dither can explain.
Exits non-zero if a frame is off by more than the limits below.
"""
import subprocess, sys, tempfile, os
import numpy as np
from PIL import Image

MAX_SMOOTH_P999 = 1.5   # steps of 255, 99.9th percentile of the smoothed diff
MAX_RAW_P999 = 4.0

def ppm(path):
    with open(path, 'rb') as f:
        assert f.readline().strip() == b'P6'
        w, h = map(int, f.readline().split())
        f.readline()
        return np.frombuffer(f.read(), np.uint8).reshape(h, w, 3).astype(np.float32)

def box9(a):
    c = np.cumsum(np.cumsum(a, 0), 1)
    c = np.pad(c, ((1, 0), (1, 0), (0, 0)))
    k = 9
    return (c[k:, k:] - c[:-k, k:] - c[k:, :-k] + c[:-k, :-k]) / (k * k)

still, refdir = sys.argv[1:3]
times = sys.argv[3:] or ['2.5', '5', '7', '8.5', '9.8', '11', '12.6']
ok = True
print(f"{'T':>5} {'raw mean':>9} {'raw p99.9':>10} {'raw max':>8} {'smooth mean':>12} {'smooth p99.9':>13} {'smooth max':>11}")
for t in times:
    ref = np.array(Image.open(f'{refdir}/c{t}.png').convert('RGB')).astype(np.float32)
    with tempfile.TemporaryDirectory() as d:
        out = os.path.join(d, 'f.ppm')
        subprocess.check_call([still, '3200', '2000', t, out])
        a = ppm(out)
    raw = np.abs(a - ref)
    sm = np.abs(box9(a) - box9(ref))
    r9, s9 = np.percentile(raw, 99.9), np.percentile(sm, 99.9)
    print(f"{t:>5} {raw.mean():9.3f} {r9:10.2f} {raw.max():8.0f} {sm.mean():12.3f} {s9:13.2f} {sm.max():11.2f}")
    ok &= bool(s9 <= MAX_SMOOTH_P999 and r9 <= MAX_RAW_P999)
sys.exit(0 if ok else 1)
