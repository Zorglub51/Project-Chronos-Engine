/*
 * gl_proxy.c - Native aarch64 GL proxy for m2engage
 *
 * Runs natively on the host, owns the EGL context (via GBM) and X11 window.
 * Uses GBM surfaceless EGL for GPU access (DRI3 unavailable on Xwayland),
 * with XShmPutImage for fast display.
 *
 * Build (native aarch64):
 *   gcc -O2 -o gl_proxy gl_proxy.c -lEGL -lGLESv2 -lX11 -lXext -lgbm -lpthread -lrt
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <errno.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>

#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <gbm.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/keysym.h>
#include <linux/input-event-codes.h>
#include "gl_protocol.h"
#include "frame_pacing.h"

static int g_debug = 0;
#define DBG(...) do { if (g_debug) fprintf(stderr, "[gl_proxy] " __VA_ARGS__); } while(0)
#define ERR(...) fprintf(stderr, "[gl_proxy] ERROR: " __VA_ARGS__)

#define RENDER_WIDTH 1280
#define RENDER_HEIGHT 720

/* GBM / EGL state */
static int drm_fd = -1;
static struct gbm_device *gbm_dev = NULL;
static EGLDisplay egl_dpy = EGL_NO_DISPLAY;
static EGLContext egl_ctx = EGL_NO_CONTEXT;

/* Display FBO (replaces default framebuffer for surfaceless context) */
static GLuint display_fbo = 0;
static GLuint display_color_tex = 0;
static GLuint display_depth_rb = 0;
static int use_bgra_readback = 0;  /* GL_EXT_read_format_bgra */
#define GL_BGRA_EXT 0x80E1

/* X11 display state */
static Display *x_dpy = NULL;
static Window   x_win = 0;
static GC       x_gc = 0;
static XImage  *x_image = NULL;

/* Shared memory */
static struct gl_shm *shm = NULL;

/* FPS tracking & frame limiter */
static int swap_count = 0;
static struct timespec fps_start;
static int64_t frame_deadline_ns;

#define FRAME_BYTES (RENDER_WIDTH * RENDER_HEIGHT * 4)

/* Readback buffer */
static unsigned char readback_buf[RENDER_WIDTH * RENDER_HEIGHT * 4];
static volatile sig_atomic_t capture_requested;

static void request_capture(int sig) {
    (void)sig;
    capture_requested = 1;
}

/* Capture only the emulator image, never the rest of the user's desktop. */
static void capture_frame(void) {
    if (!capture_requested) return;
    capture_requested = 0;
    const char *path = getenv("CHRONOS_SCREENSHOT");
    if (!path || !*path) return;
    char *tmp = NULL;
    if (asprintf(&tmp, "%s.tmp", path) < 0) return;
    FILE *file = fopen(tmp, "wb");
    if (!file) { free(tmp); return; }
    int ok = fprintf(file, "P6\n%d %d\n255\n", RENDER_WIDTH, RENDER_HEIGHT) > 0;
    unsigned char row[RENDER_WIDTH * 3];
    for (int y = 0; ok && y < RENDER_HEIGHT; y++) {
        const unsigned char *src = readback_buf + (RENDER_HEIGHT - 1 - y) * RENDER_WIDTH * 4;
        for (int x = 0; x < RENDER_WIDTH; x++) {
            row[x * 3] = src[x * 4 + (use_bgra_readback ? 2 : 0)];
            row[x * 3 + 1] = src[x * 4 + 1];
            row[x * 3 + 2] = src[x * 4 + (use_bgra_readback ? 0 : 2)];
        }
        ok = fwrite(row, 1, sizeof(row), file) == sizeof(row);
    }
    if (fclose(file) != 0) ok = 0;
    if (ok) rename(tmp, path);
    else unlink(tmp);
    free(tmp);
}

/* ------------------------------------------------------------------ */
/* Input FIFOs - write armhf evdev events for m2engage                 */
/* ------------------------------------------------------------------ */

#define INPUT_FIFO_0 "/tmp/m2e_input0"
#define INPUT_FIFO_1 "/tmp/m2e_input1"

static int fifo_fds[2] = {-1, -1};

/* 16-byte armhf struct input_event (not native aarch64 which is 24 bytes) */
struct armhf_input_event {
    uint32_t tv_sec;
    uint32_t tv_usec;
    uint16_t type;
    uint16_t code;
    int32_t  value;
} __attribute__((packed));

static void init_input_fifos(void) {
    unlink(INPUT_FIFO_0);
    unlink(INPUT_FIFO_1);
    if (mkfifo(INPUT_FIFO_0, 0666) < 0) { ERR("mkfifo(%s): %s\n", INPUT_FIFO_0, strerror(errno)); }
    if (mkfifo(INPUT_FIFO_1, 0666) < 0) { ERR("mkfifo(%s): %s\n", INPUT_FIFO_1, strerror(errno)); }
    /* Open O_RDWR so open never blocks and writes never get ENXIO */
    fifo_fds[0] = open(INPUT_FIFO_0, O_RDWR | O_NONBLOCK);
    fifo_fds[1] = open(INPUT_FIFO_1, O_RDWR | O_NONBLOCK);
    if (fifo_fds[0] < 0 || fifo_fds[1] < 0)
        ERR("Failed to open input FIFOs\n");
}

static void emit_input(uint16_t type, uint16_t code, int32_t val) {
    struct armhf_input_event ev = {0, 0, type, code, val};
    for (int i = 0; i < 2; i++) {
        if (fifo_fds[i] >= 0)
            write(fifo_fds[i], &ev, sizeof(ev));
    }
}

static void handle_x11_key(XKeyEvent *xkey, int pressed) {
    KeySym ks = XLookupKeysym(xkey, 0);
    switch (ks) {
    case XK_Up:
        emit_input(EV_ABS, ABS_HAT0Y, pressed ? -1 : 0);
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    case XK_Down:
        emit_input(EV_ABS, ABS_HAT0Y, pressed ? 1 : 0);
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    case XK_Left:
        emit_input(EV_ABS, ABS_HAT0X, pressed ? -1 : 0);
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    case XK_Right:
        emit_input(EV_ABS, ABS_HAT0X, pressed ? 1 : 0);
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    case XK_z: case XK_Z:
        emit_input(EV_KEY, BTN_C, pressed);       /* Button I (0x132) */
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    case XK_x: case XK_X:
        emit_input(EV_KEY, BTN_EAST, pressed);    /* Button II (0x131) */
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    case XK_Return:
        emit_input(EV_KEY, BTN_TR2, pressed);     /* RUN (0x139) */
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    case XK_Shift_R:
        emit_input(EV_KEY, BTN_TL2, pressed);     /* SELECT (0x138) */
        emit_input(EV_SYN, SYN_REPORT, 0);
        break;
    }
}

