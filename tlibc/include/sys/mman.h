#ifndef _SYS_MMAN_H
#define _SYS_MMAN_H

#include <stdint.h>
#include <sys/types.h>

#define MAP_ANON 32
#define MAP_ANONYMOUS MAP_ANON
#define MAP_FIXED 16
#define MAP_FIXED_NOREPLACE 1048576
#define MAP_PRIVATE 2
#define MAP_SHARED 1
#define MAP_TYPE 15
#define MS_ASYNC 1
#define MS_INVALIDATE 2
#define MS_SYNC 4

#define PROT_EXEC 4
#define PROT_NONE 0
#define PROT_READ 1
#define PROT_WRITE 2


#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

void *mmap(void *addr, size_t len, int prot, int flags, int fildes, off_t off);

// int mprotect(void *addr, size_t len, int prot);

// int msync(void *addr, size_t len, int flags);

int munmap(void *addr, size_t len);

// int shm_open(const char *name, int oflag, mode_t mode);

// int shm_unlink(const char *name);

#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus

#endif /* _SYS_MMAN_H */
