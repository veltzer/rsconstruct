test_checker!(stylelint, tool: "stylelint", processor: "processor.checker.stylelint",
    files: [(".stylelintrc.json", "{\"rules\": {}}\n"), ("test.css", "body {\n  color: red;\n}\n")]);
