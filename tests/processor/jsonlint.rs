test_checker!(jsonlint, tool: "jsonlint", processor: "jsonlint",
    files: [("test.json", "{\"name\": \"test\", \"value\": 42}\n")]);
