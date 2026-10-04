test_checker!(standard, tool: "standard", processor: "processor.checker.standard",
    files: [("test.js", "var x = 1\nconsole.log(x)\n")]);
