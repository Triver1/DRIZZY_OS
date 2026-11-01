/* First Light as a Plymouth splash plugin.
 *
 * Why not a script theme: Plymouth's script plugin has no clock, and on this panel a
 * single full-screen PNG sprite costs 30 ms to blend and a dithered frame 3.6 MB and
 * 150 ms to decode. Here the picture is computed, so the theme ships no images at all
 * and any output size is simply drawn at its own resolution.
 *
 * The film has no fixed length. The dawn plays once and then the limb breathes for as
 * long as the boot takes. `plymouth update --status=land` starts the landing. So does
 * being asked to go idle, which is what `plymouth deactivate` and `plymouth quit` do,
 * and those are only answered once the last frame is on screen. That frame stays there
 * for as long as plymouthd runs.
 *
 * Failing safely is the point of the shape of this file: if a view can't be set up it
 * stays black, and nothing here ever waits on anything but its own timer.
 */
#define _GNU_SOURCE
#include <assert.h>
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <ply-boot-splash-plugin.h>
#include <ply-event-loop.h>
#include <ply-key-file.h>
#include <ply-list.h>
#include <ply-logger.h>
#include <ply-pixel-buffer.h>
#include <ply-pixel-display.h>
#include <ply-region.h>
#include <ply-trigger.h>
#include <ply-utils.h>

#include "fl.h"

#define DAWN_END 7.0            /* film seconds: the dawn is done, the pull-back would start */
#define LAND_END 13.4           /* film seconds: EXIT has finished */
#define ARRIVE_START 12.4       /* the shader's limb starts giving way to the real ring */
#define BREATH_PERIOD 6.5       /* seconds, one swell of the limb */

#define FPS_DAWN 30.0
#define FPS_HOLD 20.0           /* nothing moves fast here, leave the CPU to the boot */
#define FPS_LAND 60.0

#define THREADS 4

typedef enum {
        PHASE_NONE,             /* not animating */
        PHASE_FILM,             /* dawn, then hold */
        PHASE_LAND,
        PHASE_DONE,             /* the last frame is up */
} phase_t;

typedef struct {
        ply_boot_splash_plugin_t *plugin;
        ply_pixel_display_t      *display;
        fl_view_t                *fl;
        int                       width, height;        /* physical pixels */
        bool                      painted;              /* the first frame paints it all */
} view_t;

struct _ply_boot_splash_plugin {
        ply_event_loop_t *loop;
        ply_boot_splash_mode_t mode;
        ply_list_t *views;
        ply_trigger_t *idle_trigger;

        phase_t phase;
        bool timer_armed;
        double started;         /* first frame */
        double land_started;
        double land_speed;      /* film seconds per second */
        double land_dawn;       /* the dawn's clock when the landing began */
        double land_breath;

        fl_frame_t frame;       /* what on_draw paints */

        /* for the line in the kernel log */
        int frames;
        double frame_last, gap_worst;
        double draw_total, flush_total;
};

ply_boot_splash_plugin_interface_t *ply_boot_splash_plugin_get_interface (void);

static double
smooth (double a, double b, double x)
{
        double t = (x - a) / (b - a);
        t = t < 0. ? 0. : t > 1. ? 1. : t;
        return t * t * (3. - 2. * t);
}

/* Measurements and complaints, to read after a real boot: /run survives the switch
 * from the initrd. Not the kernel log: it silently drops all but the first of these. */
static void
say (const char *format, ...)
{
        va_list args;
        FILE *log = fopen ("/run/first-light.log", "ae");
        if (log == NULL)
                return;
        va_start (args, format);
        vfprintf (log, format, args);
        va_end (args);
        fputc ('\n', log);
        fclose (log);
}

static void
on_draw (view_t             *view,
         ply_pixel_buffer_t *buffer,
         int                 x,
         int                 y,
         int                 width,
         int                 height,
         ply_pixel_display_t *display)
{
        ply_boot_splash_plugin_t *plugin = view->plugin;
        ply_region_t *updated = ply_pixel_buffer_get_updated_areas (buffer);
        ply_rectangle_t all = { 0, 0, view->width, view->height };
        uint32_t *pixels = ply_pixel_buffer_get_argb32_data (buffer);
        double t0 = ply_get_timestamp ();

        if (view->fl == NULL || plugin->phase == PHASE_NONE) {
                /* Black, whatever else went wrong. The rectangle is in device pixels. */
                if (!view->painted) {
                        ply_pixel_buffer_fill_with_hex_color (buffer, NULL, 0x000000);
                        view->painted = true;
                }
                return;
        }

        fl_rect_t damage = { 0, 0, 0, 0 };
        fl_draw (view->fl, &plugin->frame, pixels, view->width, &damage);

        if (!view->painted) {
                ply_region_add_rectangle (updated, &all);
                view->painted = true;
        } else if (damage.x1 > damage.x0 && damage.y1 > damage.y0) {
                ply_rectangle_t r = { damage.x0, damage.y0, damage.x1 - damage.x0, damage.y1 - damage.y0 };
                ply_region_add_rectangle (updated, &r);
        }
        plugin->draw_total += ply_get_timestamp () - t0;
}

