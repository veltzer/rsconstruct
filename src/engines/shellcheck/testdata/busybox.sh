#!/bin/busybox sh
# busybox sh: ash with a few extensions.
local x=1
echo "$x"
[[ $1 == a ]] && echo a
echo ${1:0:1}
echo $RANDOM
arr=(1 2)
source ./lib.sh
echo -e "\t"
