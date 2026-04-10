/// Central processor registry — the single source of truth for all built-in processors.
///
/// Adding a new processor requires adding ONE line here. All wiring code
/// (name constants, config struct fields, default lists, builder registration, etc.)
/// is auto-generated from this table by consumer macros.
///
/// Each entry: `CONST_NAME, field_name, ConfigType, ProcessorType;`
///
/// - `CONST_NAME`: used for `processors::names::CONST_NAME` constant (value = stringify!(field_name))
/// - `field_name`: the field name in `ProcessorConfig` and the TOML section name `[processor.field_name]`
/// - `ConfigType`: the config struct (must impl Default, Serialize, Deserialize, Clone, KnownFields)
/// - `ProcessorType`: the processor struct (must impl ProductDiscovery, have `fn new(config) -> Self`)
macro_rules! for_each_processor {
    ($callback:ident) => {
        $callback! {
            TERA,           tera,           TeraConfig,            TeraProcessor;
            RUFF,           ruff,           RuffConfig,            RuffProcessor;
            PYLINT,         pylint,         PylintConfig,          PylintProcessor;
            MYPY,           mypy,           MypyConfig,            MypyProcessor;
            PYREFLY,        pyrefly,        PyreflyConfig,         PyreflyProcessor;
            BLACK,          black,          BlackConfig,           BlackProcessor;
            DOCTEST,        doctest,        DoctestConfig,         DoctestProcessor;
            PYTEST,         pytest,         PytestConfig,          PytestProcessor;
            CC_SINGLE_FILE, cc_single_file, CcSingleFileConfig,    CcSingleFileProcessor;
            CC,             cc,             CcConfig,              CcProcessor;
            CPPCHECK,       cppcheck,       CppcheckConfig,        CppcheckProcessor;
            CLANG_TIDY,     clang_tidy,     ClangTidyConfig,       ClangTidyProcessor;
            ZSPELL,         zspell,         ZspellConfig,          ZspellProcessor;
            SHELLCHECK,     shellcheck,     ShellcheckConfig,      ShellcheckProcessor;
            LUACHECK,       luacheck,       LuacheckConfig,        LuacheckProcessor;
            MAKE,           make,           MakeConfig,            MakeProcessor;
            CARGO,          cargo,          CargoConfig,           CargoProcessor;
            CLIPPY,         clippy,         ClippyConfig,          ClippyProcessor;
            RUMDL,          rumdl,          RumdlConfig,           RumdlProcessor;
            YAMLLINT,       yamllint,       YamllintConfig,        YamllintProcessor;
            JQ,             jq,             JqConfig,              JqProcessor;
            JSONLINT,       jsonlint,       JsonlintConfig,        JsonlintProcessor;
            TAPLO,          taplo,          TaploConfig,           TaploProcessor;
            JSON_SCHEMA,    json_schema,    JsonSchemaConfig,      JsonSchemaProcessor;
            TAGS,           tags,           TagsConfig,            TagsProcessor;
            PIP,            pip,            PipConfig,             PipProcessor;
            SPHINX,         sphinx,         SphinxConfig,          SphinxProcessor;
            MDBOOK,         mdbook,         MdbookConfig,          MdbookProcessor;
            NPM,            npm,            NpmConfig,             NpmProcessor;
            GEM,            gem,            GemConfig,             GemProcessor;
            MDL,            mdl,            MdlConfig,             MdlProcessor;
            MARKDOWNLINT,   markdownlint,   MarkdownlintConfig,    MarkdownlintProcessor;
            ASPELL,         aspell,         AspellConfig,          AspellProcessor;
            MARP,           marp,           MarpConfig,            MarpProcessor;
            PANDOC,         pandoc,         PandocConfig,          PandocProcessor;
            MARKDOWN2HTML,  markdown2html,  Markdown2htmlConfig,   Markdown2htmlProcessor;
            PDFLATEX,       pdflatex,       PdflatexConfig,        PdflatexProcessor;
            A2X,            a2x,            A2xConfig,             A2xProcessor;
            ASCII,    ascii,    AsciiConfig,      AsciiProcessor;
            TERMS,          terms,          TermsConfig,           TermsProcessor;
            CHROMIUM,       chromium,       ChromiumConfig,        ChromiumProcessor;
            MAKO,           mako,           MakoConfig,            MakoProcessor;
            JINJA2,         jinja2,         Jinja2Config,          Jinja2Processor;
            MERMAID,        mermaid,        MermaidConfig,         MermaidProcessor;
            DRAWIO,         drawio,         DrawioConfig,          DrawioProcessor;
            LIBREOFFICE,    libreoffice,    LibreofficeConfig,     LibreofficeProcessor;
            PROTOBUF,       protobuf,       ProtobufConfig,        ProtobufProcessor;
            PDFUNITE,       pdfunite,       PdfuniteConfig,        PdfuniteProcessor;
            IPDFUNITE,      ipdfunite,      IpdfuniteConfig,       IpdfuniteProcessor;
            SCRIPT,   script,   ScriptConfig,     ScriptProcessor;
            GENERATOR,  generator,  GeneratorConfig,   GeneratorProcessor;
            EXPLICIT,   explicit,   ExplicitConfig,    ExplicitProcessor;
            LINUX_MODULE,   linux_module,   LinuxModuleConfig,     LinuxModuleProcessor;
            CPPLINT,        cpplint,        CpplintConfig,         CpplintProcessor;
            CHECKPATCH,     checkpatch,     CheckpatchConfig,      CheckpatchProcessor;
            OBJDUMP,        objdump,        ObjdumpConfig,         ObjdumpProcessor;
            ESLINT,         eslint,         EslintConfig,          EslintProcessor;
            JSHINT,         jshint,         JshintConfig,          JshintProcessor;
            HTMLHINT,       htmlhint,       HtmlhintConfig,        HtmlhintProcessor;
            TIDY,           tidy,           TidyConfig,            TidyProcessor;
            STYLELINT,      stylelint,      StylelintConfig,       StylelintProcessor;
            JSLINT,         jslint,         JslintConfig,          JslintProcessor;
            STANDARD,       standard,       StandardConfig,        StandardProcessor;
            HTMLLINT,       htmllint,       HtmllintConfig,        HtmllintProcessor;
            PHP_LINT,       php_lint,       PhpLintConfig,         PhpLintProcessor;
            PERLCRITIC,     perlcritic,     PerlcriticConfig,      PerlcriticProcessor;
            XMLLINT,        xmllint,        XmllintConfig,         XmllintProcessor;
            SVGLINT,        svglint,        SvglintConfig,         SvglintProcessor;
            CHECKSTYLE,     checkstyle,     CheckstyleConfig,      CheckstyleProcessor;
            YQ,             yq,             YqConfig,              YqProcessor;
            CMAKE,          cmake,          CmakeConfig,           CmakeProcessor;
            HADOLINT,       hadolint,       HadolintConfig,        HadolintProcessor;
            JEKYLL,         jekyll,         JekyllConfig,          JekyllProcessor;
            SASS,           sass,           SassConfig,            SassProcessor;
            IJQ,            ijq,            IjqConfig,             IjqProcessor;
            IJSONLINT,      ijsonlint,      IjsonlintConfig,       IjsonlintProcessor;
            IYAMLLINT,      iyamllint,      IyamllintConfig,       IyamllintProcessor;
            IYAMLSCHEMA,    iyamlschema,    IyamlschemaConfig,     IyamlschemaProcessor;
            ITAPLO,         itaplo,         ItaploConfig,          ItaploProcessor;
            IMARKDOWN2HTML, imarkdown2html, Imarkdown2htmlConfig,  Imarkdown2htmlProcessor;
            ISASS,          isass,          IsassConfig,           IsassProcessor;
            YAML2JSON,      yaml2json,      Yaml2jsonConfig,       Yaml2jsonProcessor;
            RUST_SINGLE_FILE, rust_single_file, RustSingleFileConfig, RustSingleFileProcessor;
            SLIDEV,         slidev,         SlidevConfig,          SlidevProcessor;
            ENCODING,       encoding,       EncodingConfig,        EncodingProcessor;
            DUPLICATE_FILES, duplicate_files, DuplicateFilesConfig, DuplicateFilesProcessor;
            MARP_IMAGES,    marp_images,    MarpImagesConfig,      MarpImagesProcessor;
            LICENSE_HEADER, license_header, LicenseHeaderConfig,   LicenseHeaderProcessor;
        }
    };
}
