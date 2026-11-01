/* first-light-vt-mode text|graphics: sets the foreground VT's mode.
 *
 * The VT stays in graphics mode through the hand-over. If what comes up after all is a
 * text greeter, it has to be put back, or that greeter draws on a console nobody sees. */
#include <fcntl.h>
#include <linux/kd.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

int main (int argc, char **argv)
{
        if (argc != 2 || (strcmp (argv[1], "text") != 0 && strcmp (argv[1], "graphics") != 0)) {
                fprintf (stderr, "usage: first-light-vt-mode text|graphics\n");
                return 2;
        }
        int fd = open ("/dev/tty0", O_RDWR | O_NOCTTY | O_CLOEXEC);
        if (fd < 0 || ioctl (fd, KDSETMODE, argv[1][0] == 't' ? KD_TEXT : KD_GRAPHICS) < 0) {
                perror ("first-light-vt-mode");
                return 1;
        }
        close (fd);
        return 0;
}
