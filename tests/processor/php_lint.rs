test_checker!(php_lint, tool: "php", processor: "php_lint",
    files: [("test.php", "<?php\necho \"hello\";\n")]);
