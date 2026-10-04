test_checker!(pylint, tool: "pylint", processor: "pylint",
    files: [("test.py", "\"\"\"Test module.\"\"\"\nX = 1\n")]);
