/* Offline front end to the renderer: a still as a binary PPM, or timings.
 *   still W H T out.ppm [dawnT breath arrive]    the film's geometry unless LANDED=1
 *   still bench W H [threads]                    one pass over the timeline
 *   still replay W H threads [out.ppm]           the landing through damage rectangles
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "fl.h"

static double now (void) { struct timespec t; clock_gettime (CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec * 1e-9; }

static void phase (fl_view_t *v, uint32_t *px, int W, const char *name, double a, double b, int n, double breath)
{
        double sum = 0, worst = 0;
        long area = 0;
        for (int i = 0; i < n; i++) {
                double T = a + (b - a) * i / (n > 1 ? n - 1 : 1);
                fl_frame_t f = { T, T > 7 ? 7 : T, breath, 0, (unsigned) i };
                fl_rect_t d;
                double t0 = now ();
                fl_draw (v, &f, px, W, &d);
                double t = now () - t0;
                sum += t; worst = t > worst ? t : worst;
                area += (long) (d.x1 - d.x0) * (d.y1 - d.y0);
        }
        printf ("%-22s avg %6.2f ms  worst %6.2f ms  (%.1f fps)  avg damage %.1f Mpx\n", name, sum / n * 1e3, worst * 1e3, n / sum, area / (double) n / 1e6);
}

/* The whole landing, as the plugin plays it: a shadow buffer, and a front buffer that only
 * ever receives the damage rectangles. What's left there afterwards is what's on screen. */
static int replay (int W, int H, int threads, double seconds, const char *out)
{
        fl_ring_t ring = fl_ring_for_output (W, H);
        fl_view_t *view = fl_view_new (W, H, &ring);
        fl_view_set_threads (view, threads);
        uint32_t *shadow = calloc ((size_t) W * H, 4), *front = calloc ((size_t) W * H, 4);
        for (size_t i = 0; i < (size_t) W * H; i++)
                shadow[i] = front[i] = 0xff000000u;
        int frames = (int) (seconds * 44.);     /* the rate measured in the VM */
        for (int i = 0; i <= frames; i++) {
                double T = 7. + 6.4 * i / frames;
                double t = (T - 12.4);
                t = t < 0 ? 0 : t > 1 ? 1 : t;
                fl_frame_t f = { T, 7., 0, t * t * (3 - 2 * t), 1 };
                fl_rect_t d = { 0, 0, W, H };
                fl_rect_t *dp = &d;
                fl_draw (view, &f, shadow, W, dp);
                if (i == 0) { d.x0 = 0; d.y0 = 0; d.x1 = W; d.y1 = H; }
                for (int y = d.y0; y < d.y1; y++)
                        memcpy (front + (size_t) y * W + d.x0, shadow + (size_t) y * W + d.x0, (size_t) (d.x1 - d.x0) * 4);
        }
        long fnb = 0, snb = 0, mism = 0;
        for (size_t i = 0; i < (size_t) W * H; i++) {
                fnb += (front[i] & 0xffffff) != 0;
                snb += (shadow[i] & 0xffffff) != 0;
                mism += front[i] != shadow[i];
        }
        printf ("after %d frames: front non-black %ld, shadow non-black %ld, front != shadow %ld\n", frames, fnb, snb, mism);
        if (out != NULL) {
                FILE *o = fopen (out, "wb");
                fprintf (o, "P6\n%d %d\n255\n", W, H);
                unsigned char *row = malloc (W * 3);
                for (int y = 0; y < H; y++) {
                        for (int x = 0; x < W; x++) {
                                uint32_t p = front[(size_t) y * W + x];
                                row[x * 3] = p >> 16; row[x * 3 + 1] = p >> 8; row[x * 3 + 2] = p;
                        }
                        fwrite (row, 3, W, o);
                }
                fclose (o);
        }
        return mism != 0 || fnb != snb;
}

int main (int c, char **v)
{
        if (c >= 5 && strcmp (v[1], "replay") == 0)
                return replay (atoi (v[2]), atoi (v[3]), atoi (v[4]), 3.2, c > 5 ? v[5] : NULL);
        if (c >= 4 && strcmp (v[1], "bench") == 0) {
                int W = atoi (v[2]), H = atoi (v[3]);
                fl_ring_t ring = fl_ring_for_output (W, H);
                fl_view_t *view = fl_view_new (W, H, &ring);
                fl_view_set_threads (view, c > 4 ? atoi (v[4]) : 1);
                uint32_t *px = calloc ((size_t) W * H, 4);
                phase (view, px, W, "dawn (T 0..7)", 0, 7, 60, 0);
                phase (view, px, W, "hold", 7, 7, 40, .6);
                phase (view, px, W, "pull-back (T 7..10)", 7, 10, 60, 0);
                phase (view, px, W, "pull-back (T 10..12.6)", 10, 12.6, 60, 0);
                phase (view, px, W, "settle (T 12.6..13.4)", 12.6, 13.4, 30, 0);
                return 0;
        }
        if (c < 5)
                return 2;
        int W = atoi (v[1]), H = atoi (v[2]);
        double T = atof (v[3]);
        fl_frame_t f = { T, c > 5 ? atof (v[5]) : T, c > 6 ? atof (v[6]) : 0, c > 7 ? atof (v[7]) : 0, 1 };
        fl_ring_t ring = fl_ring_for_output (W, H);
        fl_view_t *view = fl_view_new (W, H, getenv ("LANDED") ? &ring : NULL);
        uint32_t *px = calloc ((size_t) W * H, 4);
        fl_rect_t d;
        fl_draw (view, &f, px, W, &d);
        FILE *o = fopen (v[4], "wb");
        fprintf (o, "P6\n%d %d\n255\n", W, H);
        unsigned char *row = malloc (W * 3);
        for (int y = 0; y < H; y++) {
                for (int x = 0; x < W; x++) {
                        uint32_t p = px[(size_t) y * W + x];
                        row[x * 3] = p >> 16; row[x * 3 + 1] = p >> 8; row[x * 3 + 2] = p;
                }
                fwrite (row, 3, W, o);
        }
        fclose (o);
        return 0;
}
