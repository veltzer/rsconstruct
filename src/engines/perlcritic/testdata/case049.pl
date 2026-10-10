map { +{ a => 1 } } @x; map {; $_ => 1 } @x; map { ($_ => 1) } @x; map { lc($_) => 1 } @x; grep { $_ } @x;
