// SPDX-License-Identifier: GPL-2.0
#include <linux/types.h>

typedef struct my_thing {
	int a;
} my_thing_t;

static unsigned long long int big;
static long int medium;
static short int small;
static int unsigned misordered;
static long unsigned misordered2;
static signed sign_only;
static unsigned *ptr_only;
static const int const twice;
const static int static_after;
static int const late_const = 1;
static char *strs[] = { "x", "y" };
static int __initdata data_init;
static const int __initdata const_initdata = 1;
static int __initconst no_const;
static const int __read_mostly ro = 2;
int __weak weak_fn(void);
int old_style();
int (*fp) (int);
int ( *fp2)(int);
int (* fp3)(int);
int (*fp4 )(int);
void* bad_pointer;
char * spaced;
int **pp, * *qq;
long x = (long long)1;
unsigned int y = (unsigned int)7;
extern struct foo linker_thing[];

struct thing {
	int a;
	int b;
} thing_instance = {
	.a = 1,
	.b = 2,
};

int arrays(int n)
{
	int a[10];
	int b[ 3 ];

	return sizeof(a) / sizeof(a[0]) + ARRAY_SIZE(b) + sizeof(int) + n;
}

int self_assign(void)
{
	int unused = unused;

	return 0;
}

int multiline_deref(struct thing *t)
{
	return t->
		a;
}
