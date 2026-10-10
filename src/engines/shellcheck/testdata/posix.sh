#!/bin/sh
# Bashisms in a POSIX sh script.

f() {
	local x=1
	echo "$x"
}
f
echo -n "no newline"
echo -e "a\tb"
source ./lib.sh
if [[ -n "$1" ]]; then
	echo set
fi
ls &> /dev/null
v=a
v+=b
echo "$RANDOM $v"
set -o pipefail
declare -i n=1
echo "$n"
trap 'echo err' ERR
arr=(1 2 3)
echo "${arr[0]}"
s=hello
echo "${s:1:2}"
echo "${s^^}"
echo $"localized"
i=0
((i++))
echo "$((i ** 2))"
read -a words
echo "$words"
cat <<< "here string"
diff <(ls a) <(ls b)
function g { :; }
g
select opt in a b; do echo "$opt"; done
echo {1..5}
echo "$((16#ff))"
[ "$s" == "hello" ]
type -p ls
printf -v out '%s' "$s"
kill -SIGTERM $$
echo "${!s}"
echo "${s/l/L}"
time ls
ulimit -c unlimited
shopt -s nullglob
pushd /tmp
popd
let n=n+1
export -f f
coproc cat
wait -n
