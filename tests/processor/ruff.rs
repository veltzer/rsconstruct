test_checker!(ruff, tool: "ruff", processor: "ruff",
    files: [("test.py", "x = 1\n")]);
