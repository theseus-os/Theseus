#ifndef _STRING_H
#define _STRING_H

#include <stddef.h>
#include <stdint.h>
#include <strings.h>

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

void *memccpy(void *dest, const void *src, int c, size_t n);

void *memchr(const void *haystack, int needle, size_t len);

int memcmp(const void *s1, const void *s2, size_t n);

void *memcpy(void *s1, const void *s2, size_t n);

void *memmove(void *s1, const void *s2, size_t n);

void *memrchr(const void *haystack, int needle, size_t len);

void *memset(void *s, int c, size_t n);

char *strcasestr(const char *haystack, const char *needle);

char *strcat(char *s1, const char *s2);

char *strchr(const char *s, int c);

int strcmp(const char *s1, const char *s2);

int strcoll(const char *s1, const char *s2);

char *strcpy(char *dst, const char *src);

size_t strcspn(const char *s1, const char *s2);

char *strdup(const char *s1);

char *strerror(int errnum);

int strerror_r(int errnum, char *buf, size_t buflen);

size_t strlen(const char *s);

char *strncat(char *s1, const char *s2, size_t n);

int strncmp(const char *s1, const char *s2, size_t n);

char *strncpy(char *dst, const char *src, size_t n);

char *strndup(const char *s1, size_t size);

size_t strnlen(const char *s, size_t size);

size_t strnlen_s(const char *s, size_t size);

char *strpbrk(const char *s1, const char *s2);

char *strrchr(const char *s, int c);

const char *strsignal(int sig);

size_t strspn(const char *s1, const char *s2);

char *strstr(const char *haystack, const char *needle);

char *strtok(char *s1, const char *delimiter);

char *strtok_r(char *s, const char *delimiter, char **lasts);

size_t strxfrm(char *s1, const char *s2, size_t n);

#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus

#endif /* _STRING_H */
