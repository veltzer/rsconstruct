/// Macro to generate `ProductDiscovery` trait implementations for checker processors.
///
/// Eliminates boilerplate for `description()`, `auto_detect()`, `required_tools()`,
/// `discover()`, `execute()`, `config_json()`, and batch support.
///
/// # Required parameters
/// - `$processor:ty` — the processor struct type
/// - `config: $config_field:ident` — name of the config field on the struct
/// - `description: $desc:expr` — human-readable description string
/// - `name: $name:expr` — processor name passed to `discover_checker_products()`
/// - `execute: $execute:ident` — method on self for single-product execution
///
/// # Optional parameters (in any order after required ones)
/// - `guard: $guard_method:ident` — method on self returning bool; gates `auto_detect()` and `discover()`
/// - `tools: [$($tool:expr),+]` — string literal expressions for `required_tools()`
/// - `tool_field: $field:ident` — config sub-field to clone as a tool name
/// - `tool_field_extra: $field:ident [$($extra:expr),+]` — config field plus extra static tools
/// - `config_json: true` — emit `config_json()` using `serde_json::to_string`
/// - `batch: $batch_method:ident` — method on self for batch execution
macro_rules! impl_checker {
    // --- @build: generate the impl block ---
    (@build $processor:ty, $config_field:ident, $desc:expr, $name:expr,
     guard: [$($guard:ident)?],
     tools_kind: $tools_kind:tt,
     config_json: $cj:tt,
     batch: [$($batch:ident)?],
     execute: $execute:ident,
    ) => {
        impl $crate::processors::ProductDiscovery for $processor {
            fn description(&self) -> &str {
                $desc
            }

            fn auto_detect(&self, file_index: &$crate::file_index::FileIndex) -> bool {
                impl_checker!(@auto_detect self, file_index, $config_field, [$($guard)?])
            }

            fn required_tools(&self) -> Vec<String> {
                impl_checker!(@tools self, $config_field, $tools_kind)
            }

            fn discover(
                &self,
                graph: &mut $crate::graph::BuildGraph,
                file_index: &$crate::file_index::FileIndex,
            ) -> anyhow::Result<()> {
                impl_checker!(@discover self, graph, file_index, $config_field, $name, [$($guard)?])
            }

            fn execute(&self, product: &$crate::graph::Product) -> anyhow::Result<()> {
                self.$execute(product)
            }

            impl_checker!(@config_json self, $config_field, $cj);

            impl_checker!(@batch self, $config_field, [$($batch)?]);

            fn max_jobs(&self) -> Option<usize> {
                self.$config_field.max_jobs
            }
        }
    };

    // --- auto_detect ---
    (@auto_detect $self:ident, $fi:ident, $cfg:ident, [scan_root]) => {
        $crate::processors::scan_root_valid(&$self.$cfg.scan) && !$fi.scan(&$self.$cfg.scan, true).is_empty()
    };
    (@auto_detect $self:ident, $fi:ident, $cfg:ident, [$guard:ident]) => {
        $self.$guard() && !$fi.scan(&$self.$cfg.scan, true).is_empty()
    };
    (@auto_detect $self:ident, $fi:ident, $cfg:ident, []) => {
        !$fi.scan(&$self.$cfg.scan, true).is_empty()
    };

    // --- tools ---
    // No tools
    (@tools $self:ident, $cfg:ident, [none]) => {
        Vec::new()
    };
    // Static tool names (expressions that may reference $self.$cfg)
    (@tools $self:ident, $cfg:ident, [literal: $($tool:expr),+]) => {
        vec![$($tool),+]
    };
    // Dynamic tool name from a config field
    (@tools $self:ident, $cfg:ident, [field: $tool_field:ident]) => {
        vec![$self.$cfg.$tool_field.clone()]
    };
    // Dynamic tool name from a config field plus extra static tools
    (@tools $self:ident, $cfg:ident, [field_and_extra: $tool_field:ident, [$($extra:expr),+]]) => {
        vec![$self.$cfg.$tool_field.clone(), $($extra),+]
    };

    // --- discover ---
    // Shared body: build extra_inputs and call discover_checker_products
    (@discover_body $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr) => {{
        let mut extra_inputs = $self.$cfg.extra_inputs.clone();
        for ai in &$self.$cfg.auto_inputs {
            extra_inputs.extend($crate::processors::config_file_inputs(ai));
        }
        $crate::processors::discover_checker_products(
            $graph, &$self.$cfg.scan, $fi, &extra_inputs, &$self.$cfg, $name,
        )
    }};
    // With scan_root guard (built-in)
    (@discover $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr, [scan_root]) => {{
        if !$crate::processors::scan_root_valid(&$self.$cfg.scan) {
            return Ok(());
        }
        impl_checker!(@discover_body $self, $graph, $fi, $cfg, $name)
    }};
    // With custom guard method
    (@discover $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr, [$guard:ident]) => {{
        if !$self.$guard() {
            return Ok(());
        }
        impl_checker!(@discover_body $self, $graph, $fi, $cfg, $name)
    }};
    // No guard
    (@discover $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr, []) => {{
        impl_checker!(@discover_body $self, $graph, $fi, $cfg, $name)
    }};

    // --- config_json ---
    (@config_json $self:ident, $cfg:ident, true) => {
        fn config_json(&self) -> Option<String> {
            serde_json::to_string(&self.$cfg).ok()
        }
    };
    (@config_json $self:ident, $cfg:ident, false) => {};

    // --- batch ---
    (@batch $self:ident, $cfg:ident, [$batch:ident]) => {
        fn supports_batch(&self) -> bool { self.$cfg.batch }

        fn execute_batch(&self, products: &[&$crate::graph::Product]) -> Vec<anyhow::Result<()>> {
            $crate::processors::execute_checker_batch(
                products,
                |files| self.$batch(files),
            )
        }
    };
    (@batch $self:ident, $cfg:ident, []) => {};

    // --- Entry point: parse options using TT muncher ---
    ($processor:ty,
     config: $config_field:ident,
     description: $desc:expr,
     name: $name:expr,
     execute: $execute:ident
     $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [],
            tools_kind: [none],
            config_json: false,

            batch: [],
            ; $($($rest)*)?
        );
    };

    // Parse guard
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [],
     tools_kind: $tk:tt,
     config_json: $cj:tt,

     batch: [$($batch:ident)?],
     ; guard: $guard:ident $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$guard],
            tools_kind: $tk,
            config_json: $cj,

            batch: [$($batch)?],
            ; $($($rest)*)?
        );
    };

    // Parse tools (literal expressions like "cppcheck".to_string())
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: [none],
     config_json: $cj:tt,

     batch: [$($batch:ident)?],
     ; tools: [$($tool:expr),+] $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: [literal: $($tool),+],
            config_json: $cj,

            batch: [$($batch)?],
            ; $($($rest)*)?
        );
    };

    // Parse tool_field (field name on config struct, e.g. `linter` → self.config.linter.clone())
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: [none],
     config_json: $cj:tt,

     batch: [$($batch:ident)?],
     ; tool_field: $tool_field:ident $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: [field: $tool_field],
            config_json: $cj,

            batch: [$($batch)?],
            ; $($($rest)*)?
        );
    };

    // Parse tool_field_extra (field name + extra static tools, e.g. `tool_field_extra: linter ["python3".to_string()]`)
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: [none],
     config_json: $cj:tt,

     batch: [$($batch:ident)?],
     ; tool_field_extra: $tool_field:ident [$($extra:expr),+] $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: [field_and_extra: $tool_field, [$($extra),+]],
            config_json: $cj,

            batch: [$($batch)?],
            ; $($($rest)*)?
        );
    };

    // Parse config_json
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: false,

     batch: [$($batch:ident)?],
     ; config_json: true $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: true,

            batch: [$($batch)?],
            ; $($($rest)*)?
        );
    };

    // Parse batch
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: $cj:tt,

     batch: [],
     ; batch: $batch_method:ident $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: $cj,

            batch: [$batch_method],
            ; $($($rest)*)?
        );
    };

    // Terminal: no more tokens to parse
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: $cj:tt,

     batch: [$($batch:ident)?],
     ;
    ) => {
        impl_checker!(@build $processor, $config_field, $desc, $name,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: $cj,

            batch: [$($batch)?],
            execute: $execute,
        );
    };
}

