// SPDX-License-Identifier: GPL-2.0
#include <linux/kernel.h>

static int helper(int a, int b);
int declared_args(int, char *);
int ok_args(int a, char *b);

static int
multi_line_definition(int a)
{
	return a;
}

static int brace_late(int a)
	{
	return a;
}

static int helper(int a, int b)
{
	if (a)
		return b;
	else
		return a;
}

void void_return(void)
{
	helper(1, 2);
	return;
}

int trailing_after_if(int a)
{
	if (a) return 1;
	while (a--) helper(a, a);
	for (;;) ;
	if (a)
		a++;
	else a--;
	} if (a)
		return 2;
	return 0;
}

int else_after_return(int a)
{
	if (a) {
		return 1;
	} else {
		return 2;
	}
}

int break_after_return(int a)
{
	switch (a) {
	case 0:
		return 1;
		break;
	default:
		break;
	}
	return 0;
}

int deep(int a)
{
	if (a)
		if (a > 1)
			if (a > 2)
				if (a > 3)
					if (a > 4)
						if (a > 5)
							return a;
	return 0;
}

static const struct pci_device_id ids[] = {
	{ PCI_DEVICE(1, 2) },
	{ PCI_DEVICE(3, 4) },
};

static const struct pci_device_id ids2[] = {
	{ PCI_DEVICE(1, 2) },
	{ }
};

int func_name_in_string(void)
{
	pr_info("func_name_in_string called\n");
	return 0;
}

int const_cmp(int a)
{
	if (10 < a)
		return 1;
	if (NULL != a)
		return 2;
	return 0;
}

void *alloc_ptr(void)
{
	int *p __free(kfree);
	int *q __free(kfree) = NULL;

	return q;
}
