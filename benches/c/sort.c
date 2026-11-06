/* Benchmark mirror of benches/sort.plk: bottom-up mergesort, 600 rounds. */
#include <stdio.h>
#include <stdint.h>

#define N 512

static void fill(int32_t *a, int64_t seed) {
    int64_t x = seed;
    for (int32_t i = 0; i < N; i++) {
        x = x ^ (x << 13);
        x = x ^ (x >> 7);
        x = x ^ (x << 17);
        a[i] = (int32_t)(x & 65535);
    }
}

static void merge(const int32_t *a, int32_t *t, int32_t lo, int32_t mid, int32_t hi) {
    int32_t i = lo, j = mid, k = lo;
    while (i < mid && j < hi) {
        if (a[i] <= a[j]) { t[k++] = a[i++]; } else { t[k++] = a[j++]; }
    }
    while (i < mid) t[k++] = a[i++];
    while (j < hi) t[k++] = a[j++];
}

static void copy_back(const int32_t *t, int32_t *a) {
    for (int32_t i = 0; i < N; i++) a[i] = t[i];
}

int main(void) {
    int32_t data[N], tmp[N];
    int64_t total = 0;
    for (int32_t r = 0; r < 600; r++) {
        fill(data, (int64_t)88172645463325252LL + (int64_t)r);
        for (int32_t width = 1; width < N; width *= 2) {
            for (int32_t lo = 0; lo < N; lo += 2 * width) {
                int32_t mid = lo + width < N ? lo + width : N;
                int32_t hi = lo + 2 * width < N ? lo + 2 * width : N;
                merge(data, tmp, lo, mid, hi);
            }
            copy_back(tmp, data);
        }
        for (int32_t i = 0; i < N; i++) total += (int64_t)data[i] * (int64_t)(i + 1);
    }
    printf("%lld\n", (long long)total);
    return 0;
}
