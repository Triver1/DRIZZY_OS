/* What fl_core.c and the per-pixel loops share. */
#pragma once
#include <stdint.h>

#define FL_BITS 7                       /* sub-steps per octave of distance */
#define FL_OCTAVES 14                   /* 1 .. 16384 px */
#define FL_RAD_N (FL_OCTAVES << FL_BITS)
#define FL_M_N 2048
#define FL_TINT_N 2048                  /* a power of two */
#define FL_CHUNK 1024
#define FL_TONE_BITS 8
#define FL_TONE_FLOOR 103               /* exponent of 2^-24 */
#define FL_TONE_N (30 << FL_TONE_BITS)  /* 2^-24 .. 2^6 */
#define FL_MAX_W 16384

typedef struct {
        float cx, cy;                   /* the ball's centre, px from the top left */
        float rp;                       /* its radius, px */
        float unit;                     /* px per shader unit */
        float rad[3][2 * FL_RAD_N];     /* multiplied by m: outside, then inside */
        float q[2][2 * FL_RAD_N];       /* multiplied by the prism colour; q[1] by m too */
        float q_r;                      /* the prism's reach, px from the centre */
        int has_spread, has_sun, has_ring;
        float m[FL_M_N];
        float m_k;                      /* radians to m index */
        float sun_x, sun_y, sun_r;
        float sun_t[3][FL_RAD_N];
        float tint[3][FL_TINT_N];
        float tone[FL_TONE_N];
        float keep, amp;                /* share of the shader left, and of its dither */
        int nx, ny;                     /* dither offset */
        float (*noise)[512];
        /* the exact login ring, display values 0..255 */
        int ring_x, ring_y, ring_w, ring_h;
        const float *ring_px;           /* ring_w * ring_h * 3 */
        float ring_a;
} fl_tables_t;

typedef struct {
        float D[FL_MAX_W], R[FL_MAX_W], G[FL_MAX_W], B[FL_MAX_W];
        float M[FL_MAX_W], Q[FL_MAX_W], Q2[FL_MAX_W];
        int32_t I[FL_MAX_W];
} fl_work_t;

typedef void (*fl_rows_fn) (const fl_tables_t *t, fl_work_t *w, int y, int x0, int x1, uint32_t *row);
void fl_rows_v3 (const fl_tables_t *t, fl_work_t *w, int y, int x0, int x1, uint32_t *row);
void fl_rows_base (const fl_tables_t *t, fl_work_t *w, int y, int x0, int x1, uint32_t *row);

/* The run of [0, n) inside a circle of radius r around (cx, 0), dy from its row. */
void fl_chord (float cx, float dy, float r, int n, int *a, int *b);
void fl_ring_over (const fl_tables_t *t, fl_work_t *w, int y, int x0, int n);
