test_checker!(yamllint, tool: "yamllint", processor: "yamllint",
    files: [("test.yaml", "---\nname: test\nvalue: 42\n")]);
