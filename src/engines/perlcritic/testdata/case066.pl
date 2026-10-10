use feature qw(say signatures);
sub f ($x) {}
{ no feature 'signatures'; sub g ($y) {} }
sub h ($z) {}
