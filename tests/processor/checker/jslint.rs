test_checker!(jslint, tool: "jslint", processor: "processor.checker.jslint",
    files: [("test.js", "\"use strict\";\nvar x = 1;\n")]);
