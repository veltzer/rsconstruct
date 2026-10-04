test_checker!(xmllint, tool: "xmllint", processor: "xmllint",
    files: [("test.xml", "<?xml version=\"1.0\"?>\n<root><item>test</item></root>\n")]);
