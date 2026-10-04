test_checker!(htmlhint, tool: "htmlhint", processor: "processor.checker.htmlhint",
    files: [("test.html", "<!DOCTYPE html>\n<html><head><title>Test</title></head><body></body></html>\n")]);