static void
view_free (view_t *view)
{
        ply_pixel_display_set_draw_handler (view->display, NULL, NULL);
        fl_view_free (view->fl);
        free (view);
}

static void
view_setup (view_t *view)
{
        ply_pixel_buffer_t *buffer;
        ply_renderer_t *renderer = ply_pixel_display_get_renderer (view->display);
        ply_renderer_head_t *head = ply_pixel_display_get_renderer_head (view->display);
        int scale = ply_pixel_display_get_device_scale (view->display);

        buffer = ply_renderer_get_buffer_for_head (renderer, head);
        view->width = ply_pixel_display_get_width (view->display) * scale;
        view->height = ply_pixel_display_get_height (view->display) * scale;

        /* A panel mounted sideways needs its buffer rotated, which only the slow path
         * does. Not worth it for a boot splash: it stays black. */
        if (ply_pixel_buffer_get_device_rotation (buffer) != PLY_PIXEL_BUFFER_ROTATE_UPRIGHT) {
                say ("rotated output %dx%d stays black", view->width, view->height);
                return;
        }
        fl_ring_t ring = fl_ring_for_output (view->width, view->height);
        view->fl = fl_view_new (view->width, view->height, &ring);
        if (view->fl == NULL) {
                say ("no renderer for %dx%d, staying black", view->width, view->height);
                return;
        }
        long cpus = sysconf (_SC_NPROCESSORS_ONLN);
        fl_view_set_threads (view->fl, cpus >= 2 * THREADS ? THREADS : cpus > 1 ? cpus / 2 : 1);
}

/* Plymouth holds displays paused and flushes them off a 60 Hz timer of its own, which
 * beats against ours and stretches by however long each flush takes. So flush here,
 * right after drawing: unpausing and pausing again leaves its count as it was. */
static void
draw_all (ply_boot_splash_plugin_t *plugin)
{
        ply_list_node_t *node;
        for (node = ply_list_get_first_node (plugin->views); node != NULL;
             node = ply_list_get_next_node (plugin->views, node)) {
                view_t *view = ply_list_node_get_data (node);
                ply_pixel_display_draw_area (view->display, 0, 0, view->width, view->height);
                double t0 = ply_get_timestamp ();
                ply_pixel_display_unpause_updates (view->display);
                ply_pixel_display_pause_updates (view->display);
                plugin->flush_total += ply_get_timestamp () - t0;
        }
}

static void
views_stop_threads (ply_boot_splash_plugin_t *plugin)
{
        ply_list_node_t *node;
        for (node = ply_list_get_first_node (plugin->views); node != NULL;
             node = ply_list_get_next_node (plugin->views, node)) {
                view_t *view = ply_list_node_get_data (node);
                if (view->fl != NULL)
                        fl_view_set_threads (view->fl, 1);
        }
}

/* Where the film is at this moment. Only ever a function of the clock: a slow frame
 * costs a frame, never the timing. */
static void
compute_frame (ply_boot_splash_plugin_t *plugin, double now)
{
        fl_frame_t *f = &plugin->frame;
        double t = now - plugin->started;

        f->arrive = 0.;
        f->seed = 1;    /* one dither pattern throughout: at this rate a moving one boils */

        if (plugin->phase == PHASE_LAND) {
                double u = now - plugin->land_started;
                f->T = DAWN_END + u * plugin->land_speed;
                if (f->T > LAND_END)
                        f->T = LAND_END;
                /* A boot faster than the dawn: the dawn hurries up under the pull-back. */
                f->dawn = plugin->land_dawn + (DAWN_END - plugin->land_dawn) * smooth (0., 1.2, u);
                f->breath = plugin->land_breath * (1. - smooth (0., .8, u));
                f->arrive = smooth (ARRIVE_START, LAND_END, f->T);
        } else if (t < DAWN_END) {
                f->T = f->dawn = t;
                f->breath = 0.;
        } else {
                f->T = f->dawn = DAWN_END;
                f->breath = .5 - .5 * cos ((t - DAWN_END) / BREATH_PERIOD * 2. * M_PI);
        }
}

