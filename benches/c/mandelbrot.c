/* Benchmark mirror of benches/mandelbrot.plk: 400x300 escape iteration. */
#include <stdio.h>
#include <stdint.h>

int main(void) {
    int64_t total = 0;
    for (int32_t py = 0; py < 300; py++) {
        double y0 = (double)py * 0.01 - 1.5;
        for (int32_t px = 0; px < 400; px++) {
            double x0 = (double)px * 0.01 - 2.0;
            double x = 0.0, y = 0.0;
            int32_t it = 0;
            while (it < 100 && x * x + y * y <= 4.0) {
                double xt = x * x - y * y + x0;
                y = 2.0 * x * y + y0;
                x = xt;
                it++;
            }
            total += (int64_t)it;
        }
    }
    printf("%lld\n", (long long)total);
    return 0;
}
