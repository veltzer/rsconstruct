eval "use Foo; 1";
eval { 1 };
eval $code;
eval;
eval 'require Bar';
$x->eval("x");
eval q{use Foo;};
