test_checker!(jq, tool: "jq", processor: "processor.checker.jq",
    files: [("test.json", "{\"name\": \"test\", \"value\": 42}\n")]);
