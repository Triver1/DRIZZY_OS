/* The per-pixel loops of First Light. Built twice, once for x86-64-v3 and once for
 * the baseline, from this one source: see fl_core.c for the dispatch. These loops are
 * written for the auto-vectoriser, so no branches, no calls, one thing per loop. */
#include <stdint.h>
#include "fl_int.h"

#ifndef FL_ROWS
#error "build with -DFL_ROWS=name"
#endif

static inline int32_t bits_of (float f) { union { float f; int32_t i; } u = { f }; return u.i; }

/* log2 lookup index of |v| + 1 px: the exponent and FL_BITS bits of mantissa. So the
 * table is finest right at the limb, where the picture is sharpest. */
static inline int32_t log_index (float v) { return (bits_of (v + 1.f) - 0x3f800000) >> (23 - FL_BITS); }

static void
radial (float *restrict D, int32_t *restrict I, float x, float dy2, float rp, int n)
{
        for (int i = 0; i < n; i++) {
                float dx = x + (float) i;
                float d = __builtin_sqrtf (dx * dx + dy2) - rp;
                float a = __builtin_fabsf (d);
                int32_t k = log_index (a);
                k = k < FL_RAD_N ? k : FL_RAD_N - 1;
                D[i] = d;
                I[i] = k + (d < 0.f ? FL_RAD_N : 0);
        }
}

static void
gather (float *restrict out, const float *restrict t, const int32_t *restrict I, int n)
{
        for (int i = 0; i < n; i++)
                out[i] = t[I[i]];
}

/* atan2 (a, b) for a >= 0, in 0..pi. Polynomial good to 1e-5 rad. */
static inline float
angle (float a, float b)
{
        float ab = __builtin_fabsf (b);
        float big = a > ab ? a : ab;
        float sml = a > ab ? ab : a;
        float t = sml / (big + 1e-6f);
        float s = t * t;
        float p = t * (0.9998660f + s * (-0.3302995f + s * (0.1801410f + s * (-0.0851330f + s * 0.0208351f))));
        float q = a > ab ? 1.5707963f - p : p;
        return b < 0.f ? 3.1415927f - q : q;
}

/* m in the shader: the dawn's spread along the limb, by angle from straight up. */
static void
spread (float *restrict M, const float *restrict t, float k, float x, float vy, int n)
{
        int32_t J[FL_CHUNK];
        for (int o = 0; o < n; o += FL_CHUNK) {
                int c = n - o < FL_CHUNK ? n - o : FL_CHUNK;
                for (int i = 0; i < c; i++) {
                        float dx = __builtin_fabsf (x + (float) (o + i));
                        int32_t q = (int32_t) (angle (dx, vy) * k);
                        J[i] = q < FL_M_N - 1 ? q : FL_M_N - 1;
                }
                for (int i = 0; i < c; i++)
                        M[o + i] = t[J[i]];
        }
}

static void
mul (float *restrict a, const float *restrict b, int n)
{
        for (int i = 0; i < n; i++)
                a[i] *= b[i];
}

/* The sun's glint at the top of the limb. */
static void
sun (float *restrict R, float *restrict G, float *restrict B, const float *restrict D,
     const float *restrict tr, const float *restrict tg, const float *restrict tb,
     float x, float dy2, float unit, int n)
{
        int32_t J[FL_CHUNK];
        float C[FL_CHUNK];
        float ir = 1.f / (.012f * unit), off = .007f * unit;
        for (int o = 0; o < n; o += FL_CHUNK) {
                int c = n - o < FL_CHUNK ? n - o : FL_CHUNK;
                for (int i = 0; i < c; i++) {
                        float dx = x + (float) (o + i);
                        int32_t k = log_index (__builtin_sqrtf (dx * dx + dy2));
                        J[i] = k < FL_RAD_N ? k : FL_RAD_N - 1;
                        float t = (D[o + i] + off) * ir;
                        t = t < 0.f ? 0.f : t > 1.f ? 1.f : t;
                        C[i] = t * t * (3.f - 2.f * t);
                }
                for (int i = 0; i < c; i++) R[o + i] += tr[J[i]] * C[i];
                for (int i = 0; i < c; i++) G[o + i] += tg[J[i]] * C[i];
                for (int i = 0; i < c; i++) B[o + i] += tb[J[i]] * C[i];
        }
}

/* The prism: colour by angle around the ball, weighted by Q. */
static void
prism (float *restrict R, float *restrict G, float *restrict B, const float *restrict Q,
       const float *restrict tr, const float *restrict tg, const float *restrict tb,
       float x, float vy, int n)
{
        int32_t J[FL_CHUNK];
        const float k = (float) FL_TINT_N / 6.2831853f;
        for (int o = 0; o < n; o += FL_CHUNK) {
                int c = n - o < FL_CHUNK ? n - o : FL_CHUNK;
                for (int i = 0; i < c; i++) {
                        float dx = x + (float) (o + i);
                        float a = angle (__builtin_fabsf (dx), vy);     /* from up, 0..pi */
                        a = dx < 0.f ? 6.2831853f - a : a;              /* clockwise from up */
                        int32_t q = (int32_t) (a * k);
                        J[i] = q & (FL_TINT_N - 1);
                }
                for (int i = 0; i < c; i++) R[o + i] += tr[J[i]] * Q[o + i];
                for (int i = 0; i < c; i++) G[o + i] += tg[J[i]] * Q[o + i];
                for (int i = 0; i < c; i++) B[o + i] += tb[J[i]] * Q[o + i];
        }
}

