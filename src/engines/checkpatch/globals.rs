//! checkpatch.pl's global patterns (`$Ident`, `$Type`, ...), defined in
//! its order from the same pattern text, byte for byte (the unit tests
//! compare each with the value checkpatch.pl builds), each a `qr//` value
//! as Perl stringifies it.

// `${name}` in these templates is Perl interpolation, not a format argument.
#![allow(clippy::literal_string_with_formatting_args)]

use std::collections::HashMap;

use super::re::{interp, qr};

/// The pattern variables by name.
#[derive(Clone)]
pub struct Vars {
    by_name: HashMap<String, Vec<u8>>,
    /// `@typeListFile` and `@modifierListFile`: types and modifiers
    /// `possible()` learned from the file.
    pub type_list_file: Vec<Vec<u8>>,
    pub modifier_list_file: Vec<Vec<u8>>,
}

impl Vars {
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.by_name.get(name).cloned()
    }

    pub fn val(&self, name: &str) -> &[u8] {
        self.by_name
            .get(name)
            .unwrap_or_else(|| panic!("no checkpatch variable ${name}"))
    }

    fn eval(&self, template: &str) -> Vec<u8> {
        interp(template.as_bytes(), &|n| self.get(n))
    }

    /// `our $name = qr{template}flags`.
    fn qr(&mut self, name: &str, template: &str, flags: &str) {
        let value = qr(&self.eval(template), flags);
        self.by_name.insert(name.to_string(), value);
    }

    /// `our $name = "template"`.
    fn string(&mut self, name: &str, template: &str) {
        let value = self.eval(template);
        self.by_name.insert(name.to_string(), value);
    }

    fn set(&mut self, name: &str, value: Vec<u8>) {
        self.by_name.insert(name.to_string(), value);
    }

    /// The pattern variables at startup, `build_types` run once.
    pub fn new() -> Self {
        let mut v = Self {
            by_name: HashMap::new(),
            type_list_file: Vec::new(),
            modifier_list_file: Vec::new(),
        };
        v.qr(
            "Ident",
            r"
			[A-Za-z_][A-Za-z\d_]*
			(?:\s*\#\#\s*[A-Za-z_][A-Za-z\d_]*)*
		",
            "x",
        );
        v.qr("Storage", r"extern|static|asmlinkage", "");
        v.qr(
            "Sparse",
            r"
			__user|
			__kernel|
			__force|
			__iomem|
			__must_check|
			__kprobes|
			__ref|
			__refconst|
			__refdata|
			__rcu|
			__private
		",
            "x",
        );
        v.qr("InitAttributePrefix", r"__(?:mem|cpu|dev|net_|)", "");
        v.qr(
            "InitAttributeData",
            r"$InitAttributePrefix(?:initdata\b)",
            "",
        );
        v.qr(
            "InitAttributeConst",
            r"$InitAttributePrefix(?:initconst\b)",
            "",
        );
        v.qr("InitAttributeInit", r"$InitAttributePrefix(?:init\b)", "");
        v.qr(
            "InitAttribute",
            r"$InitAttributeData|$InitAttributeConst|$InitAttributeInit",
            "",
        );
        v.qr(
            "Attribute",
            r"
			const|
			volatile|
			__percpu|
			__nocast|
			__safe|
			__bitwise|
			__packed__|
			__packed2__|
			__naked|
			__maybe_unused|
			__always_unused|
			__noreturn|
			__used|
			__cold|
			__pure|
			__noclone|
			__deprecated|
			__read_mostly|
			__ro_after_init|
			__kprobes|
			$InitAttribute|
			__aligned\s*\(.*\)|
			____cacheline_aligned|
			____cacheline_aligned_in_smp|
			____cacheline_internodealigned_in_smp|
			__weak|
			__alloc_size\s*\(\s*\d+\s*(?:,\s*\d+\s*)?\)
		  ",
            "x",
        );
        v.qr(
            "Inline",
            r"inline|__always_inline|noinline|__inline|__inline__",
            "",
        );
        v.qr("Member", r"->$Ident|\.$Ident|\[[^]]*\]", "");
        v.qr("Lval", r"$Ident(?:$Member)*", "");
        v.qr("Int_type", r"(?i)llu|ull|ll|lu|ul|l|u", "");
        v.qr("Binary", r"(?i)0b[01]+$Int_type?", "");
        v.qr("Hex", r"(?i)0x[0-9a-f]+$Int_type?", "");
        v.qr("Int", r"[0-9]+$Int_type?", "");
        v.qr("Octal", r"0[0-7]+$Int_type?", "");
        v.qr("String", r#"(?:\b[Lu])?"[X\t]*""#, "");
        v.qr("Float_hex", r"(?i)0x[0-9a-f]+p-?[0-9]+[fl]?", "");
        v.qr(
            "Float_dec",
            r"(?i)(?:[0-9]+\.[0-9]*|[0-9]*\.[0-9]+)(?:e-?[0-9]+)?[fl]?",
            "",
        );
        v.qr("Float_int", r"(?i)[0-9]+e-?[0-9]+[fl]?", "");
        v.qr("Float", r"$Float_hex|$Float_dec|$Float_int", "");
        v.qr("Constant", r"$Float|$Binary|$Octal|$Hex|$Int", "");
        v.qr("Assignment", r"\*\=|/=|%=|\+=|-=|<<=|>>=|&=|\^=|\|=|=", "");
        v.qr("Compare", r"<=|>=|==|!=|<|(?<!-)>", "");
        v.qr("Arithmetic", r"\+|-|\*|\/|%", "");
        v.qr(
            "Operators",
            r"
			<=|>=|==|!=|
			=>|->|<<|>>|<|>|!|~|
			&&|\|\||,|\^|\+\+|--|&|\||$Arithmetic
		  ",
            "x",
        );
        v.qr(
            "c90_Keywords",
            r"do|for|while|if|else|return|goto|continue|switch|default|case|break",
            "x",
        );
        v.qr(
            "NON_ASCII_UTF8",
            r"
	[\xC2-\xDF][\x80-\xBF]               # non-overlong 2-byte
	|  \xE0[\xA0-\xBF][\x80-\xBF]        # excluding overlongs
	| [\xE1-\xEC\xEE\xEF][\x80-\xBF]{2}  # straight 3-byte
	|  \xED[\x80-\x9F][\x80-\xBF]        # excluding surrogates
	|  \xF0[\x90-\xBF][\x80-\xBF]{2}     # planes 1-3
	| [\xF1-\xF3][\x80-\xBF]{3}          # planes 4-15
	|  \xF4[\x80-\x8F][\x80-\xBF]{2}     # plane 16
",
            "x",
        );
        v.qr(
            "UTF8",
            r"
	[\x09\x0A\x0D\x20-\x7E]              # ASCII
	| $NON_ASCII_UTF8
",
            "x",
        );
        v.qr(
            "typeC99Typedefs",
            r"(?:__)?(?:[us]_?)?int_?(?:8|16|32|64)_t",
            "",
        );
        v.qr(
            "typeOtherOSTypedefs",
            r"(?x:
	u_(?:char|short|int|long) |          # bsd
	u(?:nchar|short|int|long)            # sysv
)",
            "",
        );
        v.qr(
            "typeKernelTypedefs",
            r"(?x:
	(?:__)?(?:u|s|be|le)(?:8|16|32|64)|
	atomic_t
)",
            "",
        );
        v.qr(
            "typeStdioTypedefs",
            r"(?x:
	FILE
)",
            "",
        );
        v.qr(
            "typeTypedefs",
            r"(?x:
	$typeC99Typedefs\b|
	$typeOtherOSTypedefs\b|
	$typeKernelTypedefs\b|
	$typeStdioTypedefs\b
)",
            "",
        );
        v.qr(
            "zero_initializer",
            r"(?:(?:0[xX])?0+$Int_type?|NULL|false)\b",
            "",
        );
        v.qr("logFunctions", r"(?x:
	printk(?:_ratelimited|_once|_deferred_once|_deferred|)|
	(?:[a-z0-9]+_){1,2}(?:printk|emerg|alert|crit|err|warning|warn|notice|info|debug|dbg|vdbg|devel|cont|WARN)(?:_ratelimited|_once|)|
	TP_printk|
	WARN(?:_RATELIMIT|_ONCE|)|
	panic|
	MODULE_[A-Z_]+|
	seq_vprintf|seq_printf|seq_puts
)", "");
        v.qr(
            "allocFunctions",
            r"(?x:
	(?:(?:devm_)?
		(?:kv|k|v)[czm]alloc(?:_array)?(?:_node)? |
		kstrdup(?:_const)? |
		kmemdup(?:_nul)?) |
	(?:\w+)?alloc_skb(?:_ip_align)? |
				# dev_alloc_skb/netdev_alloc_skb, et al
	dma_alloc_coherent
)",
            "",
        );
        v.qr(
            "signature_tags",
            r"(?xi:
	Signed-off-by:|
	Co-developed-by:|
	Acked-by:|
	Tested-by:|
	Reviewed-by:|
	Reported-by:|
	Suggested-by:|
	Assisted-by:|
	To:|
	Cc:
)",
            "",
        );
        v.set("link_tags_search", b"(?:Link:|Closes:)".to_vec());
        v.set("link_tags_print", b"'Link:' or 'Closes:'".to_vec());
        v.qr(
            "tracing_logging_tags",
            r"(?xi:
	[=-]*> |
	<[=-]* |
	\[ |
	\] |
	start |
	called |
	entered |
	entry |
	enter |
	in |
	inside |
	here |
	begin |
	exit |
	end |
	done |
	leave |
	completed |
	out |
	return |
	[\.\!:\s]*
)",
            "",
        );
        v.qr("dev_id_types", r"\b[a-z]\w*_device_id\b", "");
        v.qr(
            "obsolete_archives",
            r"(?xi:
	\Qfreedesktop.org/archives/dri-devel\E |
	\Qlists.infradead.org\E |
	\Qlkml.org\E |
	\Qmail-archive.com\E |
	\Qmailman.alsa-project.org/pipermail\E |
	\Qmarc.info\E |
	\Qozlabs.org/pipermail\E |
	\Qspinics.net\E
)",
            "",
        );
        v.qr(
            "C90_int_types",
            r"(?x:
	long\s+long\s+int\s+(?:un)?signed|
	long\s+long\s+(?:un)?signed\s+int|
	long\s+long\s+(?:un)?signed|
	(?:(?:un)?signed\s+)?long\s+long\s+int|
	(?:(?:un)?signed\s+)?long\s+long|
	int\s+long\s+long\s+(?:un)?signed|
	int\s+(?:(?:un)?signed\s+)?long\s+long|

	long\s+int\s+(?:un)?signed|
	long\s+(?:un)?signed\s+int|
	long\s+(?:un)?signed|
	(?:(?:un)?signed\s+)?long\s+int|
	(?:(?:un)?signed\s+)?long|
	int\s+long\s+(?:un)?signed|
	int\s+(?:(?:un)?signed\s+)?long|

	int\s+(?:un)?signed|
	(?:(?:un)?signed\s+)?int
)",
            "",
        );
        let mode_perm_funcs: Vec<&str> = MODE_PERMISSION_FUNCS.iter().map(|(f, _)| *f).collect();
        v.set(
            "mode_perms_search",
            format!("(?:{})", mode_perm_funcs.join("|")).into_bytes(),
        );
        let deprecated: Vec<&str> = DEPRECATED_APIS.iter().map(|(f, _)| *f).collect();
        v.set(
            "deprecated_apis_search",
            format!("(?:{})", deprecated.join("|")).into_bytes(),
        );
        v.qr(
            "mode_perms_world_writable",
            r"
	S_IWUGO		|
	S_IWOTH		|
	S_IRWXUGO	|
	S_IALLUGO	|
	0[0-7][0-7][2367]
",
            "x",
        );
        let strings: Vec<&str> = MODE_PERMISSION_STRING_TYPES
            .iter()
            .map(|(s, _)| *s)
            .collect();
        v.set("mode_perms_string_search", strings.join("|").into_bytes());
        v.string(
            "single_mode_perms_string_search",
            r"(?:${mode_perms_string_search})",
        );
        v.qr(
            "multi_mode_perms_string_search",
            r"
	${single_mode_perms_string_search}
	(?:\s*\|\s*${single_mode_perms_string_search})*
",
            "x",
        );
        v.qr(
            "allowed_asm_includes",
            r"(?x:
	irq|
	memory|
	time|
	reboot
)",
            "",
        );
        v.build_types();
        v.qr("Typecast", r"\s*(\(\s*$NonptrType\s*\)){0,1}\s*", "");
        v.qr("balanced_parens", r"(\((?:[^\(\)]++|(?-1))*\))", "");
        v.qr(
            "LvalOrFunc",
            r"((?:[\&\*]\s*)?$Lval)\s*($balanced_parens{0,1})\s*",
            "",
        );
        v.qr(
            "FuncArg",
            r"$Typecast{0,1}($LvalOrFunc|$Constant|$String)",
            "",
        );
        v.qr(
            "declaration_macros",
            r"(?x:
	(?:$Storage\s+)?(?:[A-Z_][A-Z0-9]*_){0,2}(?:DEFINE|DECLARE)(?:_[A-Z0-9]+){1,6}\s*\(|
	(?:$Storage\s+)?[HLP]?LIST_HEAD\s*\(|
	(?:SKCIPHER_REQUEST|SHASH_DESC|AHASH_REQUEST)_ON_STACK\s*\(|
	(?:$Storage\s+)?(?:XA_STATE|XA_STATE_ORDER)\s*\(|
	__cacheline_group_(?:begin|end)(?:_aligned)?\s*\(|
	__dma_from_device_group_(?:begin|end)\s*\(
)",
            "",
        );
        v.set("word_pattern", WORD_PATTERN.as_bytes().to_vec());
        v.set("CONFIG_", b"CONFIG_".to_vec());
        // `process` builds these per line; their text never changes.
        v.qr(
            "barriers",
            r"
			mb|
			rmb|
			wmb
		",
            "x",
        );
        v.qr(
            "barrier_stems",
            r"
			mb__before_atomic|
			mb__after_atomic|
			store_release|
			load_acquire|
			store_mb|
			(?:$barriers)
		",
            "x",
        );
        v.qr(
            "all_barriers",
            r"
			(?:$barriers)|
			smp_(?:$barrier_stems)|
			virt_(?:$barrier_stems)
		",
            "x",
        );
        v.qr("underscore_smp_barriers", r"__smp_(?:$barrier_stems)", "x");
        v.qr(
            "asm_volatile",
            r"\b(__asm__|asm)\s+(__volatile__|volatile)\b",
            "",
        );
        v.qr(
            "ops",
            r"
				<<=|>>=|<=|>=|==|!=|
				\+=|-=|\*=|\/=|%=|\^=|\|=|&=|
				=>|->|<<|>>|<|>|=|!|~|
				&&|\|\||,|\^|\+\+|--|&|\||\+|-|\*|\/|%|
				\?:|\?|:
			",
            "x",
        );
        v.qr(
            "valid_licenses",
            r"
						GPL|
						GPL\ v2|
						GPL\ and\ additional\ rights|
						Dual\ BSD/GPL|
						Dual\ MIT/GPL|
						Dual\ MPL/GPL|
						Proprietary
					",
            "x",
        );
        v.qr(
            "rcu_trace_funcs",
            r"(?x:
			rcu_read_lock_tasks_trace |
			rcu_read_lock_trace |
			rcu_read_lock_trace_held |
			rcu_read_unlock_trace |
			rcu_read_unlock_tasks_trace |
			call_rcu_tasks_trace |
			synchronize_rcu_tasks_trace |
			rcu_barrier_tasks_trace |
			rcu_tasks_trace_expedite_current |
			rcu_request_urgent_qs_task
		)",
            "",
        );
        v.qr(
            "rcu_trace_paths",
            r"(?x:
			kernel/bpf/ |
			include/linux/bpf |
			net/bpf/ |
			kernel/rcu/ |
			include/linux/rcu
		)",
            "",
        );
        v
    }

    /// `build_types`: the type patterns, from the built-in lists and what
    /// the file has added to them.
    pub fn build_types(&mut self) {
        let ident = self.val("Ident").to_vec();
        let init_attribute = self.val("InitAttribute").to_vec();
        let item = |template: &str| -> Vec<u8> {
            qr(
                &interp(template.as_bytes(), &|n| match n {
                    "Ident" => Some(ident.clone()),
                    "InitAttribute" => Some(init_attribute.clone()),
                    _ => None,
                }),
                "",
            )
        };
        let misordered: Vec<Vec<u8>> = TYPE_LIST_MISORDERED.iter().map(|t| item(t)).collect();
        let mut type_list: Vec<Vec<u8>> = TYPE_LIST.iter().map(|t| item(t)).collect();
        type_list.extend(misordered.iter().cloned());
        let mut with_attr = type_list.clone();
        with_attr.push(item(r"struct\s+$InitAttribute\s+$Ident"));
        with_attr.push(item(r"union\s+$InitAttribute\s+$Ident"));
        let mut modifiers = vec![item("fastcall")];
        modifiers.extend(self.modifier_list_file.iter().cloned());
        let mut all = type_list.clone();
        all.extend(self.type_list_file.iter().cloned());
        let group = |items: &[Vec<u8>]| -> Vec<u8> {
            let mut out = b"(?x:  \n".to_vec();
            out.extend(items.join(&b"|\n  "[..]));
            out.extend(b"\n)");
            out
        };
        self.set("mods", group(&modifiers));
        self.set("all", group(&all));
        self.set("Misordered", group(&misordered));
        self.set("allWithAttr", group(&with_attr));
        self.qr("Modifier", r"(?:$Attribute|$Sparse|$mods)", "");
        self.qr(
            "BasicType",
            r"
				(?:$typeTypedefs\b)|
				(?:${all}\b)
		",
            "x",
        );
        self.qr(
            "NonptrType",
            r"
			(?:$Modifier\s+|const\s+)*
			(?:
				(?:typeof|__typeof__)\s*\([^\)]*\)|
				(?:$typeTypedefs\b)|
				(?:${all}\b)
			)
			(?:\s+$Modifier|\s+const)*
		  ",
            "x",
        );
        self.qr(
            "NonptrTypeMisordered",
            r"
			(?:$Modifier\s+|const\s+)*
			(?:
				(?:${Misordered}\b)
			)
			(?:\s+$Modifier|\s+const)*
		  ",
            "x",
        );
        self.qr(
            "NonptrTypeWithAttr",
            r"
			(?:$Modifier\s+|const\s+)*
			(?:
				(?:typeof|__typeof__)\s*\([^\)]*\)|
				(?:$typeTypedefs\b)|
				(?:${allWithAttr}\b)
			)
			(?:\s+$Modifier|\s+const)*
		  ",
            "x",
        );
        self.qr(
            "Type",
            r"
			$NonptrType
			(?:(?:\s|\*|\[\])+\s*const|(?:\s|\*\s*(?:const\s*)?|\[\])+|(?:\s*\[\s*\])+){0,4}
			(?:\s+$Inline|\s+$Modifier)*
		  ",
            "x",
        );
        self.qr(
            "TypeMisordered",
            r"
			$NonptrTypeMisordered
			(?:(?:\s|\*|\[\])+\s*const|(?:\s|\*\s*(?:const\s*)?|\[\])+|(?:\s*\[\s*\])+){0,4}
			(?:\s+$Inline|\s+$Modifier)*
		  ",
            "x",
        );
        self.qr("Declare", r"(?:$Storage\s+(?:$Inline\s+)?)?$Type", "");
        self.qr(
            "DeclareMisordered",
            r"(?:$Storage\s+(?:$Inline\s+)?)?$TypeMisordered",
            "",
        );
    }
}

