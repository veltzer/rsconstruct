select(undef, undef, undef, 0.25);
select undef, undef, undef, $t;
select(undef, undef, undef, undef);
select(STDOUT);
select($r, undef, undef, 1);
