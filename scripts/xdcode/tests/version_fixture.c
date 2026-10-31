/* Small native packaging fixture. This is not the xdcode runtime. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    const char *name = strrchr(argv[0], '/');
    name = name ? name + 1 : argv[0];
    if (argc == 2 && strcmp(argv[1], "--hang") == 0) {
        for (;;) pause();
    }
    if (argc == 2 && strcmp(argv[1], "--flood") == 0) {
        for (;;) puts("probe output limit fixture");
    }
    if (argc == 2 && strcmp(argv[1], "--fail") == 0) {
        fputs("private diagnostic must not be forwarded", stderr);
        return 7;
    }
    if (argc == 2 && strcmp(argv[1], "--environment") == 0) {
        puts(getenv("XDCODE_API_KEY") || getenv("LD_LIBRARY_PATH") || getenv("HTTP_PROXY")
             ? "inherited environment" : "clean");
        return 0;
    }
    if (strcmp(name, "bwrap") == 0) {
        puts("bubblewrap built for Xdcode");
        return 0;
    }
    if (strcmp(name, "rg") == 0) {
        puts("ripgrep 14.1.1");
        return 0;
    }
    if (strcmp(name, "xdcode-code-mode-host") == 0) {
        puts("Usage: xdcode-code-mode-host [OPTIONS]\n  --listen <URL>");
        return 0;
    }
    puts("xdcode 0.162.0");
    return 0;
}
