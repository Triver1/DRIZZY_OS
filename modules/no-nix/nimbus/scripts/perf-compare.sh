#!/usr/bin/env bash
# Delta table between two perf results (A = before, B = after).
#
#   scripts/perf-compare.sh [--threshold PCT] [--all] [--brief] [--strict] A.json B.json
#
# Flags a metric as REGRESSION when B is worse than A by more than PCT %
# (default 5) *and* by more than the metric's absolute noise floor (e.g.
# 0.05 CPU-%-points, 256 KiB of memory, 5 ms startup). "(ranges overlap)"
# means the min–max ranges of the repeats overlap, i.e. likely noise.
# --all also lists informational metrics; --brief only flagged rows;
# --strict exits 2 on regressions.
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec python3 -I "$HERE/perf.py" compare "$@"
