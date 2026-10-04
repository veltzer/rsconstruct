test_checker!(shellcheck, tool: "shellcheck", processor: "processor.checker.shellcheck",
    files: [("test.sh", "#!/bin/bash\necho \"hello\"\n")]);
