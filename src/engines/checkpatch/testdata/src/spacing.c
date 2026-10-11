// SPDX-License-Identifier: GPL-2.0
/*
 * Spacing, indentation and brace placement.
 */
#include <linux/kernel.h>
#include <linux/module.h>

static int counter = 0;
static int *table;
int global_value = 0;

int add(int a,int b)
{
	return a+b;
}

int sub(int a, int b) {
    return a - b ;
}

void loops(int n)
{
	int i;
	for(i = 0; i < n; i++)
	{
		counter++ ;
	}
	while (n --) {
		if (n == 3)
			continue;
		else {
			counter += n;
		}
	}
	do
	{
		n++;
	} while(n < 10);
	if (n) {
		counter = 1;
	}
	else
		counter = 2;
	if ((n == 4))
		counter = 3;
	if (n = 5) counter = 4;
	switch (n) {
		case 1:
			break;
	default:
		break;
	}
}

int * get_table(void)
{
	return (table);
}

void casts(char* p, unsigned long x)
{
	char *q = (char *) p;
	int y = (int)x;
	unsigned z;

	y = y<<2;
	y = y >>1;
	y = - y;
	y = !! y;
	q = q+ 1;
	p [0] = 'a';
	p[ 1 ] = 'b';
	z = sizeof x;
	if (y == NULL || NULL == q)
		return ;
	if (x == true)
		return;
	x = y ? : z;
	label :
	return;
}

static const char *names[] = { "a", "b" };
const char greeting[] = "hello";
static char buf[] = "world";
