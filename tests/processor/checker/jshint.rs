test_checker!(jshint, tool: "jshint", processor: "processor.checker.jshint",
    files: [("test.js", "var x = 1;\n")]);