/// Generate a complete trivial checker processor from just its parameters.
///
/// This eliminates boilerplate for the ~20 checkers that only call `run_checker()`.
/// Each trivial checker file becomes a single macro invocation instead of ~35 lines.
///
/// # Parameters
/// - `$processor:ident` — processor struct name
/// - `$config:ty` — config struct type
/// - `$desc:expr` — description string
/// - `$name:expr` — processor name constant
///
/// # Tool specification (one required)
/// - `tool_field: $field:ident` — tool name from config field (e.g. `linter`)
/// - `tool_field_extra: $field:ident [$($extra:expr),+]` — config field + extra tools
/// - `tools: [$($tool:expr),+]` — static tool name expressions
///
/// # Optional
/// - `subcommand: $sub:expr` — subcommand string (e.g. "check")
/// - `prepend_args: [$($arg:expr),+]` — args prepended before config args
macro_rules! simple_checker {
    // Internal: generate struct + methods + impl_checker
    (@gen $processor:ident, $config:ty, $desc:expr, $name:expr,
     subcommand: [$($sub:expr)?],
     prepend_args: [$($pa:expr),*],
     tool_kind: $tool_kind:tt,
    ) => {
        pub struct $processor {
            config: $config,
        }

        impl $processor {
            pub fn new(config: $config) -> Self {
                Self { config }
            }

            fn execute_product(&self, product: &$crate::graph::Product) -> anyhow::Result<()> {
                self.check_files(&[product.primary_input()])
            }

            fn check_files(&self, files: &[&std::path::Path]) -> anyhow::Result<()> {
                simple_checker!(@run_checker self, files, [$($sub)?], [$($pa),*], $tool_kind)
            }
        }

        simple_checker!(@impl_checker $processor, $desc, $name, $tool_kind);
    };

    // --- run_checker dispatch ---
    // Tool from config field
    (@run_checker $self:ident, $files:ident, [$($sub:expr)?], [$($pa:expr),*], [field: $field:ident]) => {{
        simple_checker!(@do_run_checker &$self.config.$field, [$($sub)?], &$self.config.args, $files, [$($pa),*])
    }};
    // Tool from config field with extra tools (extra tools only affect required_tools, not the command)
    (@run_checker $self:ident, $files:ident, [$($sub:expr)?], [$($pa:expr),*], [field_extra: $field:ident, [$($extra:expr),+]]) => {{
        simple_checker!(@do_run_checker &$self.config.$field, [$($sub)?], &$self.config.args, $files, [$($pa),*])
    }};
    // Static tool name (first tool expression is the command)
    (@run_checker $self:ident, $files:ident, [$($sub:expr)?], [$($pa:expr),*], [static_tool: $tool:expr $(, $extra:expr)*]) => {{
        let tool_name = $tool;
        simple_checker!(@do_run_checker &tool_name, [$($sub)?], &$self.config.args, $files, [$($pa),*])
    }};

    // --- do_run_checker: handle subcommand and prepend_args ---
    (@do_run_checker $tool:expr, [], $args:expr, $files:ident, []) => {
        $crate::processors::run_checker($tool, None, $args, $files)
    };
    (@do_run_checker $tool:expr, [$sub:expr], $args:expr, $files:ident, []) => {
        $crate::processors::run_checker($tool, Some($sub), $args, $files)
    };
    (@do_run_checker $tool:expr, [], $args:expr, $files:ident, [$($pa:expr),+]) => {{
        let mut combined_args: Vec<String> = vec![$($pa.to_string()),+];
        combined_args.extend_from_slice($args);
        $crate::processors::run_checker($tool, None, &combined_args, $files)
    }};
    (@do_run_checker $tool:expr, [$sub:expr], $args:expr, $files:ident, [$($pa:expr),+]) => {{
        let mut combined_args: Vec<String> = vec![$($pa.to_string()),+];
        combined_args.extend_from_slice($args);
        $crate::processors::run_checker($tool, Some($sub), &combined_args, $files)
    }};

    // --- impl_checker dispatch ---
    (@impl_checker $processor:ident, $desc:expr, $name:expr, [field: $field:ident]) => {
        impl_checker!($processor,
            config: config,
            description: $desc,
            name: $name,
            execute: execute_product,
            tool_field: $field,
            config_json: true,
            batch: check_files,
        );
    };
    (@impl_checker $processor:ident, $desc:expr, $name:expr, [field_extra: $field:ident, [$($extra:expr),+]]) => {
        impl_checker!($processor,
            config: config,
            description: $desc,
            name: $name,
            execute: execute_product,
            tool_field_extra: $field [$($extra),+],
            config_json: true,
            batch: check_files,
        );
    };
    (@impl_checker $processor:ident, $desc:expr, $name:expr, [static_tool: $($tool:expr),+]) => {
        impl_checker!($processor,
            config: config,
            description: $desc,
            name: $name,
            execute: execute_product,
            tools: [$($tool),+],
            config_json: true,
            batch: check_files,
        );
    };

    // --- Public entry points ---

    // tool_field variant (no subcommand, no prepend_args)
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tool_field: $field:ident $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [], prepend_args: [], tool_kind: [field: $field],);
    };
    // tool_field with subcommand
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tool_field: $field:ident, subcommand: $sub:expr $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [$sub], prepend_args: [], tool_kind: [field: $field],);
    };
    // tool_field with subcommand + prepend_args
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tool_field: $field:ident, subcommand: $sub:expr, prepend_args: [$($pa:expr),+ $(,)?] $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [$sub], prepend_args: [$($pa),+], tool_kind: [field: $field],);
    };
    // tool_field with prepend_args (no subcommand)
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tool_field: $field:ident, prepend_args: [$($pa:expr),+ $(,)?] $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [], prepend_args: [$($pa),+], tool_kind: [field: $field],);
    };
    // tool_field_extra variant
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tool_field_extra: $field:ident [$($extra:expr),+] $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [], prepend_args: [], tool_kind: [field_extra: $field, [$($extra),+]],);
    };
    // tools variant (no subcommand)
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tools: [$($tool:expr),+] $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [], prepend_args: [], tool_kind: [static_tool: $($tool),+],);
    };
    // tools with subcommand
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tools: [$($tool:expr),+], subcommand: $sub:expr $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [$sub], prepend_args: [], tool_kind: [static_tool: $($tool),+],);
    };
    // tools with prepend_args (no subcommand)
    ($processor:ident, $config:ty, $desc:expr, $name:expr,
     tools: [$($tool:expr),+], prepend_args: [$($pa:expr),+ $(,)?] $(,)?
    ) => {
        simple_checker!(@gen $processor, $config, $desc, $name,
            subcommand: [], prepend_args: [$($pa),+], tool_kind: [static_tool: $($tool),+],);
    };
}

