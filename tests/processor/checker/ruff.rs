test_checker!(ruff, tool: "ruff", processor: "processor.checker.ruff",
    files: [("test.py", "x = 1\n")]);