static void on_timeout (ply_boot_splash_plugin_t *plugin);

static void
land_finished (ply_boot_splash_plugin_t *plugin)
{
        double took = ply_get_timestamp () - plugin->land_started;
        if (plugin->frames > 0)
                say ("landed in %.2f s: %d frames, %.1f fps, worst gap %.1f ms, draw %.1f ms, flush %.1f ms",
                     took, plugin->frames, plugin->frames / took, plugin->gap_worst * 1e3,
                     plugin->draw_total / plugin->frames * 1e3,
                     plugin->flush_total / plugin->frames * 1e3);
        views_stop_threads (plugin);
        plugin->phase = PHASE_DONE;
        /* nimbus-splash-land is waiting for this. */
        FILE *landed = fopen ("/run/first-light.landed", "we");
        if (landed != NULL)
                fclose (landed);
        if (plugin->idle_trigger != NULL) {
                ply_trigger_t *trigger = plugin->idle_trigger;
                plugin->idle_trigger = NULL;
                ply_trigger_pull (trigger, NULL);
        }
}

static void
arm_timer (ply_boot_splash_plugin_t *plugin, double seconds)
{
        if (plugin->timer_armed)
                return;
        plugin->timer_armed = true;
        ply_event_loop_watch_for_timeout (plugin->loop, seconds < .001 ? .001 : seconds,
                                          (ply_event_loop_timeout_handler_t) on_timeout, plugin);
}

static void
on_timeout (ply_boot_splash_plugin_t *plugin)
{
        double start = ply_get_timestamp ();
        plugin->timer_armed = false;

        if (plugin->phase != PHASE_FILM && plugin->phase != PHASE_LAND)
                return;

        if (plugin->started <= 0.)
                plugin->started = start;
        compute_frame (plugin, start);

        bool landing = plugin->phase == PHASE_LAND;
        bool last = landing && plugin->frame.T >= LAND_END;

        draw_all (plugin);

        double end = ply_get_timestamp ();
        if (landing) {
                plugin->frames++;
                if (plugin->frame_last > 0. && start - plugin->frame_last > plugin->gap_worst)
                        plugin->gap_worst = start - plugin->frame_last;
                plugin->frame_last = start;
        }

        if (last) {
                land_finished (plugin);
                return;
        }
        double fps = landing ? FPS_LAND : plugin->frame.T < DAWN_END ? FPS_DAWN : FPS_HOLD;
        arm_timer (plugin, 1. / fps - (end - start));
}

static void
start_landing (ply_boot_splash_plugin_t *plugin)
{
        double now = ply_get_timestamp ();

        if (plugin->phase != PHASE_FILM)
                return;

        compute_frame (plugin, now);
        plugin->land_dawn = plugin->frame.dawn;
        plugin->land_breath = plugin->frame.breath;
        plugin->land_started = now;
        plugin->phase = PHASE_LAND;
        plugin->frames = 0;
        plugin->frame_last = plugin->gap_worst = 0.;
        plugin->draw_total = plugin->flush_total = 0.;
        if (now - plugin->started > 0.)
                say ("landing after %.2f s of film", now - plugin->started);
}

static ply_boot_splash_plugin_t *
create_plugin (ply_key_file_t *key_file)
{
        ply_boot_splash_plugin_t *plugin = calloc (1, sizeof *plugin);
        double seconds = 0.;

        if (key_file != NULL) {
                char *value = ply_key_file_get_value (key_file, "first-light", "LandSeconds");
                if (value != NULL) {
                        seconds = atof (value);
                        free (value);
                }
        }
        if (!(seconds >= .5 && seconds <= 20.))
                seconds = 3.2;
        plugin->land_speed = (LAND_END - DAWN_END) / seconds;
        plugin->views = ply_list_new ();
        return plugin;
}

static void
stop_animation (ply_boot_splash_plugin_t *plugin)
{
        if (plugin->loop != NULL && plugin->timer_armed)
                ply_event_loop_stop_watching_for_timeout (plugin->loop,
                                                          (ply_event_loop_timeout_handler_t) on_timeout, plugin);
        plugin->timer_armed = false;
        plugin->phase = PHASE_NONE;
}

static void
on_loop_exit (ply_boot_splash_plugin_t *plugin)
{
        plugin->loop = NULL;
}

