test_checker!(jslint, tool: "jslint", processor: "jslint",
    files: [("test.js", "\"use strict\";\nvar x = 1;\n")]);
