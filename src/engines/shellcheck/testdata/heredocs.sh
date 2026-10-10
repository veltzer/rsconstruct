#!/bin/bash
# Here documents, redirections and subshells.

name=$1
cat <<EOF
Hello $name
Unset: $nobody
EOF
cat <<'EOF'
Literal $name
EOF
cat <<-EOF
	Indented $name
	EOF
ssh host <<EOF
echo $name
EOF
cat <<EOF | grep Hello
Hello $name
EOF
cat << EOF > out.txt
written $name
EOF
cat <<"END"
quoted end $name
END
cat <<EOF
mismatch
 EOF
EOF
exec 3> file
echo "to fd" >&3
exec 3>&-
cmd 2>&1 > /dev/null
cmd > /dev/null 2>&1
cmd &> /dev/null
cmd >& /dev/null
echo hi > "$name" < "$name"
{ echo a; echo b; } > both
(cd /tmp && ls)
out=$(cd /tmp; ls)
echo "$out"
echo "$(< file)"
cat < <(ls)
while read -r l; do echo "$l"; done < <(ls)
x=$(cat <<EOF
inside $name
EOF
)
echo "$x"
cat <<EOF1 <<EOF2
one
EOF1
two
EOF2
true >| clobbered
read -r v < /dev/stdin
echo "$v"