mod aspell;
mod ascii;
mod black;
mod checkpatch;
mod clippy;
mod clang_tidy;
mod cppcheck;
mod cpplint;
mod doctest;
mod duplicate_files;
mod encoding;

mod license_header;
mod make;
mod marp_images;
mod markdownlint;
mod mdl;
mod mypy;


mod pylint;
mod pytest;
mod pyrefly;
mod ruff;
mod rumdl;
mod script;
mod shellcheck;
mod zspell;
mod yamllint;
mod jsonlint;
mod taplo;
mod jq;
mod json_schema;
mod luacheck;
mod eslint;
mod htmlhint;
mod htmllint;
mod jshint;
mod jslint;
mod standard;
mod stylelint;
mod tidy;
mod php_lint;
mod perlcritic;
mod xmllint;
mod checkstyle;
mod yq;
mod cmake;
mod hadolint;
mod slidev;
pub(crate) mod terms;

pub use aspell::AspellProcessor;
pub use ascii::AsciiProcessor;
pub use black::BlackProcessor;
pub use checkpatch::CheckpatchProcessor;
pub use clippy::ClippyProcessor;
pub use clang_tidy::ClangTidyProcessor;
pub use cppcheck::CppcheckProcessor;
pub use cpplint::CpplintProcessor;
pub use doctest::DoctestProcessor;
pub use duplicate_files::DuplicateFilesProcessor;
pub use encoding::EncodingProcessor;
pub use license_header::LicenseHeaderProcessor;
pub use make::MakeProcessor;
pub use marp_images::MarpImagesProcessor;
pub use markdownlint::MarkdownlintProcessor;
pub use mdl::MdlProcessor;
pub use mypy::MypyProcessor;


