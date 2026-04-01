simple_checker!(BlackCheckProcessor, crate::config::BlackCheckConfig,
    "Check Python formatting with black",
    crate::processors::names::BLACK_CHECK,
    tools: ["black".to_string(), "python3".to_string()],
    prepend_args: ["--check"],
);
