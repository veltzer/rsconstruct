package Foo::Inventory;

use strict;
use warnings;
use Carp qw(croak);
use List::Util qw(sum first);

our $VERSION = '1.02';

=head1 NAME

Foo::Inventory - a small inventory, for the tests

=head1 SYNOPSIS

    my $inv = Foo::Inventory->new(name => 'shop');
    $inv->add(apple => 3);

=cut

sub new {
    my ($class, %args) = @_;
    my $self = bless { name => $args{name} || 'default', items => {} }, $class;
    return $self;
}

sub add {
    my ($self, $item, $count) = @_;
    croak "no item" unless defined $item;
    $self->{items}{$item} += $count // 1;
    return $self;
}

sub total {
    my ($self) = @_;
    return sum(values %{ $self->{items} }) || 0;
}

sub names {
    my $self = shift;
    my @names = sort { lc($a) cmp lc($b) } keys %{ $self->{items} };
    return @names;
}

sub report {
    my ($self, $fh) = @_;
    my $header = <<"END";
Inventory: $self->{name}
=========
END
    print {$fh} $header;
    for my $name ($self->names) {
        printf {$fh} "%-20s %5d\n", $name, $self->{items}{$name};
    }
    my $big = first { $self->{items}{$_} > 10 } $self->names;
    print {$fh} "biggest: $big\n" if defined $big;
    return;
}

sub load {
    my ($self, $path) = @_;
    open my $in, '<:encoding(UTF-8)', $path or croak "open $path: $!";
    while (my $line = <$in>) {
        chomp $line;
        next if $line =~ /^\s*(?:#|$)/;
        my ($item, $count) = split /\s*=\s*/, $line, 2;
        $self->add($item, $count);
    }
    close $in or croak "close $path: $!";
    return $self;
}

sub clean {
    my ($self) = @_;
    my @kept = grep { $self->{items}{$_} } keys %{ $self->{items} };
    my %copy = map { ($_ => $self->{items}{$_}) } @kept;
    $self->{items} = \%copy;
    return scalar @kept;
}

1;

__END__

=head1 AUTHOR

The tests.

=cut
