/*
 * CEP:FILE: runtime/planck_rt.c
 * CEP:WHAT: Minimal C runtime for compiled Planck programs: printing builtins, panic handler, and the process entry wrapper.
 * CEP:WHY: The design's Tier-3 runtime philosophy in miniature: explicit, swappable, and free of hidden state; every service here is called only when the program asks for it.
 * CEP:CLASS: CEP-1
 * CEP:STATUS: complete
 * CEP:FAILURE: print functions perform one write(2) each and report failures by returning silently (output errors are not fatal per design; the exit code still reflects the program). planck_rt_panic prints to stderr and exits with code 101; it never returns.
 * CEP:ASSUMES: POSIX write(2) is available. Integer formatting is manual and deterministic; floats use snprintf with the fixed %.6f format (documented deviation, ticket PLK-016). The entry wrapper assumes planck_main returns an i32 exit code.
 * CEP:COST: print_i64/print_u64 are O(digits) with no allocation; print_str is one write syscall; the panic path terminates the process.
 * CEP:EVIDENCE: end-to-end execution tests in crates/planckc/tests.
 * CEP:SECURITY: All pointers originate from compiler-generated code (stack frame or read-only data); lengths are compiler-provided constants. No untrusted input reaches this file.
 * CEP:PORTABILITY: POSIX (Linux/macOS); Windows support is ticket PLK-027.
 */

#include <stdint.h>
#include <stdio.h>
#include <unistd.h>
#include <stdlib.h>

extern int planck_main(void);

int main(void) {
    return planck_main();
}

static void write_all(const char *buf, uint64_t len) {
    uint64_t written = 0;
    while (written < len) {
        ssize_t n = write(1, buf + written, len - written);
        if (n <= 0) {
            return; /* output failure is not fatal (see CEP:FAILURE) */
        }
        written += (uint64_t)n;
    }
}

/* Decimal formatting into a fixed buffer; returns pointer and updates length. */
static const char *format_i64(int64_t value, char *buf, uint64_t *len) {
    char tmp[24];
    uint64_t index = 0;
    uint64_t magnitude;
    int negative = value < 0;
    if (negative) {
        magnitude = (uint64_t)(-(int64_t)(value + 1)) + 1; /* avoids UB at INT64_MIN */
    } else {
        magnitude = (uint64_t)value;
    }
    if (magnitude == 0) {
        tmp[index++] = '0';
    }
    while (magnitude > 0) {
        tmp[index++] = (char)('0' + (magnitude % 10));
        magnitude /= 10;
    }
    uint64_t out = 0;
    if (negative) {
        buf[out++] = '-';
    }
    while (index > 0) {
        buf[out++] = tmp[--index];
    }
    *len = out;
    return buf;
}

void planck_rt_print_i64(int64_t value) {
    char buf[32];
    uint64_t len = 0;
    format_i64(value, buf, &len);
    write_all(buf, len);
}

void planck_rt_print_u64(uint64_t value) {
    char buf[32];
    char tmp[24];
    uint64_t index = 0;
    if (value == 0) {
        tmp[index++] = '0';
    }
    while (value > 0) {
        tmp[index++] = (char)('0' + (value % 10));
        value /= 10;
    }
    uint64_t out = 0;
    while (index > 0) {
        buf[out++] = tmp[--index];
    }
    write_all(buf, out);
}

void planck_rt_print_f64(double value) {
    char buf[64];
    int n = snprintf(buf, sizeof(buf), "%.6f", value);
    if (n > 0) {
        write_all(buf, (uint64_t)n);
    }
}

void planck_rt_print_bool(uint8_t value) {
    if (value) {
        write_all("true", 4);
    } else {
        write_all("false", 5);
    }
}

void planck_rt_print_char(uint32_t scalar) {
    char buf[4];
    uint64_t len = 0;
    if (scalar < 0x80u) {
        buf[len++] = (char)scalar;
    } else if (scalar < 0x800u) {
        buf[len++] = (char)(0xC0u | (scalar >> 6));
        buf[len++] = (char)(0x80u | (scalar & 0x3Fu));
    } else if (scalar < 0x10000u) {
        buf[len++] = (char)(0xE0u | (scalar >> 12));
        buf[len++] = (char)(0x80u | ((scalar >> 6) & 0x3Fu));
        buf[len++] = (char)(0x80u | (scalar & 0x3Fu));
    } else {
        buf[len++] = (char)(0xF0u | (scalar >> 18));
        buf[len++] = (char)(0x80u | ((scalar >> 12) & 0x3Fu));
        buf[len++] = (char)(0x80u | ((scalar >> 6) & 0x3Fu));
        buf[len++] = (char)(0x80u | (scalar & 0x3Fu));
    }
    write_all(buf, len);
}

void planck_rt_print_str(const char *ptr, uint64_t len) {
    write_all(ptr, len);
}

void planck_rt_panic(const char *kind) {
    (void)!write(2, "planck panic: ", 14);
    const char *p = kind;
    while (*p != 0) {
        p++;
    }
    (void)!write(2, kind, (size_t)(p - kind));
    (void)!write(2, "\n", 1);
    exit(101);
}
