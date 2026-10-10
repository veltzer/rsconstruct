use Foo { a => 1 };
use constant { A => 1 };
use Foo 1.0 { b => 2 };
use Foo qw(a);
no Foo 1 (x);
