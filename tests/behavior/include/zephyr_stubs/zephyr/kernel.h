/* Minimal Zephyr kernel stub for behavior tests (host-side). */
#ifndef ZEPHYR_KERNEL_STUB_H
#define ZEPHYR_KERNEL_STUB_H

#include <stdint.h>

typedef struct { int64_t ticks; } k_timeout_t;

struct k_timer {
        void (*expiry_fn)(struct k_timer *);
        void (*stop_fn)(struct k_timer *);
        void *user_data;
};

typedef void (*k_timer_expiry_t)(struct k_timer *);
typedef void (*k_timer_stop_t)(struct k_timer *);

#define K_MSEC(ms) ((k_timeout_t){(int64_t)(ms)})
#define K_NO_WAIT  ((k_timeout_t){0})

static inline void k_timer_init(struct k_timer *timer,
                                k_timer_expiry_t expiry_fn,
                                k_timer_stop_t stop_fn)
{
        timer->expiry_fn = expiry_fn;
        timer->stop_fn = stop_fn;
        timer->user_data = (void *)0;
}

static inline void k_timer_start(struct k_timer *timer,
                                  k_timeout_t duration,
                                  k_timeout_t period)
{
        (void)timer;
        (void)duration;
        (void)period;
}

static inline void k_timer_stop(struct k_timer *timer)
{
        (void)timer;
}

static inline void k_timer_user_data_set(struct k_timer *timer, void *data)
{
        timer->user_data = data;
}

static inline void *k_timer_user_data_get(struct k_timer *timer)
{
        return timer->user_data;
}

#endif /* ZEPHYR_KERNEL_STUB_H */
