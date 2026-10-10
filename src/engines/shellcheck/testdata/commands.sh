#!/bin/bash
# Command usage the Commands module checks.

read name
read -p "Value: " value
echo "$name $value"
cd /tmp
cd "$HOME" || exit
for f in $(ls *.txt); do
	echo "$f"
done
ls -l | grep foo
ps aux | grep myproc
cat file | grep pattern
echo $(date)
echo "$(echo hi)"
grep foo file | wc -l
echo "$name" | sed 's/a/b/'
x=$(expr 1 + 2)
printf "Hello $name\n"
printf '%s %s\n' "$x"
echo "line1\nline2"
echo "first" > log
echo "second" >> log
echo "third" >> log
echo "fourth" >> log
sort data > data
trap "rm -f $name" EXIT
trap 'echo' 9
for line in $(cat file); do
	echo "$line"
done
for f in $(find . -type f); do
	echo "$f"
done
find . -name '*.o' | xargs rm
ls *.c | xargs wc -l
rm -- *
rm *
var=ls -la
exec /bin/true
echo "never"
echo foo | rm
du -s --max-depth=1
find . -type f -name '*.sh' -or -name '*.bash' -exec chmod +x {} \;
mkdir -p -m 700 /tmp/a/b
sudo echo hi > /root/file
tr 'a-z' 'A-Z' < file
tr [a-z] [A-Z] < file
cp * /tmp
ln -s file link
su -c whoami
wait $!
timeout 5 cd /tmp
nohup cd /tmp
xargs -0 echo < file
ssh host ls "$name"
seq 1 10 | while read -r i; do ssh host "echo $i"; done
egrep foo file
fgrep bar file
which ls
grep '*foo*' file
grep "$name"* file
dig +short example.com
set -e
alias ll='ls -l $1'
time -p ls
local outside=1
eval "$name"
command -v foo >/dev/null 2>&1
foo >/dev/null 2>&1 &
exit 300
