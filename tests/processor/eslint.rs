test_checker!(eslint, tool: "eslint", processor: "eslint",
    files: [("eslint.config.js", "export default [];\n"), ("test.js", "var x = 1;\n")]);
