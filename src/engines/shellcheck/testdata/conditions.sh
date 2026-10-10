#!/bin/bash
# Tests, conditions and exit statuses.

a=$1
b=$2
if [ $a = $b ]; then
	echo same
fi
if [ "$a" == "$b" -a "$a" != "" ]; then
	echo both
fi
if [ "$a" -o "$b" ]; then
	echo either
fi
[ -n $a ] && echo set
[ -z "$a" ] || echo nonempty
[ "$a" ] && echo truthy
if [ "$a" > "$b" ]; then
	echo greater
fi
if [[ $a -eq "x" ]]; then
	echo eq
fi
if [[ "$a" =~ "^[0-9]+$" ]]; then
	echo number
fi
if [ 1 = 1 ]; then
	echo always
fi
if [[ -z "$a" && -n "$b" || "$a" == "c" ]]; then
	echo mixed
fi
mkdir /tmp/x
if [ $? -ne 0 ]; then
	echo failed
fi
grep -q foo file && echo found || echo missing
if grep foo file | wc -l; then
	echo counted
fi
if [ "$(grep -c foo file)" -gt 0 ]; then
	echo some
fi
if [ $(whoami) = root ]; then
	echo root
fi
[ "$a" = "b" ] && [ "$b" = "c" ] || [ "$a" = "d" ]
[[ $a == $b ]]
[[ "$a" = *.txt ]] && echo text
[ "$a" = *.txt ] && echo glob
[[ $a > 5 ]]
[ "$a" -gt 3.5 ]
test "$a" = "$b" -a 1 = 1
if ! [ "$a" ]; then :; fi
if [ ! -f "$a" -a -d "$b" ]; then :; fi
while [ "$a" -lt 10 ]; do
	a=$((a + 1))
done
case $a in
	*) echo default ;;
esac
until false; do break; done
[ "$a" =~ foo ]
[[ "$a" -ge "10" ]]
[ "$a" == "b" ]
[ -e "$a" -o -e "$b" ]
true && false || true
