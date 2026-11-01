/* First Light, the frame: everything that only depends on time goes into tables here,
 * once per frame; the per-pixel work is in fl_rows.c. Every constant below is the
 * shader's, in the shader's order. */
#include <math.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include "fl.h"
#include "fl_int.h"

#define FL_MAX_THREADS 4        /* the caller's thread and up to three helpers */
#define FL_ROWS_AT_ONCE 24

struct fl_view {
        int w, h;
        float unit;                     /* shader units are heights; "cover" for other shapes */
        int landed;
        fl_ring_t ring;
        fl_tables_t *t;
        fl_work_t *work[FL_MAX_THREADS];
        fl_rows_fn rows;
        float (*noise)[512];
        float *ring_px;
        fl_rect_t last;

        /* one fl_draw, shared with the helpers */
        struct {
                uint32_t *pixels;
                int stride, y0, y1, ly0, ly1;
                float reach, hollow;
                int next;
        } job;

        /* helpers: asleep unless a frame is being drawn */
        pthread_t helper[FL_MAX_THREADS - 1];
        int helpers;
        pthread_mutex_t lock;
        pthread_cond_t wake, finished;
        unsigned generation;
        int busy, quit, locks_ready;
};

typedef struct { fl_view_t *v; int index; } helper_arg_t;

typedef struct { double x, y, z; } v3;

static double clamp01 (double x) { return x < 0 ? 0 : x > 1 ? 1 : x; }
static double mix (double a, double b, double t) { return a + (b - a) * t; }
static double S (double a, double b, double x) { double t = clamp01 ((x - a) / (b - a)); return t * t * (3 - 2 * t); }
static double ease (double x) { x = clamp01 (x); return x * x * x * (x * (x * 6 - 15) + 10); }
static double lin1 (double c) { return pow (c, 2.2); }
static v3 lin (v3 c) { return (v3) { lin1 (c.x), lin1 (c.y), lin1 (c.z) }; }
static v3 v3_mix (v3 a, v3 b, double t) { return (v3) { mix (a.x, b.x, t), mix (a.y, b.y, t), mix (a.z, b.z, t) }; }


static const v3 C0 = { .624, .863, 1. }, C1 = { .788, .698, 1. }, C2 = { 1., .702, .894 }, C3 = { 1., .824, .659 };

static v3
prism (double x)
{
        x = (x - floor (x)) * 4.;
        double k = x - floor (x);
        k = k * k * (3. - 2. * k);
        if (x < 1.) return lin (v3_mix (C0, C1, k));
        if (x < 2.) return lin (v3_mix (C1, C2, k));
        if (x < 3.) return lin (v3_mix (C2, C3, k));
        return lin (v3_mix (C3, C0, k));
}

/* The login ring's colour at an offset from its centre, y down: the gradient of
 * ring-geometry.md, as display values. */
static v3
ring_color (double dx, double dy, const fl_ring_t *ring)
{
        static const double ax = 0.867819, ay = -0.496880;      /* cos, sin of -.52 rad */
        double ext = ring->r_out * (fabs (ax) + fabs (ay));
        double t = clamp01 ((dx * ax + dy * ay) / ext * .5 + .5);
        v3 c;
        if (t < .4) c = v3_mix (C0, C1, t / .4);
        else if (t < .7) c = v3_mix (C1, C2, (t - .4) / .3);
        else c = v3_mix (C2, C3, (t - .7) / .3);
        double l = c.x * .299 + c.y * .587 + c.z * .114;
        c = v3_mix (c, (v3) { l, l, l }, .14);
        return v3_mix (c, (v3) { 1, 1, 1 }, .08);
}

fl_ring_t
fl_ring_for_output (int width, int height)
{
        if (width == 3200 && height == 2000)
                return (fl_ring_t) { 1600.375f, 1060.1325f, 79.8f, 77.0f };
        /* Any other output: same layout at scale 1, which is what the greeter's niri
         * leaves everything but the laptop's panel at. */
        return (fl_ring_t) { width * .5f, height * .53f, 45.6f, 44.f };
}

void
fl_chord (float cx, float dy, float r, int n, int *a, int *b)
{
        float h2 = r * r - dy * dy;
        *a = *b = 0;
        if (h2 <= 0.f)
                return;
        float h = sqrtf (h2);
        int lo = (int) floorf (cx - h - .5f), hi = (int) ceilf (cx + h + .5f);
        *a = lo < 0 ? 0 : lo > n ? n : lo;
        *b = hi < 0 ? 0 : hi > n ? n : hi;
}

