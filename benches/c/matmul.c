/* Benchmark mirror of benches/matmul.plk: 48x48 i32 matmul, 150 rounds. */
#include <stdio.h>
#include <stdint.h>

#define N 48

static void fill(int32_t *m, int64_t seed) {
    int64_t x = seed;
    for (int32_t i = 0; i < N * N; i++) {
        x = x ^ (x << 13);
        x = x ^ (x >> 7);
        x = x ^ (x << 17);
        m[i] = (int32_t)(x & 1023);
    }
}

int main(void) {
    static int32_t a[N * N], b[N * N], o[N * N];
    int64_t total = 0;
    for (int32_t r = 0; r < 150; r++) {
        fill(a, (int64_t)88172645463325252LL + (int64_t)r);
        fill(b, (int64_t)72057594037927936LL + (int64_t)r);
        for (int32_t i = 0; i < N * N; i++) o[i] = 0;
        for (int32_t i = 0; i < N; i++) {
            for (int32_t k = 0; k < N; k++) {
                int32_t av = a[i * N + k];
                for (int32_t j = 0; j < N; j++) {
                    o[i * N + j] += av * b[k * N + j];
                }
            }
        }
        for (int32_t i = 0; i < N * N; i++) total += (int64_t)o[i] * (int64_t)(i + 1);
    }
    printf("%lld\n", (long long)total);
    return 0;
}