/* ------------------------------------------------------------------ */
/* GBM + EGL initialization (surfaceless - GPU rendering)              */
/* ------------------------------------------------------------------ */

static int init_gbm_egl(void) {
    drm_fd = open("/dev/dri/renderD128", O_RDWR);
    if (drm_fd < 0) { ERR("Cannot open /dev/dri/renderD128: %s\n", strerror(errno)); return 0; }

    gbm_dev = gbm_create_device(drm_fd);
    if (!gbm_dev) { ERR("gbm_create_device failed\n"); close(drm_fd); drm_fd = -1; return 0; }

    PFNEGLGETPLATFORMDISPLAYEXTPROC eglGetPlatformDisplayEXT =
        (PFNEGLGETPLATFORMDISPLAYEXTPROC)eglGetProcAddress("eglGetPlatformDisplayEXT");
    if (!eglGetPlatformDisplayEXT) { ERR("eglGetPlatformDisplayEXT not available\n"); return 0; }

    egl_dpy = eglGetPlatformDisplayEXT(EGL_PLATFORM_GBM_MESA, gbm_dev, NULL);
    if (egl_dpy == EGL_NO_DISPLAY) { ERR("eglGetPlatformDisplayEXT failed\n"); return 0; }

    EGLint major, minor;
    if (!eglInitialize(egl_dpy, &major, &minor)) { ERR("eglInitialize failed\n"); return 0; }

    eglBindAPI(EGL_OPENGL_ES_API);

    EGLint cfg_attr[] = { EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE };
    EGLConfig cfg;
    EGLint n;
    if (!eglChooseConfig(egl_dpy, cfg_attr, &cfg, 1, &n) || n == 0) {
        ERR("eglChooseConfig failed\n"); return 0;
    }

    EGLint ctx_attr[] = { EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE };
    egl_ctx = eglCreateContext(egl_dpy, cfg, EGL_NO_CONTEXT, ctx_attr);
    if (egl_ctx == EGL_NO_CONTEXT) { ERR("eglCreateContext failed\n"); return 0; }

    if (!eglMakeCurrent(egl_dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, egl_ctx)) {
        ERR("eglMakeCurrent (surfaceless) failed: 0x%x\n", eglGetError()); return 0;
    }

    const char *renderer = (const char *)glGetString(GL_RENDERER);
    const char *version = (const char *)glGetString(GL_VERSION);
    const char *extensions = (const char *)glGetString(GL_EXTENSIONS);
    fprintf(stderr, "[gl_proxy] GL_RENDERER: %s\n", renderer ? renderer : "NULL");
    fprintf(stderr, "[gl_proxy] GL_VERSION: %s\n", version ? version : "NULL");
    if (extensions && strstr(extensions, "GL_EXT_read_format_bgra")) {
        use_bgra_readback = 1;
        fprintf(stderr, "[gl_proxy] BGRA readback available (fast path)\n");
    }

    /* Display FBO (replaces default framebuffer for surfaceless) */
    glGenTextures(1, &display_color_tex);
    glBindTexture(GL_TEXTURE_2D, display_color_tex);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, RENDER_WIDTH, RENDER_HEIGHT, 0,
                 GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glBindTexture(GL_TEXTURE_2D, 0);

    glGenRenderbuffers(1, &display_depth_rb);
    glBindRenderbuffer(GL_RENDERBUFFER, display_depth_rb);
    glRenderbufferStorage(GL_RENDERBUFFER, GL_DEPTH_COMPONENT16, RENDER_WIDTH, RENDER_HEIGHT);
    glBindRenderbuffer(GL_RENDERBUFFER, 0);

    glGenFramebuffers(1, &display_fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, display_fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, display_color_tex, 0);
    glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_DEPTH_ATTACHMENT, GL_RENDERBUFFER, display_depth_rb);

    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        ERR("Display FBO incomplete\n"); return 0;
    }
    DBG("Display FBO %u created (%dx%d)\n", display_fbo, RENDER_WIDTH, RENDER_HEIGHT);

    return 1;
}

/* ------------------------------------------------------------------ */
/* X11 window + XShm initialization (display + keyboard only)          */
/* ------------------------------------------------------------------ */

static int init_x11_display(void) {
    x_dpy = XOpenDisplay(NULL);
    if (!x_dpy) { ERR("Cannot open X11 display\n"); return 0; }

    int screen = DefaultScreen(x_dpy);
    Window root = RootWindow(x_dpy, screen);

    XSetWindowAttributes swa;
    swa.event_mask = ExposureMask | KeyPressMask | KeyReleaseMask | StructureNotifyMask;
    swa.background_pixel = BlackPixel(x_dpy, screen);

    x_win = XCreateWindow(x_dpy, root, 0, 0, RENDER_WIDTH, RENDER_HEIGHT, 0,
                           CopyFromParent, InputOutput, CopyFromParent,
                           CWBackPixel | CWEventMask, &swa);
    if (!x_win) { ERR("Cannot create X11 window\n"); return 0; }

    XStoreName(x_dpy, x_win, "m2engage (GL proxy)");
    XSizeHints *hints = XAllocSizeHints();
    if (hints) {
        hints->flags = PMinSize | PMaxSize;
        hints->min_width = hints->max_width = RENDER_WIDTH;
        hints->min_height = hints->max_height = RENDER_HEIGHT;
        XSetWMNormalHints(x_dpy, x_win, hints);
        XFree(hints);
    }
    XMapWindow(x_dpy, x_win);
    XFlush(x_dpy);

    x_gc = XCreateGC(x_dpy, x_win, 0, NULL);

    /* XImage for pixel blit */
    {
        Visual *vis = DefaultVisual(x_dpy, screen);
        int depth = DefaultDepth(x_dpy, screen);
        unsigned char *pixels = calloc(RENDER_WIDTH * RENDER_HEIGHT, 4);
        if (!pixels) { ERR("Failed to allocate pixel buffer\n"); return 0; }
        x_image = XCreateImage(x_dpy, vis, depth, ZPixmap, 0,
                                (char *)pixels, RENDER_WIDTH, RENDER_HEIGHT, 32, 0);
        if (!x_image) { ERR("XCreateImage failed\n"); free(pixels); return 0; }
    }

    return 1;
}

/* ------------------------------------------------------------------ */
/* Shared memory + eventfd setup                                       */
/* ------------------------------------------------------------------ */

