/* Benchmark mirror of benches/primes.plk: trial-division prime count. */
#include <stdio.h>
#include <stdint.h>
#include <stdbool.h>

static bool is_prime(int64_t n) {
    if (n < 2) return false;
    if (n % 2 == 0) return n == 2;
    for (int64_t d = 3; d * d <= n; d += 2) {
        if (n % d == 0) return false;
    }
    return true;
}

int main(void) {
    int32_t count = 0;
    for (int32_t i = 0; i < 200000; i++) {
        if (is_prime((int64_t)i)) count++;
    }
    printf("%d\n", count);
    return 0;
}
