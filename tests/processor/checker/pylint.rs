test_checker!(pylint, tool: "pylint", processor: "processor.checker.pylint",
    files: [("test.py", "\"\"\"Test module.\"\"\"\nX = 1\n")]);