static int init_shm(void) {
    /* Remove stale shm if exists */
    shm_unlink(GL_SHM_NAME);

    int fd = shm_open(GL_SHM_NAME, O_CREAT | O_RDWR, 0666);
    if (fd < 0) { ERR("shm_open: %s\n", strerror(errno)); return 0; }

    if (ftruncate(fd, GL_SHM_TOTAL_SIZE) < 0) {
        ERR("ftruncate: %s\n", strerror(errno));
        close(fd);
        return 0;
    }

    shm = mmap(NULL, GL_SHM_TOTAL_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    if (shm == MAP_FAILED) { ERR("mmap: %s\n", strerror(errno)); shm = NULL; return 0; }

    memset((void*)shm, 0, GL_SHM_TOTAL_SIZE);

    DBG("SHM mapped at %p, size=%zu MB\n", shm, GL_SHM_TOTAL_SIZE / (1024*1024));

    return 1;
}

/* ------------------------------------------------------------------ */
/* Helpers: read arguments from ring                                   */
/* ------------------------------------------------------------------ */

/* Ring reader state - tracks current position during command dispatch */
static uint32_t rd_pos;

static inline uint32_t read_u32(void) {
    uint32_t v;
    ring_read(shm, rd_pos, &v, 4);
    rd_pos += 4;
    return v;
}

static inline float read_f32(void) {
    union { uint32_t u; float f; } conv;
    conv.u = read_u32();
    return conv.f;
}

static inline void read_data(void *dst, uint32_t len) {
    ring_read(shm, rd_pos, dst, len);
    rd_pos += align4(len);
}

/* Set response and signal via futex */
static inline void respond_u32(uint32_t val) {
    shm->response_u32 = val;
    shm->response_size = 0;
    __atomic_store_n(&shm->ack_seq, shm->cmd_seq, __ATOMIC_RELEASE);
    futex_wake(&shm->ack_seq);
}

static inline void respond_data(uint32_t scalar, const void *data, uint32_t size) {
    shm->response_u32 = scalar;
    if (size > RESPONSE_SIZE) size = RESPONSE_SIZE;
    shm->response_size = size;
    memcpy((void*)shm->response_data, data, size);
    __atomic_store_n(&shm->ack_seq, shm->cmd_seq, __ATOMIC_RELEASE);
    futex_wake(&shm->ack_seq);
}

/* ------------------------------------------------------------------ */
/* Command dispatch                                                    */
/* ------------------------------------------------------------------ */

/* Temp buffers for data transfers */
static uint8_t tmp_buf[8 * 1024 * 1024];  /* 8MB for large texture uploads */
static char str_buf[65536];                /* for shader source etc. */
static GLuint id_buf[256];                 /* for gen/delete calls */
static GLfloat float_buf[1024];            /* for uniforms, matrices */
static GLint int_buf[256];                 /* for getIntegerv etc. */

static const char* cmd_name(uint16_t id) {
    switch (id) {
    case GL_CMD_SWAP_BUFFERS: return "SwapBuffers";
    case GL_CMD_VIEWPORT: return "Viewport";
    case GL_CMD_CLEAR_COLOR: return "ClearColor";
    case GL_CMD_CLEAR: return "Clear";
    case GL_CMD_ENABLE: return "Enable";
    case GL_CMD_DISABLE: return "Disable";
    case GL_CMD_BIND_TEXTURE: return "BindTexture";
    case GL_CMD_GEN_TEXTURES: return "GenTextures";
    case GL_CMD_TEX_IMAGE_2D: return "TexImage2D";
    case GL_CMD_TEX_SUB_IMAGE_2D: return "TexSubImage2D";
    case GL_CMD_TEX_PARAMETERI: return "TexParameteri";
    case GL_CMD_ACTIVE_TEXTURE: return "ActiveTexture";
    case GL_CMD_BIND_FRAMEBUFFER: return "BindFramebuffer";
    case GL_CMD_GEN_FRAMEBUFFERS: return "GenFramebuffers";
    case GL_CMD_FRAMEBUFFER_TEXTURE_2D: return "FramebufferTexture2D";
    case GL_CMD_CHECK_FRAMEBUFFER_STATUS: return "CheckFramebufferStatus";
    case GL_CMD_DRAW_ARRAYS: return "DrawArrays";
    case GL_CMD_DRAW_ELEMENTS: return "DrawElements";
    case GL_CMD_CREATE_SHADER: return "CreateShader";
    case GL_CMD_SHADER_SOURCE: return "ShaderSource";
    case GL_CMD_COMPILE_SHADER: return "CompileShader";
    case GL_CMD_CREATE_PROGRAM: return "CreateProgram";
    case GL_CMD_ATTACH_SHADER: return "AttachShader";
    case GL_CMD_LINK_PROGRAM: return "LinkProgram";
    case GL_CMD_USE_PROGRAM: return "UseProgram";
    case GL_CMD_GET_UNIFORM_LOCATION: return "GetUniformLocation";
    case GL_CMD_GET_ATTRIB_LOCATION: return "GetAttribLocation";
    case GL_CMD_UNIFORM_1I: return "Uniform1i";
    case GL_CMD_UNIFORM_1F: return "Uniform1f";
    case GL_CMD_UNIFORM_MATRIX_4FV: return "UniformMatrix4fv";
    case GL_CMD_VERTEX_ATTRIB_POINTER: return "VertexAttribPointer";
    case GL_CMD_ENABLE_VERTEX_ATTRIB_ARRAY: return "EnableVertexAttribArray";
    case GL_CMD_BIND_BUFFER: return "BindBuffer";
    case GL_CMD_BUFFER_DATA: return "BufferData";
    case GL_CMD_GET_STRING: return "GetString";
    case GL_CMD_GET_ERROR: return "GetError";
    case GL_CMD_GET_INTEGERV: return "GetIntegerv";
    case GL_CMD_BLEND_FUNC: return "BlendFunc";
    case GL_CMD_BLEND_FUNC_SEPARATE: return "BlendFuncSeparate";
    case GL_CMD_SCISSOR: return "Scissor";
    case GL_CMD_READ_PIXELS: return "ReadPixels";
    case GL_CMD_PIXEL_STOREI: return "PixelStorei";
    case GL_CMD_GET_SHADERIV: return "GetShaderiv";
    case GL_CMD_GET_PROGRAMIV: return "GetProgramiv";
    case GL_CMD_DELETE_TEXTURES: return "DeleteTextures";
    case GL_CMD_DELETE_PROGRAM: return "DeleteProgram";
    case GL_CMD_DELETE_SHADER: return "DeleteShader";
    default: return "???";
    }
}

static void dispatch_command(uint16_t cmd_id, uint16_t flags, uint32_t body_size) {
    (void)body_size;  /* We read args sequentially via read_u32/read_f32 */

    static int cmd_count = 0;
    cmd_count++;
    /* Log first 200 commands for debugging init, then only errors */
    if (g_debug && cmd_count <= 200 && cmd_id != GL_CMD_SWAP_BUFFERS) {
        fprintf(stderr, "[gl_proxy] cmd #%d: %s (id=%u)\n", cmd_count, cmd_name(cmd_id), cmd_id);
    }

    switch (cmd_id) {

    /* ---- EGL / Swap ---- */
    case GL_CMD_SWAP_BUFFERS: {
        /* Process X11 events (keyboard input) */
        while (XPending(x_dpy)) {
            XEvent ev;
            XNextEvent(x_dpy, &ev);
            if (ev.type == DestroyNotify) {
                fprintf(stderr, "[gl_proxy] Window destroyed, exiting\n");
                _exit(0);
            }
            if (ev.type == KeyPress)
                handle_x11_key(&ev.xkey, 1);
            else if (ev.type == KeyRelease)
                handle_x11_key(&ev.xkey, 0);
        }

        /* Readback display FBO and blit to X11 */
        glBindFramebuffer(GL_FRAMEBUFFER, display_fbo);
        {
            int stride = RENDER_WIDTH * 4;
            unsigned char *dst = (unsigned char *)x_image->data;
            if (use_bgra_readback) {
                glReadPixels(0, 0, RENDER_WIDTH, RENDER_HEIGHT,
                             GL_BGRA_EXT, GL_UNSIGNED_BYTE, readback_buf);
                for (int y = 0; y < RENDER_HEIGHT; y++)
                    memcpy(dst + y * stride,
                           readback_buf + (RENDER_HEIGHT - 1 - y) * stride, stride);
            } else {
                glReadPixels(0, 0, RENDER_WIDTH, RENDER_HEIGHT,
                             GL_RGBA, GL_UNSIGNED_BYTE, readback_buf);
                for (int y = 0; y < RENDER_HEIGHT; y++) {
                    uint32_t *src_row = (uint32_t *)(readback_buf + (RENDER_HEIGHT - 1 - y) * stride);
                    uint32_t *dst_row = (uint32_t *)(dst + y * stride);
                    for (int x = 0; x < RENDER_WIDTH; x++) {
                        uint32_t c = src_row[x];
                        dst_row[x] = (c & 0xFF00FF00) | ((c & 0xFF) << 16) | ((c >> 16) & 0xFF);
                    }
                }
            }
        }
        capture_frame();
        XPutImage(x_dpy, x_win, x_gc, x_image, 0, 0, 0, 0,
                  RENDER_WIDTH, RENDER_HEIGHT);
        XFlush(x_dpy);

        /* Keep absolute frame deadlines: scheduler oversleep must not lower
         * the emulated clock or starve audio on every following frame. Allow
         * at most one frame of catch-up after a stall. */
        {
            struct timespec now;
            clock_gettime(CLOCK_MONOTONIC, &now);
            int64_t now_ns = (int64_t)now.tv_sec * 1000000000 + now.tv_nsec;
            frame_deadline_ns = frame_next_deadline(frame_deadline_ns, now_ns);
            if (now_ns < frame_deadline_ns) {
                struct timespec target = {
                    .tv_sec = frame_deadline_ns / 1000000000,
                    .tv_nsec = frame_deadline_ns % 1000000000
                };
                while (clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &target, NULL) == EINTR) {}
            }
        }

        /* FPS tracking */
        swap_count++;
        if ((swap_count % 60) == 0) {
            struct timespec now;
            clock_gettime(CLOCK_MONOTONIC, &now);
            double elapsed = (now.tv_sec - fps_start.tv_sec) +
                             (now.tv_nsec - fps_start.tv_nsec) / 1e9;
            if (elapsed > 0) {
                DBG("FPS: %.1f (swap #%d)\n", 60.0 / elapsed, swap_count);
            }
            fps_start = now;
        }

        if (flags & CMD_FLAG_SYNC) respond_u32(1);
        break;
    }

    /* ---- State ---- */
    case GL_CMD_ENABLE:             glEnable(read_u32()); break;
    case GL_CMD_DISABLE:            glDisable(read_u32()); break;
    case GL_CMD_BLEND_FUNC:         { uint32_t s=read_u32(), d=read_u32(); glBlendFunc(s,d); break; }
    case GL_CMD_BLEND_FUNC_SEPARATE: { uint32_t sr=read_u32(),dr=read_u32(),sa=read_u32(),da=read_u32(); glBlendFuncSeparate(sr,dr,sa,da); break; }
    case GL_CMD_BLEND_EQUATION:     glBlendEquation(read_u32()); break;
    case GL_CMD_BLEND_EQUATION_SEPARATE: { uint32_t mr=read_u32(),ma=read_u32(); glBlendEquationSeparate(mr,ma); break; }
    case GL_CMD_DEPTH_FUNC:         glDepthFunc(read_u32()); break;
    case GL_CMD_DEPTH_MASK:         glDepthMask(read_u32()); break;
    case GL_CMD_DEPTH_RANGEF:       { float n=read_f32(),f=read_f32(); glDepthRangef(n,f); break; }
    case GL_CMD_COLOR_MASK:         { uint32_t r=read_u32(),g=read_u32(),b=read_u32(),a=read_u32(); glColorMask(r,g,b,a); break; }
    case GL_CMD_CULL_FACE:          glCullFace(read_u32()); break;
    case GL_CMD_FRONT_FACE:         glFrontFace(read_u32()); break;
    case GL_CMD_SCISSOR:            { int32_t x=(int32_t)read_u32(),y=(int32_t)read_u32(); uint32_t w=read_u32(),h=read_u32(); glScissor(x,y,w,h); break; }
    case GL_CMD_VIEWPORT:           { int32_t x=(int32_t)read_u32(),y=(int32_t)read_u32(); uint32_t w=read_u32(),h=read_u32(); glViewport(x,y,w,h); break; }
    case GL_CMD_CLEAR_COLOR:        { float r=read_f32(),g=read_f32(),b=read_f32(),a=read_f32(); glClearColor(r,g,b,a); break; }
    case GL_CMD_CLEAR:              glClear(read_u32()); break;
    case GL_CMD_CLEAR_DEPTHF:       glClearDepthf(read_f32()); break;
    case GL_CMD_CLEAR_STENCIL:      glClearStencil((int32_t)read_u32()); break;
    case GL_CMD_STENCIL_FUNC:       { uint32_t f=read_u32(); int32_t r=(int32_t)read_u32(); uint32_t m=read_u32(); glStencilFunc(f,r,m); break; }
    case GL_CMD_STENCIL_MASK:       glStencilMask(read_u32()); break;
    case GL_CMD_STENCIL_OP:         { uint32_t sf=read_u32(),df=read_u32(),dp=read_u32(); glStencilOp(sf,df,dp); break; }
    case GL_CMD_PIXEL_STOREI:       { uint32_t p=read_u32(); int32_t v=(int32_t)read_u32(); glPixelStorei(p,v); break; }
    case GL_CMD_ACTIVE_TEXTURE:     glActiveTexture(read_u32()); break;
    case GL_CMD_FLUSH:              glFlush(); break;
    case GL_CMD_FINISH:             glFinish(); if (flags & CMD_FLAG_SYNC) respond_u32(0); break;
    case GL_CMD_LINE_WIDTH:         glLineWidth(read_f32()); break;

    /* ---- Textures ---- */
    case GL_CMD_GEN_TEXTURES: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        glGenTextures(n, id_buf);
        respond_data(0, id_buf, n * 4);
        break;
    }
    case GL_CMD_DELETE_TEXTURES: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        for (uint32_t i = 0; i < n; i++) id_buf[i] = read_u32();
        glDeleteTextures(n, id_buf);
        break;
    }
    case GL_CMD_BIND_TEXTURE: {
        uint32_t target = read_u32(), tex = read_u32();
        glBindTexture(target, tex);
        break;
    }
    case GL_CMD_TEX_IMAGE_2D: {
        uint32_t target = read_u32();
        int32_t level = (int32_t)read_u32();
        int32_t ifmt = (int32_t)read_u32();
        int32_t w = (int32_t)read_u32();
        int32_t h = (int32_t)read_u32();
        int32_t border = (int32_t)read_u32();
        uint32_t fmt = read_u32();
        uint32_t type = read_u32();
        uint32_t has_data = read_u32();
        if (has_data) {
            uint32_t data_size = read_u32();
            if (data_size > sizeof(tmp_buf)) data_size = sizeof(tmp_buf);
            read_data(tmp_buf, data_size);
            glTexImage2D(target, level, ifmt, w, h, border, fmt, type, tmp_buf);
        } else {
            glTexImage2D(target, level, ifmt, w, h, border, fmt, type, NULL);
        }
        break;
    }
    case GL_CMD_TEX_SUB_IMAGE_2D: {
        uint32_t target = read_u32();
        int32_t level = (int32_t)read_u32();
        int32_t xoff = (int32_t)read_u32();
        int32_t yoff = (int32_t)read_u32();
        int32_t w = (int32_t)read_u32();
        int32_t h = (int32_t)read_u32();
        uint32_t fmt = read_u32();
        uint32_t type = read_u32();
        uint32_t data_size = read_u32();
        if (data_size > sizeof(tmp_buf)) data_size = sizeof(tmp_buf);
        read_data(tmp_buf, data_size);
        glTexSubImage2D(target, level, xoff, yoff, w, h, fmt, type, tmp_buf);
        break;
    }
    case GL_CMD_TEX_PARAMETERI: {
        uint32_t target = read_u32(), pname = read_u32();
        int32_t param = (int32_t)read_u32();
        glTexParameteri(target, pname, param);
        break;
    }
    case GL_CMD_GENERATE_MIPMAP:    glGenerateMipmap(read_u32()); break;
    case GL_CMD_COPY_TEX_IMAGE_2D: {
        uint32_t t=read_u32(); int32_t l=(int32_t)read_u32(); uint32_t ifmt=read_u32();
        int32_t x=(int32_t)read_u32(),y=(int32_t)read_u32(),w=(int32_t)read_u32(),h=(int32_t)read_u32();
        int32_t b=(int32_t)read_u32();
        glCopyTexImage2D(t,l,ifmt,x,y,w,h,b);
        break;
    }
    case GL_CMD_COPY_TEX_SUB_IMAGE_2D: {
        uint32_t t=read_u32(); int32_t l=(int32_t)read_u32();
        int32_t xo=(int32_t)read_u32(),yo=(int32_t)read_u32();
        int32_t x=(int32_t)read_u32(),y=(int32_t)read_u32(),w=(int32_t)read_u32(),h=(int32_t)read_u32();
        glCopyTexSubImage2D(t,l,xo,yo,x,y,w,h);
        break;
    }

    case GL_CMD_COMPRESSED_TEX_IMAGE_2D: {
        uint32_t target=read_u32(); int32_t level=(int32_t)read_u32(); uint32_t ifmt=read_u32();
        int32_t w=(int32_t)read_u32(),h=(int32_t)read_u32(),border=(int32_t)read_u32();
        int32_t imageSize=(int32_t)read_u32();
        void *data = NULL;
        if (imageSize > 0) {
            if ((uint32_t)imageSize <= sizeof(tmp_buf)) {
                read_data(tmp_buf, imageSize);
                data = tmp_buf;
            } else {
                data = malloc(imageSize);
                if (data) read_data(data, imageSize);
            }
        }
        glCompressedTexImage2D(target, level, ifmt, w, h, border, imageSize, data);
        if (data && data != (void*)tmp_buf) free(data);
        break;
    }

    /* ---- Framebuffers ---- */
    case GL_CMD_GEN_FRAMEBUFFERS: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        glGenFramebuffers(n, id_buf);
        respond_data(0, id_buf, n * 4);
        break;
    }
    case GL_CMD_DELETE_FRAMEBUFFERS: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        for (uint32_t i = 0; i < n; i++) id_buf[i] = read_u32();
        glDeleteFramebuffers(n, id_buf);
        break;
    }
    case GL_CMD_BIND_FRAMEBUFFER: {
        uint32_t target = read_u32(), fb = read_u32();
        glBindFramebuffer(target, fb == 0 ? display_fbo : fb);
        break;
    }
    case GL_CMD_FRAMEBUFFER_TEXTURE_2D: {
        uint32_t target=read_u32(), attach=read_u32(), textarget=read_u32(), tex=read_u32();
        int32_t level=(int32_t)read_u32();
        glFramebufferTexture2D(target, attach, textarget, tex, level);
        break;
    }
    case GL_CMD_CHECK_FRAMEBUFFER_STATUS: {
        uint32_t target = read_u32();
        GLenum status = glCheckFramebufferStatus(target);
        respond_u32(status);
        break;
    }
    case GL_CMD_READ_PIXELS: {
        int32_t x=(int32_t)read_u32(), y=(int32_t)read_u32();
        int32_t w=(int32_t)read_u32(), h=(int32_t)read_u32();
        uint32_t fmt=read_u32(), type=read_u32();
        uint32_t ps = pixel_size(fmt, type);
        uint32_t data_size = w * h * ps;
        if (data_size > RESPONSE_SIZE) data_size = RESPONSE_SIZE;
        glReadPixels(x, y, w, h, fmt, type, shm->response_data);
        shm->response_u32 = 0;
        shm->response_size = data_size;
        __atomic_store_n(&shm->ack_seq, shm->cmd_seq, __ATOMIC_RELEASE);
        futex_wake(&shm->ack_seq);
        break;
    }

    /* ---- Renderbuffers ---- */
    case GL_CMD_GEN_RENDERBUFFERS: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        glGenRenderbuffers(n, id_buf);
        respond_data(0, id_buf, n * 4);
        break;
    }
    case GL_CMD_DELETE_RENDERBUFFERS: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        for (uint32_t i = 0; i < n; i++) id_buf[i] = read_u32();
        glDeleteRenderbuffers(n, id_buf);
        break;
    }
    case GL_CMD_BIND_RENDERBUFFER: {
        uint32_t target=read_u32(), rb=read_u32();
        glBindRenderbuffer(target, rb);
        break;
    }
    case GL_CMD_RENDERBUFFER_STORAGE: {
        uint32_t target=read_u32(), ifmt=read_u32(), w=read_u32(), h=read_u32();
        glRenderbufferStorage(target, ifmt, w, h);
        break;
    }
    case GL_CMD_FRAMEBUFFER_RENDERBUFFER: {
        uint32_t target=read_u32(), attach=read_u32(), rbtarget=read_u32(), rb=read_u32();
        glFramebufferRenderbuffer(target, attach, rbtarget, rb);
        break;
    }

    /* ---- Shaders ---- */
    case GL_CMD_CREATE_SHADER: {
        uint32_t type = read_u32();
        GLuint s = glCreateShader(type);
        respond_u32(s);
        break;
    }
    case GL_CMD_DELETE_SHADER:      glDeleteShader(read_u32()); break;
    case GL_CMD_SHADER_SOURCE: {
        uint32_t shader = read_u32();
        uint32_t src_len = read_u32();
        if (src_len >= sizeof(str_buf)) src_len = sizeof(str_buf) - 1;
        read_data(str_buf, src_len);
        str_buf[src_len] = '\0';
        const char *src = str_buf;
        GLint len = (GLint)src_len;
        glShaderSource(shader, 1, &src, &len);
        break;
    }
    case GL_CMD_COMPILE_SHADER: {
        uint32_t shader = read_u32();
        glCompileShader(shader);
        if (g_debug) {
            GLint ok = 0;
            glGetShaderiv(shader, 0x8B81/*GL_COMPILE_STATUS*/, &ok);
            if (!ok) {
                char log[512];
                glGetShaderInfoLog(shader, sizeof(log), NULL, log);
                fprintf(stderr, "[gl_proxy] SHADER %u COMPILE FAILED: %s\n", shader, log);
            } else {
                DBG("shader %u compiled OK\n", shader);
            }
        }
        break;
    }
    case GL_CMD_GET_SHADERIV: {
        uint32_t shader = read_u32(), pname = read_u32();
        GLint val = 0;
        glGetShaderiv(shader, pname, &val);
        respond_u32((uint32_t)val);
        break;
    }
    case GL_CMD_GET_SHADER_INFO_LOG: {
        uint32_t shader = read_u32(), maxlen = read_u32();
        if (maxlen > sizeof(str_buf)) maxlen = sizeof(str_buf);
        GLsizei actual = 0;
        glGetShaderInfoLog(shader, maxlen, &actual, str_buf);
        respond_data((uint32_t)actual, str_buf, actual > 0 ? actual : 0);
        break;
    }
    case GL_CMD_ATTACH_SHADER: {
        uint32_t prog = read_u32(), shader = read_u32();
        glAttachShader(prog, shader);
        break;
    }
    case GL_CMD_GET_ATTACHED_SHADERS: {
        uint32_t prog = read_u32(), maxCount = read_u32();
        if (maxCount > 64) maxCount = 64;
        GLuint shaders[64];
        GLsizei count = 0;
        glGetAttachedShaders(prog, maxCount, &count, shaders);
        respond_data((uint32_t)count, shaders, count * 4);
        break;
    }

    /* ---- Programs ---- */
    case GL_CMD_CREATE_PROGRAM: {
        GLuint p = glCreateProgram();
        respond_u32(p);
        break;
    }
    case GL_CMD_DELETE_PROGRAM:     glDeleteProgram(read_u32()); break;
    case GL_CMD_USE_PROGRAM:        glUseProgram(read_u32()); break;
    case GL_CMD_LINK_PROGRAM: {
        uint32_t prog = read_u32();
        glLinkProgram(prog);
        if (g_debug) {
            GLint ok = 0;
            glGetProgramiv(prog, 0x8B82/*GL_LINK_STATUS*/, &ok);
            if (!ok) {
                char log[512];
                glGetProgramInfoLog(prog, sizeof(log), NULL, log);
                fprintf(stderr, "[gl_proxy] PROGRAM %u LINK FAILED: %s\n", prog, log);
            } else {
                DBG("program %u linked OK\n", prog);
            }
        }
        break;
    }
    case GL_CMD_GET_PROGRAMIV: {
        uint32_t prog = read_u32(), pname = read_u32();
        GLint val = 0;
        glGetProgramiv(prog, pname, &val);
        respond_u32((uint32_t)val);
        break;
    }
    case GL_CMD_GET_PROGRAM_INFO_LOG: {
        uint32_t prog = read_u32(), maxlen = read_u32();
        if (maxlen > sizeof(str_buf)) maxlen = sizeof(str_buf);
        GLsizei actual = 0;
        glGetProgramInfoLog(prog, maxlen, &actual, str_buf);
        respond_data((uint32_t)actual, str_buf, actual > 0 ? actual : 0);
        break;
    }
    case GL_CMD_GET_UNIFORM_LOCATION: {
        uint32_t prog = read_u32();
        uint32_t name_len = read_u32();
        if (name_len >= sizeof(str_buf)) name_len = sizeof(str_buf) - 1;
        read_data(str_buf, name_len);
        str_buf[name_len] = '\0';
        GLint loc = glGetUniformLocation(prog, str_buf);
        respond_u32((uint32_t)(int32_t)loc);
        break;
    }
    case GL_CMD_GET_ATTRIB_LOCATION: {
        uint32_t prog = read_u32();
        uint32_t name_len = read_u32();
        if (name_len >= sizeof(str_buf)) name_len = sizeof(str_buf) - 1;
        read_data(str_buf, name_len);
        str_buf[name_len] = '\0';
        GLint loc = glGetAttribLocation(prog, str_buf);
        respond_u32((uint32_t)(int32_t)loc);
        break;
    }

    /* ---- Uniforms ---- */
    case GL_CMD_UNIFORM_1I: { int32_t l=(int32_t)read_u32(),v=(int32_t)read_u32(); glUniform1i(l,v); break; }
    case GL_CMD_UNIFORM_1F: { int32_t l=(int32_t)read_u32(); float v=read_f32(); glUniform1f(l,v); break; }
    case GL_CMD_UNIFORM_2F: { int32_t l=(int32_t)read_u32(); float x=read_f32(),y=read_f32(); glUniform2f(l,x,y); break; }
    case GL_CMD_UNIFORM_3F: { int32_t l=(int32_t)read_u32(); float x=read_f32(),y=read_f32(),z=read_f32(); glUniform3f(l,x,y,z); break; }
    case GL_CMD_UNIFORM_4F: { int32_t l=(int32_t)read_u32(); float x=read_f32(),y=read_f32(),z=read_f32(),w=read_f32(); glUniform4f(l,x,y,z,w); break; }
    case GL_CMD_UNIFORM_4FV: {
        int32_t loc = (int32_t)read_u32();
        uint32_t count = read_u32();
        uint32_t n_floats = count * 4;
        if (n_floats > 1024) n_floats = 1024;
        for (uint32_t i = 0; i < n_floats; i++) float_buf[i] = read_f32();
        glUniform4fv(loc, count, float_buf);
        break;
    }
    case GL_CMD_UNIFORM_MATRIX_4FV: {
        int32_t loc = (int32_t)read_u32();
        uint32_t count = read_u32();
        uint32_t transpose = read_u32();
        uint32_t n_floats = count * 16;
        if (n_floats > 1024) n_floats = 1024;
        for (uint32_t i = 0; i < n_floats; i++) float_buf[i] = read_f32();
        glUniformMatrix4fv(loc, count, transpose, float_buf);
        break;
    }

    /* ---- Buffers ---- */
    case GL_CMD_GEN_BUFFERS: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        glGenBuffers(n, id_buf);
        respond_data(0, id_buf, n * 4);
        break;
    }
    case GL_CMD_DELETE_BUFFERS: {
        uint32_t n = read_u32();
        if (n > 256) n = 256;
        for (uint32_t i = 0; i < n; i++) id_buf[i] = read_u32();
        glDeleteBuffers(n, id_buf);
        break;
    }
    case GL_CMD_BIND_BUFFER: {
        uint32_t target = read_u32(), buf = read_u32();
        glBindBuffer(target, buf);
        break;
    }
    case GL_CMD_BUFFER_DATA: {
        uint32_t target = read_u32();
        uint32_t size = read_u32();
        uint32_t usage = read_u32();
        uint32_t has_data = read_u32();
        if (has_data) {
            uint32_t data_size = read_u32();
            if (data_size > sizeof(tmp_buf)) data_size = sizeof(tmp_buf);
            read_data(tmp_buf, data_size);
            glBufferData(target, size, tmp_buf, usage);
        } else {
            glBufferData(target, size, NULL, usage);
        }
        break;
    }
    case GL_CMD_BUFFER_SUB_DATA: {
        uint32_t target = read_u32();
        uint32_t offset = read_u32();
        uint32_t size = read_u32();
        uint32_t data_size = read_u32();
        if (data_size > sizeof(tmp_buf)) data_size = sizeof(tmp_buf);
        read_data(tmp_buf, data_size);
        glBufferSubData(target, offset, size, tmp_buf);
        break;
    }
    case GL_CMD_VERTEX_ATTRIB_POINTER: {
        uint32_t index = read_u32();
        int32_t size_arg = (int32_t)read_u32();
        uint32_t type = read_u32();
        uint32_t normalized = read_u32();
        uint32_t stride = read_u32();
        uint32_t offset = read_u32();
        static int vap_log = 0;
        if (g_debug && vap_log < 20) {
            GLint bound_buf = 0;
            glGetIntegerv(0x8894/*GL_ARRAY_BUFFER_BINDING*/, &bound_buf);
            DBG("glVertexAttribPointer(idx=%u, size=%d, type=0x%x, stride=%u, ptr=0x%x) VBO=%d\n",
                index, size_arg, type, stride, offset, bound_buf);
            vap_log++;
        }
        glVertexAttribPointer(index, size_arg, type, normalized, stride, (const void*)(uintptr_t)offset);
        break;
    }
    case GL_CMD_ENABLE_VERTEX_ATTRIB_ARRAY:  glEnableVertexAttribArray(read_u32()); break;
    case GL_CMD_DISABLE_VERTEX_ATTRIB_ARRAY: glDisableVertexAttribArray(read_u32()); break;

    /* ---- Client-side array upload ---- */
    case GL_CMD_UPLOAD_CLIENT_ARRAY: {
        /* Upload vertex data into a temp VBO and set attrib pointer */
        uint32_t index = read_u32();
        int32_t size_arg = (int32_t)read_u32();
        uint32_t type = read_u32();
        uint32_t normalized = read_u32();
        uint32_t stride = read_u32();
        uint32_t data_size = read_u32();
        /* Allocate temp buffer for vertex data */
        void *data = NULL;
        if (data_size > 0) {
            if (data_size <= sizeof(tmp_buf)) {
                read_data(tmp_buf, data_size);
                data = tmp_buf;
            } else {
                data = malloc(data_size);
                if (data) read_data(data, data_size);
            }
        }
        /* Create/reuse a temp VBO for this attrib */
        static GLuint client_vbos[16] = {0};
        if (client_vbos[index] == 0)
            glGenBuffers(1, &client_vbos[index]);
        glBindBuffer(GL_ARRAY_BUFFER, client_vbos[index]);
        glBufferData(GL_ARRAY_BUFFER, data_size, data, 0x88E0/*GL_STREAM_DRAW*/);
        glVertexAttribPointer(index, size_arg, type, normalized, stride, (const void*)0);
        glBindBuffer(GL_ARRAY_BUFFER, 0);
        if (data && data != (void*)tmp_buf) free(data);
        break;
    }
    case GL_CMD_UPLOAD_INDEX_ARRAY: {
        /* Upload index data into a temp EBO */
        uint32_t data_size = read_u32();
        void *data = NULL;
        if (data_size > 0) {
            if (data_size <= sizeof(tmp_buf)) {
                read_data(tmp_buf, data_size);
                data = tmp_buf;
            } else {
                data = malloc(data_size);
                if (data) read_data(data, data_size);
            }
        }
        static GLuint client_ebo = 0;
        if (client_ebo == 0)
            glGenBuffers(1, &client_ebo);
        glBindBuffer(0x8893/*GL_ELEMENT_ARRAY_BUFFER*/, client_ebo);
        glBufferData(0x8893/*GL_ELEMENT_ARRAY_BUFFER*/, data_size, data, 0x88E0/*GL_STREAM_DRAW*/);
        /* Leave EBO bound - the following DrawElements will use offset=0 */
        if (data && data != (void*)tmp_buf) free(data);
        break;
    }

    /* ---- Draw ---- */
    case GL_CMD_DRAW_ARRAYS: {
        uint32_t mode = read_u32();
        int32_t first = (int32_t)read_u32();
        uint32_t count = read_u32();
        static int draw_log = 0;
        if (g_debug && draw_log < 20) {
            DBG("glDrawArrays(mode=0x%x, first=%d, count=%u)\n", mode, first, count);
            GLenum err = glGetError();
            if (err) DBG("  GL error before draw: 0x%x\n", err);
            draw_log++;
        }
        glDrawArrays(mode, first, count);
        if (g_debug && draw_log <= 20) {
            GLenum err = glGetError();
            if (err) fprintf(stderr, "[gl_proxy] GL ERROR after glDrawArrays: 0x%x\n", err);
        }
        break;
    }
    case GL_CMD_DRAW_ELEMENTS: {
        uint32_t mode = read_u32();
        uint32_t count = read_u32();
        uint32_t type = read_u32();
        uint32_t offset = read_u32();
        static int draw_el_log = 0;
        if (g_debug && draw_el_log < 20) {
            DBG("glDrawElements(mode=0x%x, count=%u, type=0x%x, offset=%u)\n", mode, count, type, offset);
            draw_el_log++;
        }
        glDrawElements(mode, count, type, (const void*)(uintptr_t)offset);
        if (g_debug && draw_el_log <= 20) {
            GLenum err = glGetError();
            if (err) fprintf(stderr, "[gl_proxy] GL ERROR after glDrawElements: 0x%x\n", err);
        }
        break;
    }

    /* ---- Query ---- */
    case GL_CMD_GET_ERROR: {
        GLenum err = glGetError();
        respond_u32(err);
        break;
    }
    case GL_CMD_GET_STRING: {
        uint32_t name = read_u32();
        const char *s = (const char *)glGetString(name);
        if (s) {
            uint32_t len = strlen(s);
            respond_data(len, s, len);
        } else {
            respond_data(0, "", 0);
        }
        break;
    }
    case GL_CMD_GET_INTEGERV: {
        uint32_t pname = read_u32();
        uint32_t count = read_u32();  /* how many values expected */
        if (count > 256) count = 256;
        memset(int_buf, 0, count * 4);
        glGetIntegerv(pname, int_buf);
        respond_data(0, int_buf, count * 4);
        break;
    }
    case GL_CMD_GET_FLOATV: {
        uint32_t pname = read_u32();
        uint32_t count = read_u32();
        if (count > 256) count = 256;
        memset(float_buf, 0, count * sizeof(float));
        glGetFloatv(pname, float_buf);
        respond_data(0, float_buf, count * sizeof(float));
        break;
    }

    default:
        ERR("Unknown command %d\n", cmd_id);
        break;
    }
}

