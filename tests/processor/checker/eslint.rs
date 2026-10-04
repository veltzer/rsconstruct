test_checker!(eslint, tool: "eslint", processor: "processor.checker.eslint",
    files: [("eslint.config.js", "export default [];\n"), ("test.js", "var x = 1;\n")]);
