/*
 * No SPDX tag on the first line.
 */
// C99 comment style.
#include <linux/kernel.h> // trailing comment
/* SPDX-License-Identifier: GPL-2.0 */
/* SPDX-FileCopyrightText: someone */
/* SPDX-Unknown: tag */

/* Block comment
   without leading stars */

/*
 * Misaligned
  * stars
 */

/* trailing text */ int after_comment;

/* comment ends with text */
int value; /*
	    * inline block
	    */

void blank_lines(void)
{

	int a = 1;


	a++;

}

void fallthrough_comments(int x)
{
	switch (x) {
	case 1:
		x++;
		/* fallthrough */
	case 2:
		x++;
		/* FALLTHROUGH */
	default:
		;
	}
}

void dos_endings(void)
{
	return;
}