static void
destroy_plugin (ply_boot_splash_plugin_t *plugin)
{
        ply_list_node_t *node;

        if (plugin == NULL)
                return;
        stop_animation (plugin);
        if (plugin->loop != NULL)
                ply_event_loop_stop_watching_for_exit (plugin->loop,
                                                       (ply_event_loop_exit_handler_t) on_loop_exit, plugin);
        for (node = ply_list_get_first_node (plugin->views); node != NULL;
             node = ply_list_get_next_node (plugin->views, node))
                view_free (ply_list_node_get_data (node));
        ply_list_free (plugin->views);
        free (plugin);
}

static void
add_pixel_display (ply_boot_splash_plugin_t *plugin, ply_pixel_display_t *display)
{
        view_t *view = calloc (1, sizeof *view);

        view->plugin = plugin;
        view->display = display;
        view_setup (view);
        ply_list_append_data (plugin->views, view);
        ply_pixel_display_set_draw_handler (display, (ply_pixel_display_draw_handler_t) on_draw, view);
}

static void
remove_pixel_display (ply_boot_splash_plugin_t *plugin, ply_pixel_display_t *display)
{
        ply_list_node_t *node;

        for (node = ply_list_get_first_node (plugin->views); node != NULL;
             node = ply_list_get_next_node (plugin->views, node)) {
                view_t *view = ply_list_node_get_data (node);
                if (view->display == display) {
                        ply_list_remove_node (plugin->views, node);
                        view_free (view);
                        return;
                }
        }
}

static bool
show_splash_screen (ply_boot_splash_plugin_t *plugin,
                    ply_event_loop_t         *loop,
                    ply_buffer_t             *boot_buffer,
                    ply_boot_splash_mode_t    mode)
{
        plugin->loop = loop;
        plugin->mode = mode;
        ply_event_loop_watch_for_exit (loop, (ply_event_loop_exit_handler_t) on_loop_exit, plugin);

        /* The film is for boot. Everything else (shutdown, updates) gets plain black. */
        if (mode == PLY_BOOT_SPLASH_MODE_BOOT_UP) {
                plugin->phase = PHASE_FILM;
                arm_timer (plugin, .001);
        } else {
                views_stop_threads (plugin);
                draw_all (plugin);
        }
        return true;
}

static void
update_status (ply_boot_splash_plugin_t *plugin, const char *status)
{
        if (status != NULL && strcmp (status, "land") == 0)
                start_landing (plugin);
}

static void
hide_splash_screen (ply_boot_splash_plugin_t *plugin, ply_event_loop_t *loop)
{
        stop_animation (plugin);
        if (plugin->loop != NULL) {
                ply_event_loop_stop_watching_for_exit (plugin->loop,
                                                       (ply_event_loop_exit_handler_t) on_loop_exit, plugin);
                plugin->loop = NULL;
        }
}

static void
become_idle (ply_boot_splash_plugin_t *plugin, ply_trigger_t *idle_trigger)
{
        if (plugin->phase == PHASE_FILM)
                start_landing (plugin);

        if (plugin->phase == PHASE_LAND && ply_list_get_length (plugin->views) > 0) {
                /* Answered by land_finished. */
                plugin->idle_trigger = idle_trigger;
                return;
        }
        ply_trigger_pull (idle_trigger, NULL);
}

static void display_normal (ply_boot_splash_plugin_t *plugin) { }
static void display_message (ply_boot_splash_plugin_t *plugin, const char *message) { }
static void hide_message (ply_boot_splash_plugin_t *plugin, const char *message) { }
static void display_password (ply_boot_splash_plugin_t *plugin, const char *prompt, int bullets) { }
static void display_question (ply_boot_splash_plugin_t *plugin, const char *prompt, const char *entry_text) { }
static void display_prompt (ply_boot_splash_plugin_t *plugin, const char *prompt, const char *entry_text, bool is_secret) { }

ply_boot_splash_plugin_interface_t *
ply_boot_splash_plugin_get_interface (void)
{
        static ply_boot_splash_plugin_interface_t interface = {
                .create_plugin        = create_plugin,
                .destroy_plugin       = destroy_plugin,
                .add_pixel_display    = add_pixel_display,
                .remove_pixel_display = remove_pixel_display,
                .show_splash_screen   = show_splash_screen,
                .update_status        = update_status,
                .hide_splash_screen   = hide_splash_screen,
                .become_idle          = become_idle,
                .display_normal       = display_normal,
                .display_message      = display_message,
                .hide_message         = hide_message,
                .display_password     = display_password,
                .display_question     = display_question,
                .display_prompt       = display_prompt,
        };

        return &interface;
}