void
fl_ring_over (const fl_tables_t *t, fl_work_t *w, int y, int x0, int n)
{
        int ry = y - t->ring_y;
        if (ry < 0 || ry >= t->ring_h)
                return;
        int a = t->ring_x > x0 ? t->ring_x : x0;
        int b = t->ring_x + t->ring_w < x0 + n ? t->ring_x + t->ring_w : x0 + n;
        const float *p = t->ring_px + (size_t) ry * t->ring_w * 3 + (size_t) (a - t->ring_x) * 3;
        const float k = t->ring_a;
        for (int x = a; x < b; x++, p += 3) {
                w->R[x - x0] += p[0] * k;
                w->G[x - x0] += p[1] * k;
                w->B[x - x0] += p[2] * k;
        }
}

static float
hash (float x, float y)
{
        /* hash() of COMMON */
        float q0 = x * .1031f, q1 = y * .1031f, q2 = x * .1031f;
        q0 -= floorf (q0); q1 -= floorf (q1); q2 -= floorf (q2);
        float d = q0 * (q1 + 33.33f) + q1 * (q2 + 33.33f) + q2 * (q0 + 33.33f);
        q0 += d; q1 += d; q2 += d;
        float r = (q0 + q1) * q2;
        return r - floorf (r);
}

static double tone_of (double c) { return pow (1. - exp (-c * 1.15), 1. / 2.2) * 255.; }

fl_view_t *
fl_view_new (int width, int height, const fl_ring_t *ring)
{
        if (width <= 0 || height <= 0 || width > FL_MAX_W)
                return NULL;
        fl_view_t *v = calloc (1, sizeof *v);
        if (v == NULL)
                return NULL;
        v->w = width;
        v->h = height;
        /* The picture was composed at 16:10. Fill any other shape, centred. */
        v->unit = fmaxf ((float) height, width / 1.6f);
        v->landed = ring != NULL;
        if (ring != NULL)
                v->ring = *ring;
        v->rows = fl_rows_base;
        __builtin_cpu_init ();
        if (__builtin_cpu_supports ("avx2") && __builtin_cpu_supports ("fma") && __builtin_cpu_supports ("bmi2"))
                v->rows = fl_rows_v3;
        if (posix_memalign ((void **) &v->t, 64, sizeof *v->t) != 0 ||
            posix_memalign ((void **) &v->work[0], 64, sizeof (fl_work_t)) != 0 ||
            posix_memalign ((void **) &v->noise, 64, sizeof (float) * 512 * 256) != 0) {
                fl_view_free (v);
                return NULL;
        }
        memset (v->t, 0, sizeof *v->t);
        for (int y = 0; y < 256; y++)
                for (int x = 0; x < 512; x++)
                        v->noise[y][x] = (hash ((float) (x & 255) + .5f, (float) y + .5f) - .5f) * 2.2f;
        v->t->noise = v->noise;

        for (int i = 0; i < FL_TONE_N; i++) {
                int e = i >> FL_TONE_BITS, m = i & ((1 << FL_TONE_BITS) - 1);
                double c = ldexp (1. + (m + .5) / (1 << FL_TONE_BITS), e + FL_TONE_FLOOR - 127);
                /* the floor added before the lookup must not lift black */
                c -= 5.9604645e-8;
                v->t->tone[i] = c <= 0. ? 0.f : (float) tone_of (c);
        }

        if (v->landed) {
                int rw = (int) ceil (v->ring.r_out) + 2;
                fl_tables_t *t = v->t;
                t->ring_x = (int) floor (v->ring.cx) - rw;
                t->ring_y = (int) floor (v->ring.cy) - rw;
                t->ring_w = t->ring_h = 2 * rw + 1;
                v->ring_px = calloc ((size_t) t->ring_w * t->ring_h * 3, sizeof (float));
                if (v->ring_px == NULL) {
                        fl_view_free (v);
                        return NULL;
                }
                for (int y = 0; y < t->ring_h; y++)
                        for (int x = 0; x < t->ring_w; x++) {
                                double dx = t->ring_x + x + .5 - v->ring.cx, dy = t->ring_y + y + .5 - v->ring.cy;
                                double r = hypot (dx, dy);
                                /* as the greeter: a disc of the colour, then a black one over it */
                                double out = clamp01 (.5 - (r - v->ring.r_out)), in = clamp01 (.5 - (r - v->ring.r_in));
                                v3 c = ring_color (dx, dy, &v->ring);
                                double k = out * (1. - in) * 255.;
                                float *p = v->ring_px + (size_t) (y * t->ring_w + x) * 3;
                                p[0] = (float) (c.x * k);
                                p[1] = (float) (c.y * k);
                                p[2] = (float) (c.z * k);
                        }
                t->ring_px = v->ring_px;
        }
        return v;
}

