#ifndef _BITS_STDIO_H
#define _BITS_STDIO_H

#include <stddef.h>

// XXX: this is only here because cbindgen can't handle string constants
#define P_tmpdir "/tmp"

typedef struct FILE FILE;

// A typedef doesn't suffice, because libgmp uses this definition to check if
// STDIO was loaded.
#define FILE FILE

#ifdef __cplusplus
extern "C" {
#endif

int asprintf(char **strp, const char * fmt, ...);
// int fprintf(FILE * stream, const char * fmt, ...);
int printf(const char * fmt, ...);
int snprintf(char *s, size_t n, const char * fmt, ...);
int sprintf(char *s, const char * fmt, ...);
// int fscanf(FILE * stream, const char * fmt, ...);
// int scanf(const char * fmt, ...);
// int sscanf(const char * input, const char * fmt, ...);

#ifdef __cplusplus
} // extern "C"
#endif

#endif /* _BITS_STDIO_H */
