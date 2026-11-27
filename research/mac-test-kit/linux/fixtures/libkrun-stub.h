/* A stand-in for libkrun 1.19's header, only to check krun/krun-boot.c compiles (linux/test-krun.sh). */
#include <stdint.h>
#include <stdbool.h>
#define KRUN_KERNEL_FORMAT_RAW 0
#define KRUN_KERNEL_FORMAT_ELF 1
#define KRUN_DISK_FORMAT_RAW 0
#define COMPAT_NET_FEATURES 0
#define NET_FLAG_VFKIT 1
int32_t krun_create_ctx(void); int32_t krun_set_vm_config(uint32_t,uint8_t,uint32_t);
int32_t krun_set_kernel(uint32_t,const char*,uint32_t,const char*,const char*);
int32_t krun_disable_implicit_init(uint32_t); int32_t krun_add_disk2(uint32_t,const char*,const char*,uint32_t,bool);
int32_t krun_disable_implicit_vsock(uint32_t); int32_t krun_add_vsock(uint32_t,uint32_t);
int32_t krun_add_vsock_port2(uint32_t,uint32_t,const char*,bool);
int32_t krun_add_net_unixgram(uint32_t,const char*,int,uint8_t*,uint32_t,uint32_t);
int32_t krun_set_console_output(uint32_t,const char*); int32_t krun_get_shutdown_eventfd(uint32_t);
int32_t krun_start_enter(uint32_t);