void
fl_view_free (fl_view_t *v)
{
        if (v == NULL)
                return;
        fl_view_set_threads (v, 1);
        if (v->locks_ready) {
                pthread_mutex_destroy (&v->lock);
                pthread_cond_destroy (&v->wake);
                pthread_cond_destroy (&v->finished);
        }
        free (v->t);
        for (int i = 0; i < FL_MAX_THREADS; i++)
                free (v->work[i]);
        free (v->noise);
        free (v->ring_px);
        free (v);
}

/* Fills t from the frame, and says how far the picture reaches. */
static void
prepare (fl_view_t *v, const fl_frame_t *f, float *reach, float *hollow)
{
        fl_tables_t *t = v->t;
        const double T = f->T, U = v->unit;
        const double h2 = v->h * .5, w2 = v->w * .5;

        const double g = ease ((f->dawn - .6) / 6.4);
        const double z = ease ((T - 7.) / 5.6);
        const double e = ease (S (11.6, 13.4, T));

        /* The film's landing, or the real ring's. */
        double r_end = .0405, c_end_x = 0., c_end_y = .0105 - .0405;
        if (v->landed) {
                r_end = .5 * (v->ring.r_out + v->ring.r_in) / U;
                c_end_x = (v->ring.cx - w2) / U;
                c_end_y = (h2 - v->ring.cy) / U;
        }
        const double Rp = exp (mix (log (1.56), log (r_end), z));
        const double top = mix (-.16, c_end_y + r_end, z);
        const double cy = top - Rp, cx = mix (0., c_end_x, z);

        t->unit = (float) U;
        t->rp = (float) (Rp * U);
        t->cx = (float) (w2 + cx * U);
        t->cy = (float) (h2 - cy * U);

        const double spread = mix (.012, .5, g * g);
        const double m_flat = S (.15, .72, z);
        const double w = mix (1., .34, z);
        const v3 peach = lin (C3), pink = lin (C2), lilac = lin (C1), sky = lin (C0);
        const double ring = S (.55, 1., z);
        const v3 edge = v3_mix (peach, (v3) { 1, 1, 1 }, .55);
        const double fade_in = S (0., .5, T);   /* the film opened on a cut; a splash fades up */
        const double glow = S (0., .14, g) * (1. + .12 * f->breath) * fade_in;
        const double shine = (1. + .05 * f->breath) * fade_in;
        const double limb_w = mix (.0011, .00095, z);

        t->has_ring = ring > 0.;
        t->q_r = (float) (Rp * U + .16 * U);

        /* outside the limb, then inside it */
        for (int side = 0; side < 2; side++) {
                for (int i = 0; i < FL_RAD_N; i++) {
                        int oct = i >> FL_BITS, m = i & ((1 << FL_BITS) - 1);
                        double px = ldexp (1. + (m + .5) / (1 << FL_BITS), oct) - 1.;
                        double d = (side ? -px : px) / U;
                        double h = d > 0. ? d : 0.;
                        double outside = S (-.0009, .0009, d);
                        double limb = exp (-fabs (d) / limb_w);
                        double k = glow * (1. - ring) * outside * (1. - e);
                        double e1 = exp (-h / (.0065 * w)) * 1.5, e2 = exp (-h / (.021 * w)) * .46;
                        double e3 = exp (-h / (.05 * w)) * .09, e4 = exp (-h / (.13 * w)) * .022;
                        double in = (1. - outside) * .012 * exp (d / .018) * (1. - ring);
                        double lk = limb * 1.1 * (1. - ring) * shine;
                        int o = side * FL_RAD_N + i;
                        t->rad[0][o] = (float) ((peach.x * e1 + pink.x * e2 + lilac.x * e3 + sky.x * e4) * k + edge.x * lk + peach.x * in);
                        t->rad[1][o] = (float) ((peach.y * e1 + pink.y * e2 + lilac.y * e3 + sky.y * e4) * k + edge.y * lk + peach.y * in);
                        t->rad[2][o] = (float) ((peach.z * e1 + pink.z * e2 + lilac.z * e3 + sky.z * e4) * k + edge.z * lk + peach.z * in);
                        if (t->has_ring) {
                                t->q[0][o] = (float) (ring * exp (-h / .006) * .5 * outside * (1. - e));
                                t->q[1][o] = (float) (limb * 1.1 * 1.5 * ring * shine);
                        }
                }
        }

        t->has_spread = m_flat < 1.;
        if (t->has_spread) {
                double range = 4. * spread;
                t->m_k = (float) ((FL_M_N - 1) / range);
                for (int i = 0; i < FL_M_N; i++) {
                        double a = (i + .5) / t->m_k;
                        double m = i == FL_M_N - 1 ? 0. : exp (-pow (a / spread, 1.7));
                        t->m[i] = (float) mix (m, 1., m_flat);
                }
        }

        const double sun = ease ((f->dawn - 4.6) / 2.6) * (1. - S (0., .34, z)) * shine;
        t->has_sun = sun > 0.;
        if (t->has_sun) {
                t->sun_x = (float) (w2 + cx * U);
                t->sun_y = (float) (h2 - (top + .002) * U);
                t->sun_r = (float) (.30 * U);
                for (int i = 0; i < FL_RAD_N; i++) {
                        int oct = i >> FL_BITS, m = i & ((1 << FL_BITS) - 1);
                        double sd = (ldexp (1. + (m + .5) / (1 << FL_BITS), oct) - 1.) / U;
                        double a = exp (-sd / .03) * .7 * sun, b = exp (-sd / .0065) * sun;
                        t->sun_t[0][i] = (float) (peach.x * a + b);
                        t->sun_t[1][i] = (float) (peach.y * a + b);
                        t->sun_t[2][i] = (float) (peach.z * a + b);
                }
        }

        if (t->has_ring) {
                /* Clockwise from up. tint in the shader: prism (atan (v.y, v.x) / TAU + .12 + T * .012),
                 * giving way at the very end to the login ring's own colours. */
                const double phase = .12 + T * .012;
                const double gift = v->landed ? S (.8, 1., z) : 0.;
                for (int i = 0; i < FL_TINT_N; i++) {
                        double a = (i + .5) / FL_TINT_N * 2. * M_PI;
                        double th = M_PI * .5 - a;
                        v3 c = prism (th / (2. * M_PI) + phase);
                        if (gift > 0.) {
                                v3 r = ring_color (sin (a) * Rp * U, -cos (a) * Rp * U, &v->ring);
                                c = v3_mix (c, lin (r), gift);
                        }
                        t->tint[0][i] = (float) c.x;
                        t->tint[1][i] = (float) c.y;
                        t->tint[2][i] = (float) c.z;
                }
        }

        t->ring_a = v->landed ? (float) f->arrive : 0.f;
        t->keep = (float) (1. - f->arrive);
        t->amp = t->keep;
        t->nx = (int) (f->seed * 97u) & 255;
        t->ny = (int) (f->seed * 53u) & 255;

        /* How far the light reaches, past the limb: until the brightest term is under
         * half a step at the bottom of the tone curve (about 1e-7 in, 1 step out). */
        double glow_left = glow * (1. - ring) * (1. - e);
        double far = 0.;
        if (glow_left > 0.) {
                double f1 = .13 * w * log (fmax (1., .022 * glow_left / 2e-7));
                double f2 = .05 * w * log (fmax (1., .09 * glow_left / 2e-7));
                far = fmax (f1, f2);
        }
        far = fmax (far, .16);
        *reach = (float) ((Rp + far) * U);
        /* and how deep inside the planet anything but black survives */
        *hollow = (float) ((Rp - .24) * U);
}

