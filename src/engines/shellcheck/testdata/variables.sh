#!/bin/bash
# Assignments and uses: what the dataflow analysis tracks.

unused_var=1
used_var=2
echo "$used_var"
echo "$never_assigned"
echo "$used_vra"
local_outside=3
export PATH="$PATH:/opt/bin"
export $used_var
declare -r ro=1
ro=2
readonly constant=$(date)
local x=1
count=0
printf '%s\n' a b c | while read -r line; do
	count=$((count + 1))
	echo "$line"
done
echo "$count"
(
	inside=1
)
echo "$inside"
greet() {
	echo "Hello $1"
}
greet
shout() {
	echo "HEY"
}
shout "$@"
helper() {
	local result
	result=$(compute)
	echo "$result"
}
never_called() {
	echo "never"
}
compute() {
	return 1
	echo "unreachable"
}
helper
self=1
self=$self
SOME_UPPER=1
echo "$SOME_UPPR"
: "${config_file:=/etc/app.conf}"
echo "$config_file"
read -r first second
echo "$first"
for unused_loop in 1 2 3; do
	:
done
array[0]=1
array[1]=2
echo "${array[0]}"
declare -A assoc
assoc[key]=value
echo "${assoc[key]}"
typeset -i number=5
number=number+1
echo "$number"
v=1 echo "$v"
PS1='$ '
IFS=, read -r -a parts <<< "a,b"
echo "${parts[1]}"
let total=1+2
echo "$total"
unset used_var
echo "$used_var"
f() { g=1; }
f
echo "$g"
exit 0
echo "after exit"
