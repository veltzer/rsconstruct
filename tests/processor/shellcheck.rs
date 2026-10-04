test_checker!(shellcheck, tool: "shellcheck", processor: "shellcheck",
    files: [("test.sh", "#!/bin/bash\necho \"hello\"\n")]);
