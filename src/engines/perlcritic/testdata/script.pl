#!/usr/bin/perl
# A script with a little of everything the severity-5 policies look for.

print "starting\n";
use strict;
use warnings;

my %config = (verbose => 0, mode => 0755, -name => 'x');
my @files = glob '*.txt';
my @also = <*.log>;

open(LOG, ">>$0.log") or die;
opendir(DIR, '.') or die;
binmode STDOUT, ':utf8';

sub trim($) {
    my $s = shift;
    $s =~ s/^\s+|\s+$//g;
    return $s;
}

sub process {
    my @lines = @_;
    my @out = map { s/\r//; $_ } @lines;
    my $x = 1 if @out;
    for $line (@out) {
        print LOG trim($line), "\n";
    }
    return sort @out if wantarray;
    return undef;
}

if (-t STDIN) {
    select(undef, undef, undef, 0.25);
}

my $result = eval "process(\@files)";
my $obj = bless {};
chmod 0644, @files;
my $mask = 0022;

{
    no strict 'refs';
    *{"main::helper"} = sub { 1 };
}

sub outer {
    sub inner { return 1 }
    return inner();
}

## no critic (ProhibitTwoArgOpen)
open(OUT, ">out.txt");
## use critic
open(ERR, ">err.txt");

process(@files, @also);

__DATA__
data here
