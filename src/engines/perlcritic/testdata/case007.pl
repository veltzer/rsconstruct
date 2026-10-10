sub foo : lvalue { }
sub bar :prototype($) { 1 }
sub baz : method lvalue {}
my $s = sub : lvalue { 1 };
