#!/bin/bash
# shellcheck disable=SC2034
# Directives: file-wide, per command, per block, sourcing.

unused=1
# shellcheck disable=SC2086
echo $1
echo $2
# shellcheck disable=SC2086,SC2046
{
	echo $3
	echo $(ls)
}
# shellcheck disable=SC2154
f() {
	echo "$undefined"
}
f
echo "$also_undefined"
# shellcheck source=lib.sh
source ./lib.sh
# shellcheck source=/dev/null
. "$CONFIG"
. ./missing.sh
# shellcheck disable=all
echo $4
echo $5
# shellcheck enable=require-variable-braces
echo "$HOME"
# shellcheck disable=SC9999
echo "$6"
# shellcheck disable=2086
echo $7
# shellcheck disable=SC2086 # with a reason
echo $8
# shellcheck foo=bar
echo "$9"
# shellcheck disable=SC1000-SC2999
echo ${10}
