simple_checker!(DoctestProcessor, crate::config::DoctestConfig,
    "Run Python doctests",
    crate::processors::names::DOCTEST,
    tools: ["python3".to_string()],
    prepend_args: ["-m", "doctest"],
);
