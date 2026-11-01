/* First Light: the boot splash's picture, as a pure function of time.
 *
 * This is the approved fragment shader (the MAIN and COMMON strings of ring.html)
 * evaluated on the CPU. Nothing here knows about Plymouth; the plugin and the still
 * tool both draw through this.
 */
#pragma once
#include <stdint.h>

typedef struct fl_view fl_view_t;

/* The avatar ring the splash lands on, in physical pixels. */
typedef struct {
        float cx, cy;
        float r_out, r_in;
} fl_ring_t;

typedef struct {
        double T;       /* the film's clock: pull-back, exit, the prism's slow turn */
        double dawn;    /* the clock of the dawn and the sun; T unless a landing cut it short */
        double breath;  /* 0..1, the hold's slow swell */
        double arrive;  /* 0..1, how far the picture has become exactly the login ring */
        unsigned seed;  /* picks the dither offset */
} fl_frame_t;

typedef struct {
        int x0, y0, x1, y1;     /* half open; empty if x1 <= x0 */
} fl_rect_t;

/* ring == NULL draws the film's own geometry (.0405 radius at y = -.03), which is
 * what the comparison against Chrome uses. */
fl_view_t *fl_view_new (int width, int height, const fl_ring_t *ring);
void fl_view_free (fl_view_t *view);

/* Share the drawing between this many threads, the caller's included. Purely an
 * optimisation: any that can't start just leave more for the rest. 1 stops them. */
void fl_view_set_threads (fl_view_t *view, int threads);

/* Evaluates frame into pixels (XRGB8888, stride in pixels) and says what changed.
 * Keep passing the same pixels: only what the last call touched is cleared. */
void fl_draw (fl_view_t *view, const fl_frame_t *frame, uint32_t *pixels, int stride, fl_rect_t *damage);

/* Where the login ring sits on this output. Exact at 3200x2000 (from ring-geometry.md);
 * elsewhere the same layout at the scale the greeter's niri gives that output. */
fl_ring_t fl_ring_for_output (int width, int height);
