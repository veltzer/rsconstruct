simple_checker!(PytestProcessor, crate::config::PytestConfig,
    "Run Python tests with pytest",
    crate::processors::names::PYTEST,
    tools: ["pytest".to_string(), "python3".to_string()],
);
