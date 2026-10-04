test_checker!(htmllint, tool: "htmllint", processor: "processor.checker.htmllint",
    files: [(".htmllintrc", "{}\n"), ("test.html", "<!DOCTYPE html>\n<html><head><title>Test</title></head><body></body></html>\n")]);
