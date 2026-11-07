test_checker!(doctest, tool: "python", processor: "processor.checker.doctest",
    files: [("example.py", "def add(a, b):\n    \"\"\"\n    >>> add(1, 2)\n    3\n    \"\"\"\n    return a + b\n")]);