pub const TYPE_LIST_MISORDERED: &[&str] = &[
    r"char\s+(?:un)?signed",
    r"int\s+(?:(?:un)?signed\s+)?short\s",
    r"int\s+short(?:\s+(?:un)?signed)",
    r"short\s+int(?:\s+(?:un)?signed)",
    r"(?:un)?signed\s+int\s+short",
    r"short\s+(?:un)?signed",
    r"long\s+int\s+(?:un)?signed",
    r"int\s+long\s+(?:un)?signed",
    r"long\s+(?:un)?signed\s+int",
    r"int\s+(?:un)?signed\s+long",
    r"int\s+(?:un)?signed",
    r"int\s+long\s+long\s+(?:un)?signed",
    r"long\s+long\s+int\s+(?:un)?signed",
    r"long\s+long\s+(?:un)?signed\s+int",
    r"long\s+long\s+(?:un)?signed",
    r"long\s+(?:un)?signed",
];

pub const TYPE_LIST: &[&str] = &[
    r"void",
    r"(?:(?:un)?signed\s+)?char",
    r"(?:(?:un)?signed\s+)?short\s+int",
    r"(?:(?:un)?signed\s+)?short",
    r"(?:(?:un)?signed\s+)?int",
    r"(?:(?:un)?signed\s+)?long\s+int",
    r"(?:(?:un)?signed\s+)?long\s+long\s+int",
    r"(?:(?:un)?signed\s+)?long\s+long",
    r"(?:(?:un)?signed\s+)?long",
    r"(?:un)?signed",
    r"float",
    r"double",
    r"bool",
    r"struct\s+$Ident",
    r"union\s+$Ident",
    r"enum\s+$Ident",
    r"${Ident}_t",
    r"${Ident}_handler",
    r"${Ident}_handler_fn",
];

