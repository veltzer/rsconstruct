#!/bin/ksh
# ksh: arrays and [[ ]] are fine, some bash forms are not.
typeset -A map
map[a]=1
print -r -- "${map[a]}"
function f {
	typeset x=$1
	echo $x
}
f "$@"
echo ${.sh.version}
readarray lines < file
echo "${lines[0]}"
[[ $1 == +(a|b) ]] && echo match
echo $"msg"
whence -v ls
