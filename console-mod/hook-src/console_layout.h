/* Retail package selection, once at startup. No allocation or per-frame work. */
#ifndef CHRONOS_CONSOLE_LAYOUT_H
#define CHRONOS_CONSOLE_LAYOUT_H
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>

struct console_layout {
    const char *directory;
    const char *live_directory;
    const char *motion_format;
};
static const struct console_layout CONSOLE_JP = {
    "040", "/usr/game/040", "title_jp_titleselect_%s.psb.m"
};
static const struct console_layout CONSOLE_WW = {
    "041", "/usr/game/041", "title_us_titleselect_%s.psb.m"
};

static int console_layout_complete(const char *root, const struct console_layout *layout)
{
    const char *files[] = {"config/title_prof.psb.m", "config/title_mode_top.psb.m"};
    char path[512], motion[80];
    struct stat st;
    for (int i = 0; i < 4; i++) {
        if (i >= 2) snprintf(motion, sizeof(motion), layout->motion_format, i == 2 ? "jp" : "us");
        int n = snprintf(path, sizeof(path), "%s/%s/%s%s", root, layout->directory,
                         i >= 2 ? "motion/" : "", i >= 2 ? motion : files[i]);
        if (n < 0 || (size_t)n >= sizeof(path) || stat(path, &st) || !S_ISREG(st.st_mode)) return 0;
    }
    return 1;
}

static const struct console_layout *console_layout_detect(const char *root)
{
    char path[512], version[32] = {0};
    int n = snprintf(path, sizeof(path), "%s/version", root);
    if (n < 0 || (size_t)n >= sizeof(path)) return NULL;
    FILE *file = fopen(path, "r");
    if (file) {
        int read = fscanf(file, "%31s", version);
        fclose(file);
        const struct console_layout *layout = NULL;
        if (read == 1 && !strcmp(version, "1006JP")) layout = &CONSOLE_JP;
        if (read == 1 && !strcmp(version, "1006WW")) layout = &CONSOLE_WW;
        return layout && console_layout_complete(root, layout) ? layout : NULL;
    }
    /* Legacy extracted trees may omit version; never guess if both exist. */
    int jp = console_layout_complete(root, &CONSOLE_JP);
    int ww = console_layout_complete(root, &CONSOLE_WW);
    return jp != ww ? (jp ? &CONSOLE_JP : &CONSOLE_WW) : NULL;
}
#endif
