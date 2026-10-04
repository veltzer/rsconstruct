test_checker!(mypy, tool: "mypy", processor: "mypy",
    files: [("test.py", "x: int = 1\n")]);
