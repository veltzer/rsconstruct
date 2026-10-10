#!/bin/bash
# What the optional checks (enable=all) find, and nothing else.
set -e

name="$1"
if [ "$name" ]; then
	echo "$name"
fi
if [ -n "$name" ]; then
	echo ok
fi
if ! [ -z "$name" ]; then
	echo negated
fi
case "$name" in
	a) echo a ;;
esac
echo $name
echo "$UPPER_UNSET"
which ls
cat file | wc -l
check() {
	false
	echo "after false"
}
if check; then echo checked; fi
check || echo failed
result=$(check)
echo "$result $(check)"
x=$(false)
echo "$x"
[ -e "$name" ] && echo exists
echo "${name}"
