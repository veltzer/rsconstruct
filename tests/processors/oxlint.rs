test_checker!(oxlint, tool: "oxlint", processor: "oxlint",
    files: [("test.js", "var x = 1;\nconsole.log(x);\n")]);