/* finish() up to the dither: tone map and gamma, as one table on log c. */
static void
tone (float *restrict V, const float *restrict t, int n)
{
        int32_t J[FL_CHUNK];
        for (int o = 0; o < n; o += FL_CHUNK) {
                int c = n - o < FL_CHUNK ? n - o : FL_CHUNK;
                for (int i = 0; i < c; i++) {
                        float v = V[o + i] + 5.9604645e-8f;     /* 2^-24, table's floor */
                        int32_t k = (bits_of (v) >> (23 - FL_TONE_BITS)) - (FL_TONE_FLOOR << FL_TONE_BITS);
                        J[i] = k < FL_TONE_N ? k : FL_TONE_N - 1;
                }
                for (int i = 0; i < c; i++)
                        V[o + i] = t[J[i]];
        }
}

/* The last line of finish(), and the write. noise holds 512 per row, so a run of up to
 * 256 can start anywhere in the first 256. */
static void
pack (uint32_t *restrict out, const float *restrict R, const float *restrict G, const float *restrict B,
      const float *restrict noise, float amp, int n)
{
        for (int i = 0; i < n; i++) {
                float w = noise[i] * amp + .5f;
                float r = R[i] + w, g = G[i] + w, b = B[i] + w;
                r = r < 0.f ? 0.f : r > 255.f ? 255.f : r;
                g = g < 0.f ? 0.f : g > 255.f ? 255.f : g;
                b = b < 0.f ? 0.f : b > 255.f ? 255.f : b;
                out[i] = 0xff000000u | ((uint32_t) (int32_t) r << 16) | ((uint32_t) (int32_t) g << 8) | (uint32_t) (int32_t) b;
        }
}

/* One run of pixels of one row: x0 is its first column, x1 past its last. */
void
FL_ROWS (const fl_tables_t *t, fl_work_t *w, int y, int x0, int x1, uint32_t *row)
{
        const int n = x1 - x0;
        if (n <= 0)
                return;
        const float px = x0 + .5f - t->cx;
        const float vy = t->cy - (y + .5f);     /* y up, as in the shader */
        const float dy2 = vy * vy;

        radial (w->D, w->I, px, dy2, t->rp, n);
        gather (w->R, t->rad[0], w->I, n);
        gather (w->G, t->rad[1], w->I, n);
        gather (w->B, t->rad[2], w->I, n);

        if (t->has_spread) {
                spread (w->M, t->m, t->m_k, px, vy, n);
                mul (w->R, w->M, n);
                mul (w->G, w->M, n);
                mul (w->B, w->M, n);
        }

        if (t->has_sun) {
                float sy = t->sun_y - (y + .5f);
                int a = 0, b = 0;
                fl_chord (t->sun_x - x0, sy, t->sun_r, n, &a, &b);
                if (b > a)
                        sun (w->R + a, w->G + a, w->B + a, w->D + a, t->sun_t[0], t->sun_t[1], t->sun_t[2],
                             x0 + a + .5f - t->sun_x, sy * sy, t->unit, b - a);
        }

        if (t->has_ring) {
                gather (w->Q, t->q[0], w->I, n);
                if (t->has_spread) {
                        gather (w->Q2, t->q[1], w->I, n);
                        mul (w->Q2, w->M, n);
                } else {
                        gather (w->Q2, t->q[1], w->I, n);
                }
                for (int i = 0; i < n; i++)
                        w->Q[i] += w->Q2[i];
                int a = 0, b = 0;
                fl_chord (t->cx - x0, vy, t->q_r, n, &a, &b);
                if (b > a)
                        prism (w->R + a, w->G + a, w->B + a, w->Q + a, t->tint[0], t->tint[1], t->tint[2],
                               x0 + a + .5f - t->cx, vy, b - a);
        }

        tone (w->R, t->tone, n);
        tone (w->G, t->tone, n);
        tone (w->B, t->tone, n);

        /* Near the end the shader gives way to the real ring. */
        if (t->keep < 1.f) {
                for (int i = 0; i < n; i++) w->R[i] *= t->keep;
                for (int i = 0; i < n; i++) w->G[i] *= t->keep;
                for (int i = 0; i < n; i++) w->B[i] *= t->keep;
                fl_ring_over (t, w, y, x0, n);
        }

        const float *noise_row = t->noise[(y + t->ny) & 255];
        for (int o = 0; o < n; ) {
                int s = (x0 + o + t->nx) & 255;
                int c = n - o < 256 ? n - o : 256;
                pack (row + o, w->R + o, w->G + o, w->B + o, noise_row + s, t->amp, c);
                o += c;
        }
}
