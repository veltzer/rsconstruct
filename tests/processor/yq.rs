test_checker!(yq, tool: "yq", processor: "yq",
    files: [("test.yaml", "---\nname: test\nvalue: 42\n")]);
