simple_checker!(SvglintProcessor, crate::config::SvglintConfig,
    "Lint SVG files with svglint",
    crate::processors::names::SVGLINT,
    tools: ["svglint".to_string()],
);
