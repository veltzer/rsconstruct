/// Macro to generate `ProductDiscovery` trait implementations for checker processors.
///
/// Eliminates boilerplate for `description()`, `auto_detect()`, `required_tools()`,
/// `discover()`, `execute()`, `config_json()`, `hidden()`, and batch support.
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
/// - `hidden: true` — `hidden()` returns true
/// - `batch: $batch_method:ident` — method on self for batch execution
/// - `extra_discover_inputs: $method:ident` — method on self returning `Vec<String>` of extra inputs for discover
macro_rules! impl_checker {
    // --- @build: generate the impl block ---
    (@build $processor:ty, $config_field:ident, $desc:expr, $name:expr,
     guard: [$($guard:ident)?],
     tools_kind: $tools_kind:tt,
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [$($extra_discover:ident)?],
     execute: $execute:ident,
    ) => {
        impl $crate::processors::ProductDiscovery for $processor {
            fn description(&self) -> &str {
                $desc
            }

            impl_checker!(@hidden $hid);

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
                impl_checker!(@discover self, graph, file_index, $config_field, $name, [$($guard)?], [$($extra_discover)?])
            }

            fn execute(&self, product: &$crate::graph::Product) -> anyhow::Result<()> {
                self.$execute(product)
            }

            impl_checker!(@config_json self, $config_field, $cj);

            impl_checker!(@batch self, [$($batch)?]);
        }
    };

    // --- hidden ---
    (@hidden true) => {
        fn hidden(&self) -> bool { true }
    };
    (@hidden false) => {};

    // --- auto_detect ---
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
    // With guard and extra_discover
    (@discover $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr, [$guard:ident], [$extra:ident]) => {{
        if !$self.$guard() {
            return Ok(());
        }
        let mut extra_inputs = $self.$cfg.extra_inputs.clone();
        extra_inputs.extend($self.$extra());
        $crate::processors::discover_checker_products(
            $graph, &$self.$cfg.scan, $fi, &extra_inputs, &$self.$cfg, $name,
        )
    }};
    // With guard, no extra_discover
    (@discover $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr, [$guard:ident], []) => {{
        if !$self.$guard() {
            return Ok(());
        }
        $crate::processors::discover_checker_products(
            $graph, &$self.$cfg.scan, $fi, &$self.$cfg.extra_inputs, &$self.$cfg, $name,
        )
    }};
    // No guard, with extra_discover
    (@discover $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr, [], [$extra:ident]) => {{
        let mut extra_inputs = $self.$cfg.extra_inputs.clone();
        extra_inputs.extend($self.$extra());
        $crate::processors::discover_checker_products(
            $graph, &$self.$cfg.scan, $fi, &extra_inputs, &$self.$cfg, $name,
        )
    }};
    // No guard, no extra_discover
    (@discover $self:ident, $graph:ident, $fi:ident, $cfg:ident, $name:expr, [], []) => {
        $crate::processors::discover_checker_products(
            $graph, &$self.$cfg.scan, $fi, &$self.$cfg.extra_inputs, &$self.$cfg, $name,
        )
    };

    // --- config_json ---
    (@config_json $self:ident, $cfg:ident, true) => {
        fn config_json(&self) -> Option<String> {
            serde_json::to_string(&self.$cfg).ok()
        }
    };
    (@config_json $self:ident, $cfg:ident, false) => {};

    // --- batch ---
    (@batch $self:ident, [$batch:ident]) => {
        fn supports_batch(&self) -> bool { true }

        fn execute_batch(&self, products: &[&$crate::graph::Product]) -> Vec<anyhow::Result<()>> {
            $crate::processors::execute_checker_batch(
                products,
                |files| self.$batch(files),
            )
        }
    };
    (@batch $self:ident, []) => {};

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
            hidden: false,
            batch: [],
            extra_discover: [],
            ; $($($rest)*)?
        );
    };

    // Parse guard
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [],
     tools_kind: $tk:tt,
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [$($ed:ident)?],
     ; guard: $guard:ident $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$guard],
            tools_kind: $tk,
            config_json: $cj,
            hidden: $hid,
            batch: [$($batch)?],
            extra_discover: [$($ed)?],
            ; $($($rest)*)?
        );
    };

    // Parse tools (literal expressions like "cppcheck".to_string())
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: [none],
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [$($ed:ident)?],
     ; tools: [$($tool:expr),+] $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: [literal: $($tool),+],
            config_json: $cj,
            hidden: $hid,
            batch: [$($batch)?],
            extra_discover: [$($ed)?],
            ; $($($rest)*)?
        );
    };

    // Parse tool_field (field name on config struct, e.g. `linter` → self.config.linter.clone())
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: [none],
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [$($ed:ident)?],
     ; tool_field: $tool_field:ident $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: [field: $tool_field],
            config_json: $cj,
            hidden: $hid,
            batch: [$($batch)?],
            extra_discover: [$($ed)?],
            ; $($($rest)*)?
        );
    };

    // Parse tool_field_extra (field name + extra static tools, e.g. `tool_field_extra: linter ["python3".to_string()]`)
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: [none],
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [$($ed:ident)?],
     ; tool_field_extra: $tool_field:ident [$($extra:expr),+] $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: [field_and_extra: $tool_field, [$($extra),+]],
            config_json: $cj,
            hidden: $hid,
            batch: [$($batch)?],
            extra_discover: [$($ed)?],
            ; $($($rest)*)?
        );
    };

    // Parse config_json
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: false,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [$($ed:ident)?],
     ; config_json: true $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: true,
            hidden: $hid,
            batch: [$($batch)?],
            extra_discover: [$($ed)?],
            ; $($($rest)*)?
        );
    };

    // Parse hidden
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: $cj:tt,
     hidden: false,
     batch: [$($batch:ident)?],
     extra_discover: [$($ed:ident)?],
     ; hidden: true $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: $cj,
            hidden: true,
            batch: [$($batch)?],
            extra_discover: [$($ed)?],
            ; $($($rest)*)?
        );
    };

    // Parse batch
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [],
     extra_discover: [$($ed:ident)?],
     ; batch: $batch_method:ident $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: $cj,
            hidden: $hid,
            batch: [$batch_method],
            extra_discover: [$($ed)?],
            ; $($($rest)*)?
        );
    };

    // Parse extra_discover_inputs
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [],
     ; extra_discover_inputs: $extra_method:ident $(, $($rest:tt)*)?
    ) => {
        impl_checker!(@parse $processor, $config_field, $desc, $name, $execute,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: $cj,
            hidden: $hid,
            batch: [$($batch)?],
            extra_discover: [$extra_method],
            ; $($($rest)*)?
        );
    };

    // Terminal: no more tokens to parse
    (@parse $processor:ty, $config_field:ident, $desc:expr, $name:expr, $execute:ident,
     guard: [$($guard:ident)?],
     tools_kind: $tk:tt,
     config_json: $cj:tt,
     hidden: $hid:tt,
     batch: [$($batch:ident)?],
     extra_discover: [$($ed:ident)?],
     ;
    ) => {
        impl_checker!(@build $processor, $config_field, $desc, $name,
            guard: [$($guard)?],
            tools_kind: $tk,
            config_json: $cj,
            hidden: $hid,
            batch: [$($batch)?],
            extra_discover: [$($ed)?],
            execute: $execute,
        );
    };
}

