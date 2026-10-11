// SPDX-License-Identifier: GPL-2.0
#include <linux/kernel.h>
#include <linux/printk.h>
#include <linux/seq_file.h>
#include <linux/sysfs.h>

void logging(struct device *dev, struct seq_file *m, char *buf, int x)
{
	printk("no level\n");
	printk(KERN_INFO "with level\n");
	printk_once(KERN_WARNING "once\n");
	dev_printk(KERN_DEBUG, dev, "dev\n");
	pr_info(KERN_INFO "redundant level\n");
	pr_cont("continued\n");
	printk(KERN_CONT "more\n");
	pr_err("%Lx and %Zd\n", 1ULL, 2);
	pr_err("0x%d\n", x);
	pr_debug("%hhx %hd\n", x, x);
	pr_info("%s: entered\n", __func__);
	pr_info("in logging, %s\n", "x");
	pr_info("value is %d \n", x);
	pr_info("split "
		"string\n");
	pr_info("no space"
		"here\n");
	pr_info("A" "B");
	pr_info("C"D);
	pr_info("%px and %pQ and %pf\n", buf, buf, buf);
	seq_printf(m, "constant\n");
	seq_printf(m, "%d\n", x);
	trace_printk("trace\n");
	printk_ratelimit();
	WARN_ON(x);
	BUG_ON(x);
	if (!buf)
		pr_err("out of memory\n");
}

ssize_t show(struct device *dev, char *buf)
{
	return sysfs_emit(buf, "%d", 1);
}

ssize_t show2(struct device *dev, char *buf)
{
	return sysfs_emit(buf, "%d\n", 1);
}

static const char *msg = "a very long line that goes on and on and on and on and on and on and on and beyond the limit";
static const char *url = "see https://example.com/a/very/long/path/that/keeps/going/and/going/and/going/past/the/limit";
/* $Id: strings.c,v 1.1 $ */
/* This comment has the the repeated word. */
/*
 * Please write to the Free Software Foundation, Inc., 59 Temple Place.
 */
const char *date = __DATE__ " " __TIME__;