static void
draw_row (fl_view_t *v, fl_work_t *work, int y)
{
        fl_tables_t *t = v->t;
        const int y0 = v->job.y0, y1 = v->job.y1;
        const float reach = v->job.reach, hollow = v->job.hollow;
        uint32_t *row = v->job.pixels + (size_t) y * v->job.stride;
        float vy = t->cy - (y + .5f);
        int a = 0, b = 0;

        if (y >= y0 && y < y1) {
                fl_chord (t->cx, vy, reach, v->w, &a, &b);
                if (t->has_sun) {
                        int sa, sb;
                        fl_chord (t->sun_x, t->sun_y - (y + .5f), t->sun_r, v->w, &sa, &sb);
                        if (sb > sa) {
                                a = b > a ? (sa < a ? sa : a) : sa;
                                b = sb > b ? sb : b;
                        }
                }
                if (t->ring_a > 0.f && y >= t->ring_y && y < t->ring_y + t->ring_h) {
                        a = b > a ? (t->ring_x < a ? t->ring_x : a) : t->ring_x;
                        b = t->ring_x + t->ring_w > b ? t->ring_x + t->ring_w : b;
                }
        }
        a = a < 0 ? 0 : a;
        b = b > v->w ? v->w : b;

        /* the planet's unlit middle: nothing to compute there */
        int ha = a, hb = a;
        if (b > a && hollow > 0.f && !t->has_sun) {
                fl_chord (t->cx, vy, hollow, v->w, &ha, &hb);
                ha = ha < a ? a : ha;
                hb = hb > b ? b : hb;
                if (hb <= ha)
                        ha = hb = a;
        }

        const uint32_t black = 0xff000000u;
        for (int x = 0; x < a; x++) row[x] = black;
        for (int x = ha; x < hb; x++) row[x] = black;
        for (int x = b > a ? b : 0; x < v->w; x++) row[x] = black;
        if (b > a) {
                if (hb > ha) {
                        v->rows (t, work, y, a, ha, row + a);
                        v->rows (t, work, y, hb, b, row + hb);
                } else {
                        v->rows (t, work, y, a, b, row + a);
                }
        }
}

