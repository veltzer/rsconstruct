test_checker!(checkstyle, tool: "checkstyle", processor: "checkstyle",
    config: "[processor.checkstyle]\nargs = [\"-c\", \"/google_checks.xml\"]\nsrc_dirs = [\".\"]\n",
    files: [("Test.java", "public class Test {\n    public static void main(String[] args) {\n    }\n}\n")]);
