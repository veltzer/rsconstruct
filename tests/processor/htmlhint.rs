test_checker!(htmlhint, tool: "htmlhint", processor: "htmlhint",
    files: [("test.html", "<!DOCTYPE html>\n<html><head><title>Test</title></head><body></body></html>\n")]);