static void
draw_rows (fl_view_t *v, fl_work_t *work)
{
        for (;;) {
                int a = __atomic_fetch_add (&v->job.next, FL_ROWS_AT_ONCE, __ATOMIC_RELAXED);
                if (a >= v->job.ly1)
                        return;
                int b = a + FL_ROWS_AT_ONCE < v->job.ly1 ? a + FL_ROWS_AT_ONCE : v->job.ly1;
                for (int y = a; y < b; y++)
                        draw_row (v, work, y);
        }
}

static void *
helper_main (void *data)
{
        helper_arg_t arg = *(helper_arg_t *) data;
        fl_view_t *v = arg.v;
        unsigned seen = 0;
        free (data);
        for (;;) {
                pthread_mutex_lock (&v->lock);
                while (!v->quit && v->generation == seen)
                        pthread_cond_wait (&v->wake, &v->lock);
                if (v->quit) {
                        pthread_mutex_unlock (&v->lock);
                        return NULL;
                }
                seen = v->generation;
                pthread_mutex_unlock (&v->lock);

                draw_rows (v, v->work[arg.index]);

                pthread_mutex_lock (&v->lock);
                if (--v->busy == 0)
                        pthread_cond_signal (&v->finished);
                pthread_mutex_unlock (&v->lock);
        }
}

void
fl_view_set_threads (fl_view_t *v, int threads)
{
        threads = threads > FL_MAX_THREADS ? FL_MAX_THREADS : threads;
        int want = threads > 1 ? threads - 1 : 0;

        if (v->helpers > 0) {
                pthread_mutex_lock (&v->lock);
                v->quit = 1;
                pthread_cond_broadcast (&v->wake);
                pthread_mutex_unlock (&v->lock);
                for (int i = 0; i < v->helpers; i++)
                        pthread_join (v->helper[i], NULL);
                v->helpers = 0;
                v->quit = 0;
                v->generation = 0;
        }
        if (want == 0)
                return;
        if (!v->locks_ready) {
                pthread_mutex_init (&v->lock, NULL);
                pthread_cond_init (&v->wake, NULL);
                pthread_cond_init (&v->finished, NULL);
                v->locks_ready = 1;
        }
        /* Helpers are only ever an optimisation: with none, the caller draws it all. */
        for (int i = 0; i < want; i++) {
                if (v->work[i + 1] == NULL &&
                    posix_memalign ((void **) &v->work[i + 1], 64, sizeof (fl_work_t)) != 0) {
                        v->work[i + 1] = NULL;
                        break;
                }
                helper_arg_t *arg = malloc (sizeof *arg);
                if (arg == NULL)
                        break;
                *arg = (helper_arg_t) { v, i + 1 };
                if (pthread_create (&v->helper[i], NULL, helper_main, arg) != 0) {
                        free (arg);
                        break;
                }
                v->helpers++;
        }
}