/// `@mode_permission_funcs`: functions and the argument that is a mode.
pub const MODE_PERMISSION_FUNCS: &[(&str, usize)] = &[
    ("module_param", 3),
    ("module_param_(?:array|named|string)", 4),
    ("module_param_array_named", 5),
    (
        "debugfs_create_(?:file|u8|u16|u32|u64|x8|x16|x32|x64|size_t|atomic_t|bool|blob|regset32|u32_array)",
        2,
    ),
    ("proc_create(?:_data|)", 2),
    ("(?:CLASS|DEVICE|SENSOR|SENSOR_DEVICE|IIO_DEVICE)_ATTR", 2),
    ("IIO_DEV_ATTR_[A-Z_]+", 1),
    ("SENSOR_(?:DEVICE_|)ATTR_2", 2),
    ("SENSOR_TEMPLATE(?:_2|)", 3),
    ("__ATTR", 2),
];

/// `%deprecated_apis` (Perl iterates it in hash order; the patterns built
/// from it are bounded by `\b`, so the order does not change a match).
pub const DEPRECATED_APIS: &[(&str, &str)] = &[
    ("kmap", "kmap_local_page"),
    ("kunmap", "kunmap_local"),
    ("kmap_atomic", "kmap_local_page"),
    ("kunmap_atomic", "kunmap_local"),
    ("DEFINE_IDR", "DEFINE_XARRAY"),
    ("idr_init", "xa_init"),
    ("idr_init_base", "xa_init_flags"),
];

/// `%mode_permission_string_types`.
pub const MODE_PERMISSION_STRING_TYPES: &[(&str, u32)] = &[
    ("S_IRWXU", 0o700),
    ("S_IRUSR", 0o400),
    ("S_IWUSR", 0o200),
    ("S_IXUSR", 0o100),
    ("S_IRWXG", 0o070),
    ("S_IRGRP", 0o040),
    ("S_IWGRP", 0o020),
    ("S_IXGRP", 0o010),
    ("S_IRWXO", 0o007),
    ("S_IROTH", 0o004),
    ("S_IWOTH", 0o002),
    ("S_IXOTH", 0o001),
    ("S_IRWXUGO", 0o777),
    ("S_IRUGO", 0o444),
    ("S_IWUGO", 0o222),
    ("S_IXUGO", 0o111),
];

/// `%allow_repeated_words`.
pub const ALLOW_REPEATED_WORDS: &[&str] = &["add", "added", "bad", "be"];

/// `$word_pattern`.
pub const WORD_PATTERN: &str = r"\b[A-Z]?[a-z]{2,}\b";
