/* SPDX-License-Identifier: GPL-2.0 */
#ifndef MACROS_H
#define MACROS_H

#define ADD_ONE(x) x + 1
#define SQUARE(x) ((x) * (x))
#define TWICE(x) ((x) + (x))
#define UNUSED_ARG(a, b) (a)
#define STATEMENTS(a) a = 1; a++
#define IF_MACRO(a) if (a) foo()
#define SINGLE(a) do { a = 1; } while (0)
#define SEMI(a) do { a++; a--; } while (0);
#define TRAILING 42;
#define RETURNS(x) return x
#define GOTO_OUT goto out
#define CONCAT(a, b) a ## b
#define STRINGIFY(a) #a
#define BIT3 (1 << 3)
#define BIT4 (1UL << 4)
#define ARCH_HAS_FOO 1
#define CONTINUED(a) \
	do { \
		(a)++; \
	} while (0)
#define SPACEY(a)	\
	(a)
#define MULTI(a, b) \
	(a) = 1; \
	(b) = 2;
#define LIST(...) foo(__VA_ARGS__)
#define CFG IS_ENABLED(FOO)

#if 0
#define DEAD 1
#endif
#if 1
#define ALIVE 1
#endif
#ifdef  TWO_SPACES
#endif
#if defined(CONFIG_FOO) || defined(CONFIG_FOO_MODULE)
#endif
#if defined __i386__
#endif

extern int foo(int);
extern int bar(void);
typedef unsigned int my_uint;
typedef int (*handler_t)(int);
typedef unsigned short __bitwise be16_t;

struct point
{
	int x;
	int y;
};
struct line{
	struct point a, b;
};
struct flex {
	int n;
	int data[0];
};
struct flex1 {
	int n;
	char data[1];
};

static inline int inl(void) { return 0; }
static __inline__ int inl2(void)
{
	return 1;
}
int inline wrong_order(void);
int static wrong_storage;

#endif
