/// Central processor registry — the single source of truth for all built-in processors.
///
/// Adding a new processor requires adding ONE line here. All wiring code
/// (name constants, config struct fields, default lists, builder registration, etc.)
/// is auto-generated from this table by consumer macros.
///
/// Each entry: `CONST_NAME, field_name, ConfigType, ProcessorType, (scan_dir, extensions, exclude_dirs);`
///
/// - `CONST_NAME`: used for `processors::names::CONST_NAME` constant (value = stringify!(field_name))
/// - `field_name`: the field name in `ProcessorConfig` and the TOML section name `[processor.field_name]`
/// - `ConfigType`: the config struct (must impl Default, Serialize, Deserialize, Clone, KnownFields)
/// - `ProcessorType`: the processor struct (must impl ProductDiscovery, have `fn new(config) -> Self`)
/// - `(scan_dir, extensions, exclude_dirs)`: arguments to `ScanConfig::resolve()` for defaults
macro_rules! for_each_processor {
    ($callback:ident) => {
        $callback! {
            TERA,           tera,           TeraConfig,            TeraProcessor,            ("tera.templates", &[".tera"], &[]);
            RUFF,           ruff,           RuffConfig,            RuffProcessor,            ("", &[".py"], PYTHON_EXCLUDE_DIRS);
            PYLINT,         pylint,         PylintConfig,          PylintProcessor,          ("", &[".py"], PYTHON_EXCLUDE_DIRS);
            MYPY,           mypy,           MypyConfig,            MypyProcessor,            ("", &[".py"], PYTHON_EXCLUDE_DIRS);
            PYREFLY,        pyrefly,        PyreflyConfig,         PyreflyProcessor,         ("", &[".py"], PYTHON_EXCLUDE_DIRS);
            BLACK_CHECK,    black_check,    BlackCheckConfig,      BlackCheckProcessor,      ("", &[".py"], PYTHON_EXCLUDE_DIRS);
            DOCTEST,        doctest,        DoctestConfig,         DoctestProcessor,         ("", &[".py"], PYTHON_EXCLUDE_DIRS);
            PYTEST,         pytest,         PytestConfig,          PytestProcessor,          ("tests", &[".py"], PYTHON_EXCLUDE_DIRS);
            CC_SINGLE_FILE, cc_single_file, CcSingleFileConfig,    CcSingleFileProcessor,    ("src", &[".c", ".cc"], &[]);
            CC,             cc,             CcConfig,              CcProcessor,              ("", &["cc.yaml"], CC_EXCLUDE_DIRS);
            CPPCHECK,       cppcheck,       CppcheckConfig,        CppcheckProcessor,        ("src", &[".c", ".cc"], CC_EXCLUDE_DIRS);
            CLANG_TIDY,     clang_tidy,     ClangTidyConfig,       ClangTidyProcessor,       ("src", &[".c", ".cc"], CC_EXCLUDE_DIRS);
            ZSPELL,         zspell,         ZspellConfig,          ZspellProcessor,          ("", &[".md"], ZSPELL_EXCLUDE_DIRS);
            SHELLCHECK,     shellcheck,     ShellcheckConfig,      ShellcheckProcessor,      ("", &[".sh", ".bash"], SHELL_EXCLUDE_DIRS);
            LUACHECK,       luacheck,       LuacheckConfig,        LuacheckProcessor,        ("", &[".lua"], BUILD_TOOL_EXCLUDES);
            MAKE,           make,           MakeConfig,            MakeProcessor,            ("", &["Makefile"], MAKE_CARGO_EXCLUDES);
            CARGO,          cargo,          CargoConfig,           CargoProcessor,           ("", &["Cargo.toml"], MAKE_CARGO_EXCLUDES);
            CLIPPY,         clippy,         ClippyConfig,          ClippyProcessor,          ("", &["Cargo.toml"], MAKE_CARGO_EXCLUDES);
            RUMDL,          rumdl,          RumdlConfig,           RumdlProcessor,           ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            YAMLLINT,       yamllint,       YamllintConfig,        YamllintProcessor,        ("", &[".yml", ".yaml"], BUILD_TOOL_EXCLUDES);
            JQ,             jq,             JqConfig,              JqProcessor,              ("", &[".json"], BUILD_TOOL_EXCLUDES);
            JSONLINT,       jsonlint,       JsonlintConfig,        JsonlintProcessor,        ("", &[".json"], BUILD_TOOL_EXCLUDES);
            TAPLO,          taplo,          TaploConfig,           TaploProcessor,           ("", &[".toml"], BUILD_TOOL_EXCLUDES);
            JSON_SCHEMA,    json_schema,    JsonSchemaConfig,      JsonSchemaProcessor,      ("", &[".json"], BUILD_TOOL_EXCLUDES);
            TAGS,           tags,           TagsConfig,            TagsProcessor,            ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            PIP,            pip,            PipConfig,             PipProcessor,             ("", &["requirements.txt"], MAKE_CARGO_EXCLUDES);
            SPHINX,         sphinx,         SphinxConfig,          SphinxProcessor,          ("", &["conf.py"], BUILD_TOOL_EXCLUDES);
            MDBOOK,         mdbook,         MdbookConfig,          MdbookProcessor,          ("", &["book.toml"], BUILD_TOOL_EXCLUDES);
            NPM,            npm,            NpmConfig,             NpmProcessor,             ("", &["package.json"], MAKE_CARGO_EXCLUDES);
            GEM,            gem,            GemConfig,             GemProcessor,             ("", &["Gemfile"], MAKE_CARGO_EXCLUDES);
            MDL,            mdl,            MdlConfig,             MdlProcessor,             ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            MARKDOWNLINT,   markdownlint,   MarkdownlintConfig,    MarkdownlintProcessor,    ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            ASPELL,         aspell,         AspellConfig,          AspellProcessor,          ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            MARP,           marp,           MarpConfig,            MarpProcessor,            ("marp", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            PANDOC,         pandoc,         PandocConfig,          PandocProcessor,          ("pandoc", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            MARKDOWN,       markdown,       MarkdownConfig,        MarkdownProcessor,        ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            PDFLATEX,       pdflatex,       PdflatexConfig,        PdflatexProcessor,        ("", &[".tex"], BUILD_TOOL_EXCLUDES);
            A2X,            a2x,            A2xConfig,             A2xProcessor,             ("", &[".txt"], BUILD_TOOL_EXCLUDES);
            ASCII,    ascii,    AsciiConfig,      AsciiProcessor,      ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            TERMS,          terms,          TermsConfig,           TermsProcessor,           ("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
            CHROMIUM,       chromium,       ChromiumConfig,        ChromiumProcessor,        ("out/marp", &[".html"], BUILD_TOOL_EXCLUDES);
            MAKO,           mako,           MakoConfig,            MakoProcessor,            ("templates.mako", &[".mako"], &[]);
            JINJA2,         jinja2,         Jinja2Config,          Jinja2Processor,          ("templates.jinja2", &[".j2"], &[]);
            MERMAID,        mermaid,        MermaidConfig,         MermaidProcessor,         ("", &[".mmd"], BUILD_TOOL_EXCLUDES);
            DRAWIO,         drawio,         DrawioConfig,          DrawioProcessor,          ("", &[".drawio"], BUILD_TOOL_EXCLUDES);
            LIBREOFFICE,    libreoffice,    LibreofficeConfig,     LibreofficeProcessor,     ("", &[".odp"], BUILD_TOOL_EXCLUDES);
            PROTOBUF,       protobuf,       ProtobufConfig,        ProtobufProcessor,        ("proto", &[".proto"], &[]);
            PDFUNITE,       pdfunite,       PdfuniteConfig,        PdfuniteProcessor,        ("", &["course.yaml"], BUILD_TOOL_EXCLUDES);
            SCRIPT,   script,   ScriptConfig,     ScriptProcessor,     ("", &[], &[]);
            GENERATOR,  generator,  GeneratorConfig,   GeneratorProcessor,   ("", &[], &[]);
            LINUX_MODULE,   linux_module,   LinuxModuleConfig,     LinuxModuleProcessor,     ("", &["linux-module.yaml"], BUILD_TOOL_EXCLUDES);
            CPPLINT,        cpplint,        CpplintConfig,         CpplintProcessor,         ("src", &[".c", ".cc", ".h", ".hh"], CC_EXCLUDE_DIRS);
            CHECKPATCH,     checkpatch,     CheckpatchConfig,      CheckpatchProcessor,      ("src", &[".c", ".h"], CC_EXCLUDE_DIRS);
            OBJDUMP,        objdump,        ObjdumpConfig,         ObjdumpProcessor,         ("out/cc_single_file", &[".elf"], &[]);
            ESLINT,         eslint,         EslintConfig,          EslintProcessor,          ("", &[".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"], BUILD_TOOL_EXCLUDES);
            JSHINT,         jshint,         JshintConfig,          JshintProcessor,          ("", &[".js", ".jsx", ".mjs", ".cjs"], BUILD_TOOL_EXCLUDES);
            HTMLHINT,       htmlhint,       HtmlhintConfig,        HtmlhintProcessor,        ("", &[".html", ".htm"], BUILD_TOOL_EXCLUDES);
            TIDY,           tidy,           TidyConfig,            TidyProcessor,            ("", &[".html", ".htm"], BUILD_TOOL_EXCLUDES);
            STYLELINT,      stylelint,      StylelintConfig,       StylelintProcessor,       ("", &[".css", ".scss", ".sass", ".less"], BUILD_TOOL_EXCLUDES);
            JSLINT,         jslint,         JslintConfig,          JslintProcessor,          ("", &[".js"], BUILD_TOOL_EXCLUDES);
            STANDARD,       standard,       StandardConfig,        StandardProcessor,        ("", &[".js"], BUILD_TOOL_EXCLUDES);
            HTMLLINT,       htmllint,       HtmllintConfig,        HtmllintProcessor,        ("", &[".html", ".htm"], BUILD_TOOL_EXCLUDES);
            PHP_LINT,       php_lint,       PhpLintConfig,         PhpLintProcessor,         ("", &[".php"], BUILD_TOOL_EXCLUDES);
            PERLCRITIC,     perlcritic,     PerlcriticConfig,      PerlcriticProcessor,      ("", &[".pl", ".pm"], BUILD_TOOL_EXCLUDES);
            XMLLINT,        xmllint,        XmllintConfig,         XmllintProcessor,         ("", &[".xml"], BUILD_TOOL_EXCLUDES);
            CHECKSTYLE,     checkstyle,     CheckstyleConfig,      CheckstyleProcessor,      ("", &[".java"], BUILD_TOOL_EXCLUDES);
            YQ,             yq,             YqConfig,              YqProcessor,              ("", &[".yml", ".yaml"], BUILD_TOOL_EXCLUDES);
            CMAKE,          cmake,          CmakeConfig,           CmakeProcessor,           ("", &["CMakeLists.txt"], BUILD_TOOL_EXCLUDES);
            HADOLINT,       hadolint,       HadolintConfig,        HadolintProcessor,        ("", &["Dockerfile"], BUILD_TOOL_EXCLUDES);
            JEKYLL,         jekyll,         JekyllConfig,          JekyllProcessor,          ("", &["_config.yml"], BUILD_TOOL_EXCLUDES);
            SASS,           sass,           SassConfig,            SassProcessor,            ("sass", &[".scss", ".sass"], &[]);
            SLIDEV,         slidev,         SlidevConfig,          SlidevProcessor,          ("", &[".md"], BUILD_TOOL_EXCLUDES);
        }
    };
}
