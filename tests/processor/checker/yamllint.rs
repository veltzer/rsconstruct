test_checker!(yamllint, tool: "yamllint", processor: "processor.checker.yamllint",
    files: [("test.yaml", "---\nname: test\nvalue: 42\n")]);
