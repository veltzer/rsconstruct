// SPDX-License-Identifier: GPL-2.0
#include <linux/slab.h>
#include <linux/delay.h>
#include <linux/device.h>
#include <linux/module.h>
#include <linux/string.h>
#include <linux/jiffies.h>
#include <asm/io.h>

struct foo {
	int a;
	spinlock_t lock;
	struct mutex m;
};

static DEFINE_MUTEX(big_lock);
static int param;
module_param(param, int, 644);
module_param(other, int, 0);
static DEVICE_ATTR(foo, 0644, foo_show, foo_store);
static DEVICE_ATTR(bar, S_IRUGO, bar_show, NULL);
static DEVICE_ATTR(baz, 0444, show_baz, NULL);

int kapi(struct foo *p, char *dst, const char *src, size_t n, unsigned long t)
{
	struct foo *q;
	int *arr;
	int v, w;

	q = kmalloc(sizeof(struct foo), GFP_KERNEL);
	q = kzalloc(sizeof(*q), GFP_KERNEL);
	arr = kmalloc(n * sizeof(int), GFP_KERNEL);
	arr = kcalloc(sizeof(int), n, GFP_KERNEL);
	q = krealloc(q, 2 * sizeof(*q), GFP_KERNEL);
	q = (struct foo *)kmalloc(sizeof(*q), GFP_KERNEL);
	strcpy(dst, src);
	strlcpy(dst, src, n);
	strncpy(dst, src, n);
	memset(dst, n, 0);
	memset(dst, 0, 1);
	udelay(5000);
	udelay(20);
	msleep(1);
	usleep_range(10, 10);
	usleep_range(20, 10);
	if (jiffies > t)
		yield();
	if (in_atomic())
		return -ENOSYS;
	if (spin_is_locked(&p->lock))
		return EINVAL;
	smp_mb();
	v = simple_strtoul(src, NULL, 10);
	sscanf(src, "%d", &v);
	if (sscanf(src, "%d %d", &v, &w) != 2)
		return -EINVAL;
	v = min((int)v, (long)w);
	if (p)
		kfree(p);
	kfree(q);
	if (likely(v) > 0)
		return v;
	if (unlikely(IS_ERR(q)))
		return 0;
	v = (p->a)&(w)>>1;
	v = __constant_htons(v);
	kmap(NULL);
	return v;
}
EXPORT_SYMBOL(kapi);

int detached(void)
{
	return 0;
}

EXPORT_SYMBOL(detached);
MODULE_LICENSE("GPL v3");
MODULE_LICENSE("GPL");
