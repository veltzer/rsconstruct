#!/bin/dash
# dash: a few extensions are allowed, most bashisms are not.
f() {
	local v=1
	echo "$v"
}
f
echo -n x
echo "${#1}"
[[ -n $1 ]] && echo set
arr=(a b)
echo $RANDOM
printf "%q" "$1"
