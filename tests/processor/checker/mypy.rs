test_checker!(mypy, tool: "mypy", processor: "processor.checker.mypy",
    files: [("test.py", "x: int = 1\n")]);