pub use pylint::PylintProcessor;
pub use pytest::PytestProcessor;
pub use pyrefly::PyreflyProcessor;
pub use ruff::RuffProcessor;
pub use rumdl::RumdlProcessor;
pub use script::ScriptProcessor;
pub use shellcheck::ShellcheckProcessor;
pub use zspell::ZspellProcessor;
pub use yamllint::YamllintProcessor;
pub use jq::JqProcessor;
pub use jsonlint::JsonlintProcessor;
pub use luacheck::LuacheckProcessor;
pub use taplo::TaploProcessor;
pub use json_schema::JsonSchemaProcessor;
pub use eslint::EslintProcessor;
pub use htmlhint::HtmlhintProcessor;
pub use htmllint::HtmllintProcessor;
pub use jshint::JshintProcessor;
pub use jslint::JslintProcessor;
pub use standard::StandardProcessor;
pub use stylelint::StylelintProcessor;
pub use tidy::TidyProcessor;
pub use php_lint::PhpLintProcessor;
pub use perlcritic::PerlcriticProcessor;
pub use xmllint::XmllintProcessor;
pub use checkstyle::CheckstyleProcessor;
pub use yq::YqProcessor;
pub use cmake::CmakeProcessor;
pub use hadolint::HadolintProcessor;
pub use slidev::SlidevProcessor;
pub use terms::TermsProcessor;
