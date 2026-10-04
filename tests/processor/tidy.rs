test_checker!(tidy, tool: "tidy", processor: "tidy",
    files: [("test.html", "<!DOCTYPE html>\n<html><head><title>Test</title></head><body><p>Hello</p></body></html>\n")]);
