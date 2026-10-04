test_checker!(stylelint, tool: "stylelint", processor: "stylelint",
    files: [(".stylelintrc.json", "{\"rules\": {}}\n"), ("test.css", "body {\n  color: red;\n}\n")]);
