/* Benchmark mirror of benches/nqueens.plk: 11-queens bitmask backtracking. */
#include <stdio.h>
#include <stdint.h>

static int32_t solve(int64_t cols, int64_t ld, int64_t rd, int64_t full) {
    if (cols == full) return 1;
    int64_t avail = full ^ ((cols | ld | rd) & full);
    int32_t count = 0;
    while (avail != 0) {
        int64_t bit = avail & (-avail);
        avail = avail ^ bit;
        count += solve(cols | bit, (ld | bit) << 1, (rd | bit) >> 1, full);
    }
    return count;
}

int main(void) {
    int64_t full = (1 << 11) - 1;
    int32_t total = 0;
    for (int32_t r = 0; r < 10; r++) {
        total += solve(0, 0, 0, full);
    }
    printf("%d\n", total);
    return 0;
}