mod aspell;
mod ascii_check;
mod clippy;
mod clang_tidy;
mod cppcheck;

mod make;
mod markdownlint;
mod mdl;
mod mypy;


mod pylint;
mod pyrefly;
mod ruff;
mod rumdl;
mod shellcheck;
mod sleep;
mod spellcheck;
mod yamllint;
mod jsonlint;
mod taplo;
mod jq;
mod json_schema;

pub use aspell::AspellProcessor;
pub use ascii_check::AsciiCheckProcessor;
pub use clippy::ClippyProcessor;
pub use clang_tidy::ClangTidyProcessor;
pub use cppcheck::CppcheckProcessor;

pub use make::MakeProcessor;
pub use markdownlint::MarkdownlintProcessor;
pub use mdl::MdlProcessor;
pub use mypy::MypyProcessor;


pub use pylint::PylintProcessor;
pub use pyrefly::PyreflyProcessor;
pub use ruff::RuffProcessor;
pub use rumdl::RumdlProcessor;
pub use shellcheck::ShellcheckProcessor;
pub use sleep::SleepProcessor;
pub use spellcheck::SpellcheckProcessor;
pub use yamllint::YamllintProcessor;
pub use jq::JqProcessor;
pub use jsonlint::JsonlintProcessor;
pub use taplo::TaploProcessor;
pub use json_schema::JsonSchemaProcessor;