/* ------------------------------------------------------------------ */
/* Main loop                                                           */
/* ------------------------------------------------------------------ */

static volatile int running = 1;

static void sig_handler(int sig) {
    (void)sig;
    running = 0;
}

static void process_commands(void) {
    while (1) {
        uint32_t rp = shm->read_pos;
        uint32_t wp = __atomic_load_n(&shm->write_pos, __ATOMIC_ACQUIRE);

        if (rp == wp) break;  /* ring empty */

        /* Read command header */
        struct cmd_header hdr;
        ring_read(shm, rp, &hdr, sizeof(hdr));

        if (hdr.size < sizeof(hdr) || hdr.size > RING_SIZE / 2) {
            ERR("Bad command size %u at rp=%u, cmd_id=%u\n", hdr.size, rp, hdr.cmd_id);
            /* Try to recover by skipping */
            shm->read_pos = wp;
            break;
        }

        /* Set up read position for args (after header) */
        rd_pos = rp + sizeof(hdr);

        /* Dispatch */
        uint32_t body_size = hdr.size - sizeof(hdr);
        dispatch_command(hdr.cmd_id, hdr.flags, body_size);

        /* Advance read pointer */
        __atomic_store_n(&shm->read_pos, rp + align4(hdr.size), __ATOMIC_RELEASE);
    }
}

