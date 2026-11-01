#!/usr/bin/env python3
"""Checks the splash's last frame against the login ring, from the numbers in
ring-geometry.md, reimplemented here independently of fl_core.c.
    check_ring.py <final.ppm> [greeter render, PNG or PPM]"""
import sys
import numpy as np
from PIL import Image
def load(p):
    return np.array(Image.open(p).convert('RGB')).astype(np.int32)
a = load(sys.argv[1])
H, W = a.shape[:2]
assert (W, H) == (3200, 2000), (W, H)
cx, cy, ro, ri = 1600.375, 1060.1325, 79.8, 77.0
ax, ay = 0.867819, -0.496880
ext = ro * (abs(ax) + abs(ay))
stops = [(0., (.624, .863, 1.)), (.4, (.788, .698, 1.)), (.7, (1., .702, .894)), (1., (1., .824, .659))]
y, x = np.mgrid[0:H, 0:W]
dx, dy = x + .5 - cx, y + .5 - cy
r = np.hypot(dx, dy)
t = np.clip((dx * ax + dy * ay) / ext * .5 + .5, 0, 1)
c = np.stack([np.interp(t, [s[0] for s in stops], [s[1][k] for s in stops]) for k in range(3)], -1)
l = (c * [0.299, 0.587, 0.114]).sum(-1, keepdims=True)
c = c * .86 + l * .14
c = c * .92 + .08
cov = np.clip(.5 - (r - ro), 0, 1) * (1 - np.clip(.5 - (r - ri), 0, 1))
want = np.round(c * cov[..., None] * 255)
diff = np.abs(a - want)
print(f"against ring-geometry.md: max diff {diff.max():.0f}, pixels off by >1: {(diff.max(-1) > 1).sum()}, non-black pixels {(a.max(-1) > 0).sum()} (want {(want.max(-1) > 0).sum()}), outside the ring box: {((a.max(-1) > 0) & (r > 85)).sum()}")
if len(sys.argv) > 2:
    g = load(sys.argv[2])
    g = g[:2000, :3200]
    d = np.abs(a - g)
    box = np.zeros((H, W), bool); box[960:1160, 1500:1700] = True
    print(f"against the greeter's render: ring box max diff {d[box].max()}, >4 steps: {(d[box].max(-1) > 4).sum()} px")
sys.exit(0 if diff.max() <= 2 else 1)
