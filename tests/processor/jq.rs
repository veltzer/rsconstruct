test_checker!(jq, tool: "jq", processor: "jq",
    files: [("test.json", "{\"name\": \"test\", \"value\": 42}\n")]);