void
fl_draw (fl_view_t *v, const fl_frame_t *f, uint32_t *pixels, int stride, fl_rect_t *damage)
{
        fl_tables_t *t = v->t;
        float reach, hollow;
        prepare (v, f, &reach, &hollow);

        /* The lit part of this frame: its rows, then its columns. fl_chord rounds outward,
         * and a pixel written outside the damage would stay on screen for good, hence the
         * margin. */
        const int margin = 2;
        int y0 = (int) floorf (t->cy - reach) - margin, y1 = (int) ceilf (t->cy + reach) + margin;
        int x0 = (int) floorf (t->cx - reach) - margin, x1 = (int) ceilf (t->cx + reach) + margin;
        if (t->has_sun) {
                int a = (int) floorf (t->sun_y - t->sun_r) - margin, b = (int) ceilf (t->sun_y + t->sun_r) + margin;
                int c = (int) floorf (t->sun_x - t->sun_r) - margin, d = (int) ceilf (t->sun_x + t->sun_r) + margin;
                y0 = a < y0 ? a : y0;
                y1 = b > y1 ? b : y1;
                x0 = c < x0 ? c : x0;
                x1 = d > x1 ? d : x1;
        }
        if (t->ring_a > 0.f) {
                y0 = t->ring_y < y0 ? t->ring_y : y0;
                y1 = t->ring_y + t->ring_h > y1 ? t->ring_y + t->ring_h : y1;
                x0 = t->ring_x < x0 ? t->ring_x : x0;
                x1 = t->ring_x + t->ring_w > x1 ? t->ring_x + t->ring_w : x1;
        }
        y0 = y0 < 0 ? 0 : y0 > v->h ? v->h : y0;
        y1 = y1 < 0 ? 0 : y1 > v->h ? v->h : y1;
        x0 = x0 < 0 ? 0 : x0 > v->w ? v->w : x0;
        x1 = x1 < 0 ? 0 : x1 > v->w ? v->w : x1;
        fl_rect_t now = { x0, y0, x1, y1 };
        if (x1 <= x0 || y1 <= y0)
                now = (fl_rect_t) { 0, 0, 0, 0 };

        /* Rows lit last time but not this time need clearing too. */
        int ly0 = v->last.y1 > v->last.y0 && v->last.y0 < y0 ? v->last.y0 : y0;
        int ly1 = v->last.y1 > v->last.y0 && v->last.y1 > y1 ? v->last.y1 : y1;

        v->job.pixels = pixels;
        v->job.stride = stride;
        v->job.y0 = y0;
        v->job.y1 = y1;
        v->job.ly0 = ly0;
        v->job.ly1 = ly1;
        v->job.reach = reach;
        v->job.hollow = hollow;
        v->job.next = ly0;

        if (v->helpers > 0) {
                pthread_mutex_lock (&v->lock);
                v->busy = v->helpers;
                v->generation++;
                pthread_cond_broadcast (&v->wake);
                pthread_mutex_unlock (&v->lock);
        }
        draw_rows (v, v->work[0]);
        if (v->helpers > 0) {
                pthread_mutex_lock (&v->lock);
                while (v->busy > 0)
                        pthread_cond_wait (&v->finished, &v->lock);
                pthread_mutex_unlock (&v->lock);
        }

        if (damage != NULL) {
                *damage = now;
                if (v->last.x1 > v->last.x0) {
                        if (now.x1 <= now.x0) {
                                *damage = v->last;
                        } else {
                                damage->x0 = v->last.x0 < damage->x0 ? v->last.x0 : damage->x0;
                                damage->y0 = v->last.y0 < damage->y0 ? v->last.y0 : damage->y0;
                                damage->x1 = v->last.x1 > damage->x1 ? v->last.x1 : damage->x1;
                                damage->y1 = v->last.y1 > damage->y1 ? v->last.y1 : damage->y1;
                        }
                }
        }
        v->last = now;
}
