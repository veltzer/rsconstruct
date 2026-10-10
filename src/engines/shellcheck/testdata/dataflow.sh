#!/bin/bash
# Values the dataflow analysis tracks: whether an unquoted expansion needs
# quoting depends on what reaches it along every path.

clean=abc
echo $clean
spaced="a b"
echo $spaced
if [ -n "$1" ]; then
	only_then=1
fi
echo $only_then
if [ -n "$2" ]; then
	both=1
else
	both=2
fi
echo $both
if [ -z "$TMPDIR" ]; then TMPDIR=/tmp; else :; fi
work="$TMPDIR/work.$$"
echo $work
mkdir $work/sub
for i in 1 2 3; do
	last=$i
done
echo $last
while read -r line; do
	seen=$line
done < input
echo $seen
declare -i counter
counter=$1
echo $counter
num=$((1 + 2))
echo $num
setter() {
	from_function="x y"
}
setter
echo $from_function
caller() {
	echo $from_caller
}
from_caller=ok
caller
from_caller="not ok"
caller
case "$1" in
	a) choice=1 ;;
	b) choice="two words" ;;
	*) ;;
esac
echo $choice
(subshell_value=1)
echo $subshell_value
value=start
value="$value more"
echo $value
copy=$clean
echo $copy
joined="$clean$clean"
echo $joined
unset clean
echo $clean
false
status=$?
echo "$status"
ls /tmp
echo "$?"
mkdir -p "$work" && cd "$work" || exit
[ -d "$work" ]
echo $?
fail() {
	exit 1
}
fail
echo "after fail"
