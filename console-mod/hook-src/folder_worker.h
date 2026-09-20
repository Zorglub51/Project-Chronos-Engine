/* One transient IO worker. Never calls the VM, graphics or emulator API.
 * begin/poll are called only by the menu thread. Mutex protects completion;
 * IO runs outside the mutex. No queue or persistent cache. */
#ifndef CHRONOS_FOLDER_WORKER_H
#define CHRONOS_FOLDER_WORKER_H
#include <pthread.h>
#include <errno.h>
#include <string.h>

struct folder_worker {
    pthread_mutex_t lock;
    pthread_t thread;
    int active, done, result;
    int (*run)(const char *, const char *);
    char lineup[8], dir[64];
};
#define FOLDER_WORKER_INIT { .lock = PTHREAD_MUTEX_INITIALIZER }

static void *folder_worker_run(void *arg)
{
    struct folder_worker *w = arg;
    int result = w->run(w->lineup, w->dir);
    pthread_mutex_lock(&w->lock);
    w->result = result;
    w->done = 1;
    pthread_mutex_unlock(&w->lock);
    return NULL;
}

static int folder_worker_begin(struct folder_worker *w, const char *lineup,
                               const char *dir, int (*run)(const char *, const char *))
{
    if (w->active) return EBUSY;
    if (!lineup || !dir || strlen(lineup) >= sizeof(w->lineup) ||
        strlen(dir) >= sizeof(w->dir)) return EINVAL;
    strcpy(w->lineup, lineup); strcpy(w->dir, dir);
    w->run = run; w->done = 0; w->active = 1;
    pthread_attr_t attr;
    int error = pthread_attr_init(&attr);
    if (error) { w->active = 0; return error; }
    error = pthread_attr_setstacksize(&attr, 256 * 1024);
    if (!error) error = pthread_create(&w->thread, &attr, folder_worker_run, w);
    pthread_attr_destroy(&attr);
    if (error) w->active = 0;
    return error;
}

/* 1 = busy, 0 = succeeded, -1 = failed/no request. Reaps finished threads. */
static int folder_worker_poll(struct folder_worker *w)
{
    if (!w->active) return -1;
    pthread_mutex_lock(&w->lock);
    int done = w->done, result = w->result;
    pthread_mutex_unlock(&w->lock);
    if (!done) return 1;
    pthread_join(w->thread, NULL);
    w->active = 0;
    return result == 0 ? 0 : -1;
}
#endif
