test_checker!(yq, tool: "yq", processor: "processor.checker.yq",
    files: [("test.yaml", "---\nname: test\nvalue: 42\n")]);
