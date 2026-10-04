test_checker!(perlcritic, tool: "perlcritic", processor: "processor.checker.perlcritic",
    files: [("test.pl", "#!/usr/bin/perl\nuse strict;\nuse warnings;\nprint \"hello\\n\";\n")]);
