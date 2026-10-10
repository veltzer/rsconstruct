#!/bin/bash
# Arithmetic, loops and case statements.

n=$1
echo $(($n + 1))
echo $[n + 1]
echo $((n++))
let n++
(( n = n + 1 ))
i=0
x = 5
total=$((total + ${#n}))
echo "$(( 1 / 0 ))"
echo "$(( 08 + 1 ))"
for i in 1 2 3
do
	cd "$i"
	echo "$i"
done
for i in "$@"; do echo "$i"; done
for i in "1 2 3"; do echo "$i"; done
for i in 1; do echo "$i"; done
for i in {1..$n}; do echo "$i"; done
for ((j = 0; j < n; j++)); do
	echo "$j"
done
for f in *.txt; do
	[ -e "$f" ] || continue
	echo "$f"
done
while read -r line; do
	echo "$line"
done < input
while true; do
	break 2
done
break
continue
case "$n" in
	start) echo start ;;
	stop | halt) echo stop ;;
	start) echo again ;;
	*) echo other ;;
	-*) echo option ;;
esac
case $n in
	[0-9]*) echo digit ;;
	"*") echo star ;;
	$i) echo var ;;
	a|b|a) echo dup ;;
esac
case "$n" in
	1) echo one;;
	2) echo two
esac
echo $((n % 2 == 0 ? 1 : 0))
echo $(( ${n} * 2 ))
echo "$(( 2 ** 64 ))"
echo $(( n << 2 ))
[ $((n)) -gt 1 ]
((n > 1)) && echo big
y=$(( $(wc -l < file) + 1 ))
echo "$y"
seq 1 "$n" | while read -r k; do echo "$k"; done
until [ "$n" -le 0 ]; do n=$((n - 1)); done
select choice in yes no; do
	echo "$choice"
	break
done
