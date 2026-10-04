test_checker!(jsonlint, tool: "jsonlint", processor: "processor.checker.jsonlint",
    files: [("test.json", "{\"name\": \"test\", \"value\": 42}\n")]);
