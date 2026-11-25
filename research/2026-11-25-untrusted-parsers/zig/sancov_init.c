#include <stdint.h>
extern uint8_t __start___sancov_cntrs[], __stop___sancov_cntrs[];
extern uintptr_t __start___sancov_pcs1[], __stop___sancov_pcs1[];
void __sanitizer_cov_8bit_counters_init(uint8_t *, uint8_t *);
void __sanitizer_cov_pcs_init(const uintptr_t *, const uintptr_t *);
__attribute__((constructor)) static void zig_sancov_init(void) {
  __sanitizer_cov_8bit_counters_init(__start___sancov_cntrs, __stop___sancov_cntrs);
}
