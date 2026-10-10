sub outer { sub inner { } my $x = sub { }; my sub lex { } BEGIN { } }
sub a1 { { sub b1 { } } }
