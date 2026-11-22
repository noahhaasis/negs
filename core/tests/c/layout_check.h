/* Shared by layout_check.c and each Phase 3 slice's layout_3*.c. */
#ifndef LAYOUT_CHECK_H
#define LAYOUT_CHECK_H
#include <stddef.h>
#include "negs_core.h"

extern int failures;
void check(const char *name, long long c_value);
void check_3a(void);
void check_3b(void);
void check_3c(void);

#define SIZE(T) check(#T, (long long)sizeof(T))
#define FIELD(T, f) check(#T "." #f, (long long)offsetof(T, f))

#endif
