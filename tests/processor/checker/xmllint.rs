test_checker!(xmllint, tool: "xmllint", processor: "processor.checker.xmllint",
    files: [("test.xml", "<?xml version=\"1.0\"?>\n<root><item>test</item></root>\n")]);