int main(int argc, char **argv) {
    (void)argc; (void)argv;

    const char *debug_env = getenv("GL_PROXY_DEBUG");
    if (!debug_env) debug_env = getenv("LIBMALI_DEBUG");
    if (debug_env && (debug_env[0] == '1' || debug_env[0] == 'y'))
        g_debug = 1;

    fprintf(stderr, "[gl_proxy] Starting native GL proxy...\n");

    signal(SIGINT, sig_handler);
    signal(SIGTERM, sig_handler);
    signal(SIGUSR1, request_capture);

    /* Init input FIFOs (before shm so FIFOs exist when m2engage starts) */
    init_input_fifos();

    /* Init shared memory and eventfds */
    if (!init_shm()) { ERR("Failed to init shared memory\n"); return 1; }

    /* Init GBM + EGL (GPU via surfaceless context) */
    if (!init_gbm_egl()) { ERR("Failed to init GBM/EGL\n"); return 1; }

    /* Init X11 window (display + keyboard) */
    if (!init_x11_display()) { ERR("Failed to init X11 display\n"); return 1; }

    /* Mark proxy as ready */
    clock_gettime(CLOCK_MONOTONIC, &fps_start);
    frame_deadline_ns = (int64_t)fps_start.tv_sec * 1000000000 + fps_start.tv_nsec;
    __atomic_store_n(&shm->proxy_ready, 1, __ATOMIC_RELEASE);
    fprintf(stderr, "[gl_proxy] Ready, waiting for commands...\n");

    /* Main loop: futex-based wait on write_pos changes */
    while (running) {
        /* Drain any commands already in the ring */
        process_commands();

        /* Check shutdown flag */
        if (__atomic_load_n(&shm->shutdown, __ATOMIC_ACQUIRE)) {
            fprintf(stderr, "[gl_proxy] Shutdown requested\n");
            break;
        }

        /* Wait for new commands: futex_wait on write_pos
         * If write_pos still equals read_pos (ring empty), sleep.
         * Use short timeout (10ms) for responsive X11 event handling. */
        uint32_t wp = __atomic_load_n(&shm->write_pos, __ATOMIC_ACQUIRE);
        if (wp == shm->read_pos) {
            futex_wait(&shm->write_pos, wp, 10);
        }

        /* Process any commands that arrived */
        process_commands();
    }

    /* Cleanup */
    fprintf(stderr, "[gl_proxy] Shutting down (swaps=%d)\n", swap_count);

    /* Display FBO cleanup */
    if (display_fbo) glDeleteFramebuffers(1, &display_fbo);
    if (display_color_tex) glDeleteTextures(1, &display_color_tex);
    if (display_depth_rb) glDeleteRenderbuffers(1, &display_depth_rb);

    /* EGL cleanup */
    eglMakeCurrent(egl_dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglDestroyContext(egl_dpy, egl_ctx);
    eglTerminate(egl_dpy);

    /* GBM cleanup */
    if (gbm_dev) gbm_device_destroy(gbm_dev);
    if (drm_fd >= 0) close(drm_fd);

    /* X11 cleanup */
    if (x_image) XDestroyImage(x_image);
    if (x_gc) XFreeGC(x_dpy, x_gc);
    if (x_win) XDestroyWindow(x_dpy, x_win);
    if (x_dpy) XCloseDisplay(x_dpy);

    /* SHM cleanup */
    munmap(shm, GL_SHM_TOTAL_SIZE);
    shm_unlink(GL_SHM_NAME);

    for (int i = 0; i < 2; i++) {
        if (fifo_fds[i] >= 0) close(fifo_fds[i]);
    }
    unlink(INPUT_FIFO_0);
    unlink(INPUT_FIFO_1);

    return 0;
}
