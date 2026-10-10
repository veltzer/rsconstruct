return undef;
return sort @x;
my %h = (return => 1);
$h{return} = undef;
return (undef);
return sort { $a <=> $b } @x;
