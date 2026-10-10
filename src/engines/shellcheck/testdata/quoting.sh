#!/bin/bash
# Quoting, word splitting and globbing.

dir=$1
files=$(ls "$dir")
echo $files
cp $dir/* /tmp
rm -rf "$dir"/$2
for f in $(find "$dir" -name '*.txt'); do
	echo "$f"
done
echo `date`
echo "Today is `date +%F`"
echo 'Home is $HOME'
echo "The answer is "$((6 * 7))
echo "path: "$dir"/sub"
args="-l -a"
ls $args
cmd="ls -l 'my file'"
$cmd
list=(one two three)
echo $list
echo "${list[@]}"
echo ${list[*]}
printf '%s\n' "Items: $@"
items="$@"
set -- $items
arr=( $(seq 1 3) )
arr2=($dir)
echo "${#arr[@]} ${#arr2[@]}"
x=( "a" "b" )
x="c"
echo "$x"
eval echo $dir
find . -name *.log
grep -r foo $dir/*.c
[ -e $dir/$file ] && echo yes
echo ${dir:-"default value"}
echo "${dir//\// }"
echo "$dir"'s'
var="$( echo "$dir" )"
echo $var
trap "echo $dir" EXIT
ssh host "echo $dir"
echo ~/"$dir"
echo "~/$dir"
a=~
b="~"
echo "$a$b"
