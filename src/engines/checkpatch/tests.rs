//! The port checked against Perl and checkpatch.pl themselves.

use std::fmt::Write as _;
use std::process::Command;

use super::perlre::{Flags, Regex};

/// Each case's matches, as Perl reports them: `-` for no match, else the
/// start and end of the match and of each group (`u` when undefined).
const REGEX_PL: &str = r#"
use strict; use warnings;
while (my $l = <STDIN>) {
    chomp $l;
    my ($p, $f, $s, $g) = split /\t/, $l, -1;
    $p = pack 'H*', $p; $s = pack 'H*', $s;
    my $re = eval { $f ne '' ? qr/(?$f)$p/ : qr/$p/ };
    if (!$re) { print "ERR\n"; next }
    my @out;
    if ($g) {
        while ($s =~ /$re/g) {
            push @out, join(' ', map { defined $-[$_] ? "$-[$_],$+[$_]" : 'u' } 0 .. $#+);
        }
        print join(' | ', @out), "\n";
    } elsif ($s =~ $re) {
        print join(' ', map { defined $-[$_] ? "$-[$_],$+[$_]" : 'u' } 0 .. $#+), "\n";
    } else {
        print "-\n";
    }
}
"#;

fn hex(s: &[u8]) -> String {
    s.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn render(c: &super::perlre::Captures) -> String {
    (0..c.len())
        .map(|i| {
            c.get(i)
                .map_or_else(|| "u".to_string(), |(s, e)| format!("{s},{e}"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run every (pattern, flags, subject, global) through Perl and the engine.
fn assert_regexes_match_perl(cases: &[(&[u8], &str, &[u8], bool)]) {
    let mut input = String::new();
    for (p, f, s, g) in cases {
        let _ = writeln!(
            input,
            "{}\t{f}\t{}\t{}",
            hex(p),
            hex(s),
            if *g { "1" } else { "" }
        );
    }
    let mut child = Command::new("perl")
        .arg("-e")
        .arg(REGEX_PL)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("perl is not installed: install it; missing tools fail tests, they never skip");
    std::io::Write::write_all(child.stdin.as_mut().expect("stdin"), input.as_bytes())
        .expect("write");
    let out = child.wait_with_output().expect("perl");
    let expected: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(expected.len(), cases.len(), "perl answered every case");
    let mut failures = Vec::new();
    for ((p, f, s, g), want) in cases.iter().zip(&expected) {
        let got = match Regex::new(p, Flags::parse(f)) {
            Err(_) => "ERR".to_string(),
            Ok(re) => {
                if *g {
                    re.find_all(s)
                        .iter()
                        .map(render)
                        .collect::<Vec<_>>()
                        .join(" | ")
                } else {
                    re.find(s).map_or_else(|| "-".to_string(), |c| render(&c))
                }
            }
        };
        if &got != want {
            failures.push(format!(
                "/{}/{f} on {:?}{}: perl {want}, engine {got}",
                String::from_utf8_lossy(p),
                String::from_utf8_lossy(s),
                if *g { " (g)" } else { "" }
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

struct Lcg(u64);

impl Lcg {
    const fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).expect("n")).expect("index")
    }
}

/// One to four of: a line deleted, duplicated or swapped, a character
/// deleted, a line's tabs turned to spaces or a space appended, a piece of
/// C inserted inside a line or as a line of its own.
fn mutate(source: &[u8], rng: &mut Lcg) -> Vec<u8> {
    const INSERTS: &[&[u8]] = &[
        b"{",
        b"}",
        b"(",
        b")",
        b"[",
        b"]",
        b";",
        b"'",
        b"\"",
        b"/",
        b"//",
        b"/*",
        b"*/",
        b"*",
        b"&",
        b"&&",
        b"||",
        b"<",
        b">>",
        b"=",
        b"==",
        b"!=",
        b"!",
        b"++",
        b"->",
        b",",
        b"?",
        b":",
        b"#",
        b"##",
        b"\n",
        b" ",
        b"\t",
        b"        ",
        b"\\",
        b"\\\n",
        b"\r",
        b"\x0c",
        b"\xc3\xa9",
        b"\xff",
        b"if(",
        b"else",
        b"} else {",
        b"while (",
        b"do {",
        b"} while (0);",
        b"case 1: x++;",
        b"default:",
        b"return (x);",
        b"goto out;",
        b"out:",
        b"printk(",
        b"pr_info(\"%s\\n\", __func__);",
        b"#define X(a) a+1",
        b"#define Z 1;",
        b"#if 0",
        b"#endif",
        b"EXPORT_SYMBOL(foo);",
        b"typedef int foo_t;",
        b"unsigned ",
        b"long long int ",
        b"static int x = 0;",
        b"__attribute__((packed))",
        b"sizeof x",
        b"%Lx",
        b"strcpy(",
        b"BUG_ON(x);",
        b"jiffies > ",
        b"volatile ",
        b"inline ",
        b"extern ",
        b"int foo();",
        b"fooBar",
        b"uint8_t ",
        b"char* p",
        b"(char *) p",
        b" ;",
        b"( ",
        b" )",
        b"a=b",
        b"a ,b",
        b"!= NULL",
        b"== true",
        b"memset(p, 1, 0);",
        b"/* fall through */",
        b"struct foo {",
        b"};",
        b"int x[0];",
        b"\"a\" \"b\"",
        b"S_IRUGO",
        b"DEVICE_ATTR(foo, 0644, foo_show, foo_store);",
        b"if ((a == b))",
        b"if (a = b)",
        b"x = f(), y = g();",
        b"&(x->y)",
        b"1 << 3",
        b"0x01 &&",
        b"kfree(p);",
        b"int *p __free(kfree);",
        b"likely(x) > 0",
        b"the the ",
        b"// SPDX-License-Identifier: GPL-2.0\n",
    ];
    let mut data = source.to_vec();
    for _ in 0..=rng.below(4) {
        let mut lines: Vec<Vec<u8>> = data.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
        let n = lines.len();
        match rng.below(8) {
            0 if n > 1 => {
                lines.remove(rng.below(n));
            }
            1 => {
                let i = rng.below(n);
                lines.insert(i, lines[i].clone());
            }
            2 if n > 1 => lines.swap(rng.below(n), rng.below(n)),
            3 if !data.is_empty() => {
                data.remove(rng.below(data.len()));
                continue;
            }
            4 => {
                let i = rng.below(n);
                if rng.below(2) == 0 {
                    lines[i] = lines[i]
                        .iter()
                        .flat_map(|&b| {
                            if b == b'\t' {
                                b"        ".to_vec()
                            } else {
                                vec![b]
                            }
                        })
                        .collect();
                } else {
                    lines[i].push(b' ');
                }
            }
            5 => lines.insert(rng.below(n), INSERTS[rng.below(INSERTS.len())].to_vec()),
            _ => {
                let at = rng.below(data.len() + 1);
                let insert = INSERTS[rng.below(INSERTS.len())];
                data.splice(at..at, insert.iter().copied());
                continue;
            }
        }
        data = lines.join(&b'\n');
    }
    if rng.below(10) == 0 {
        while data.last() == Some(&b'\n') {
            data.pop();
        }
    }
    data
}

/// The fixtures, as paths relative to `testdata/`.
fn fixtures() -> Vec<String> {
    fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("testdata") {
            let entry = entry.expect("entry");
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.file_type().expect("type").is_dir() {
                walk(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/checkpatch/testdata"),
        "",
        &mut files,
    );
    files.sort();
    files
}

/// What `checkpatch.pl --no-tree -f` prints for each file, run in `dir`;
/// `None` where it died.
fn checkpatch_outputs(dir: &std::path::Path, files: &[String]) -> Vec<Option<Vec<u8>>> {
    let jobs = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunks: Vec<&[String]> = files.chunks(files.len().div_ceil(jobs).max(1)).collect();
    std::thread::scope(|s| {
        // Every thread is spawned before any is joined: that is the point.
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|f| {
                            let out = Command::new("checkpatch.pl")
                                .args(["--no-tree", "-f", f])
                                .current_dir(dir)
                                .stderr(std::process::Stdio::null())
                                .output()
                                .expect("checkpatch.pl is not installed: run `rsconstruct tool install checkpatch.pl`; missing tools fail tests, they never skip");
                            // Perl dies with no output and whatever errno held.
                            let died = out.stdout.is_empty() && !out.status.success();
                            (!died).then_some(out.stdout)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("checkpatch thread"))
            .collect()
    })
}

/// The engine and checkpatch.pl agree, byte for byte, on the fixtures and
/// on mutants of them, under `src/` and under `net/` (where checkpatch
/// turns its CHECKs on).
#[test]
fn checks_match_checkpatch() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let testdata =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/checkpatch/testdata");
    let mut files = Vec::new();
    let mut rng = Lcg(2026);
    for f in fixtures() {
        let content = std::fs::read(testdata.join(&f)).expect("fixture");
        let path = dir.path().join(&f);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, &content).expect("write");
        files.push(f.clone());
        for k in 0..12 {
            let sub = ["src", "net", "drivers/staging"][k % 3];
            let base = f.rsplit('/').next().expect("name");
            let name = format!("mut/{sub}/{k}-{base}");
            let path = dir.path().join(&name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, mutate(&content, &mut rng)).expect("write");
            files.push(name);
        }
    }
    let expected = checkpatch_outputs(dir.path(), &files);
    let mut failures = Vec::new();
    for (f, want) in files.iter().zip(&expected) {
        let path = dir.path().join(f);
        let content = std::fs::read(&path).expect("read");
        let mtime = super::timestamp(
            std::fs::metadata(&path)
                .expect("stat")
                .modified()
                .expect("mtime"),
        );
        let got = match super::check::Checker::check_file(f.as_bytes(), &content, 4096, &mtime) {
            super::check::Outcome::Report { output, clean } => {
                let want_clean = want.as_ref().is_some_and(|w| {
                    w.is_empty()
                        || String::from_utf8_lossy(w).contains("has no obvious style problems")
                });
                if clean != want_clean {
                    failures.push(format!(
                        "{f}: the engine says clean = {clean}, checkpatch.pl {want_clean}"
                    ));
                }
                Some(output)
            }
            super::check::Outcome::Fatal(_) => None,
        };
        if &got != want {
            failures.push(format!(
                "{f}:\n--- checkpatch.pl\n{}\n--- engine\n{}",
                want.as_ref().map_or_else(
                    || "(died)".into(),
                    |w| String::from_utf8_lossy(w).into_owned()
                ),
                got.as_ref().map_or_else(
                    || "(died)".into(),
                    |g| String::from_utf8_lossy(g).into_owned()
                )
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

/// checkpatch.pl's source, from the `checkpatch.pl` on PATH.
fn checkpatch_source() -> String {
    let out = Command::new("sh")
        .arg("-c")
        .arg("command -v checkpatch.pl")
        .output()
        .expect("sh");
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        !path.is_empty(),
        "checkpatch.pl is not installed: run `rsconstruct tool install checkpatch.pl`; missing tools fail tests, they never skip"
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

/// checkpatch.pl, stopped once its globals are set up, printing the named
/// ones (read from stdin) in hex.
fn globals_dumper() -> String {
    let source = checkpatch_source();
    let at = source
        .find("\nour %allow_repeated_words")
        .expect("checkpatch.pl defines %allow_repeated_words after its patterns");
    format!(
        "{}\n{{ no strict 'refs'; while (my $n = <STDIN>) {{ chomp $n; print unpack('H*', ${{\"main::$n\"}}), \"\\n\" }} exit 0; }}\n{}",
        &source[..at],
        &source[at..]
    )
}

#[test]
fn globals_match_checkpatch() {
    let vars = super::globals::Vars::new();
    let names = [
        "Ident",
        "Storage",
        "Sparse",
        "InitAttributePrefix",
        "InitAttributeData",
        "InitAttributeConst",
        "InitAttributeInit",
        "InitAttribute",
        "Attribute",
        "Inline",
        "Member",
        "Lval",
        "Int_type",
        "Binary",
        "Hex",
        "Int",
        "Octal",
        "String",
        "Float_hex",
        "Float_dec",
        "Float_int",
        "Float",
        "Constant",
        "Assignment",
        "Compare",
        "Arithmetic",
        "Operators",
        "c90_Keywords",
        "NON_ASCII_UTF8",
        "UTF8",
        "typeC99Typedefs",
        "typeOtherOSTypedefs",
        "typeKernelTypedefs",
        "typeStdioTypedefs",
        "typeTypedefs",
        "zero_initializer",
        "logFunctions",
        "allocFunctions",
        "signature_tags",
        "link_tags_search",
        "link_tags_print",
        "tracing_logging_tags",
        "dev_id_types",
        "obsolete_archives",
        "C90_int_types",
        "mode_perms_search",
        "mode_perms_world_writable",
        "allowed_asm_includes",
        "Modifier",
        "BasicType",
        "NonptrType",
        "NonptrTypeMisordered",
        "NonptrTypeWithAttr",
        "Type",
        "TypeMisordered",
        "Declare",
        "DeclareMisordered",
        "Typecast",
        "balanced_parens",
        "LvalOrFunc",
        "FuncArg",
        "declaration_macros",
    ];
    let script = std::env::temp_dir().join(format!(
        "rsconstruct-checkpatch-globals-{}.pl",
        std::process::id()
    ));
    std::fs::write(&script, globals_dumper()).expect("write dumper");
    let mut child = Command::new("perl")
        .arg(&script)
        .args(["--no-tree", "-f", "/dev/null"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("perl is not installed: install it; missing tools fail tests, they never skip");
    std::io::Write::write_all(
        child.stdin.as_mut().expect("stdin"),
        format!("{}\n", names.join("\n")).as_bytes(),
    )
    .expect("write");
    let out = child.wait_with_output().expect("perl");
    let _ = std::fs::remove_file(&script);
    let perl: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(perl.len(), names.len(), "checkpatch printed every variable");
    let mut failures = Vec::new();
    for (name, want) in names.iter().zip(&perl) {
        if let Err(e) = Regex::new(vars.val(name), Flags::parse("")) {
            failures.push(format!("${name} does not compile: {e}"));
        }
        let got = hex(vars.val(name));
        if &got != want {
            failures.push(format!(
                "${name}:\n  checkpatch {:?}\n  port       {:?}",
                String::from_utf8_lossy(&hex_decode(want)),
                String::from_utf8_lossy(vars.val(name))
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn regexes_match_perl() {
    let cases: &[(&[u8], &str, &[u8], bool)] = &[
        (b"a|ab", "", b"xab", false),
        (b"(a|ab)(c|bcd)(d*)", "", b"abcd", false),
        (b"a*?b", "", b"aaab", false),
        (b"(a+?)(a*)", "", b"aaa", false),
        (b"a++a", "", b"aaaa", false),
        (b"(?>a+)b", "", b"aaab", false),
        (b"(?>a|ab)c", "", b"abc", false),
        (b"(\\((?:[^\\(\\)]++|(?-1))*\\))", "", b"f(a, (b), g(c(d)))x", false),
        (b"\\w+\\s*(\\((?:[^\\(\\)]++|(?-1))*\\))\\s*;", "", b"foo(a(b)c) ;", false),
        (b"(\\((?:[^\\(\\)]++|(?-1))*\\))", "", b"((a)", false),
        (b"(?<=foo)bar", "", b"foobar", false),
        (b"(?<!foo)bar", "", b"foobar xbar", false),
        (b"(?<=a|bc)d", "", b"bcd", false),
        (b"(?=(\\w+))\\w", "", b"abc", false),
        (b"(?!a)\\w", "", b"abc", false),
        (b"(\\w)\\1", "", b"abccd", false),
        (b"(\\w)\\g{-1}", "", b"abccd", false),
        (b"(?<x>\\w)\\k<x>", "", b"abccd", false),
        (b"^a$", "m", b"b\na\nc", false),
        (b"^$", "m", b"a\n", false),
        (b"^$", "m", b"a\n\nb", false),
        (b"a$", "", b"a\n", false),
        (b"a$", "", b"a\nb", false),
        (b"a\\Z", "", b"a\n", false),
        (b"a\\z", "", b"a\n", false),
        (b"a.b", "", b"a\nb", false),
        (b"a.b", "s", b"a\nb", false),
        (b"\\bfoo\\b", "", b"afoo foo", false),
        (b"\\Bfoo", "", b"foo afoo", false),
        (b"FOO", "i", b"xfoo", false),
        (b"[a-c]+", "i", b"xABCd", false),
        (b"[^a-c]+", "i", b"ABxyz", false),
        (b"[[:alpha:]]+", "", b"12ab3", false),
        (b"[[:^digit:]]+", "", b"12ab3", false),
        (b"[\\w.]+", "", b"--a.b_c--", false),
        (b"[]a]+", "", b"x]a]", false),
        (b"[a-]+", "", b"x-a-", false),
        (b"a{2,3}", "", b"aaaa", false),
        (b"a{2,}?", "", b"aaaa", false),
        (b"a{,2}", "", b"aaaa", false),
        (b"a{x}", "", b"a{x}", false),
        (b"x{0,1}y", "", b"xy", false),
        (b"(a|b)*c", "", b"ababc", false),
        (b"(a*)*b", "", b"aab", false),
        (b"(a|)*b", "", b"aab", false),
        (b" a b # comment\n c", "x", b"abc", false),
        (b"[ ]a", "x", b" a", false),
        (b"\\ a", "x", b" a", false),
        (b"(?^x: a b )c", "", b"abc", false),
        (b"(?^:a b)", "x", b"a b", false),
        (b"(?i)ab(?-i)c", "", b"ABc ABC", false),
        (b"(?i:a)b", "", b"Ab AB", false),
        (b"a(?#comment)b", "", b"ab", false),
        (b"\\x41\\x{42}\\101\\t", "", b"AB\x41\t", false),
        (b"\\x41\\x{42}\\t", "", b"xAB\t", false),
        (b"\\s+", "", b"a \x0b\x0c b", false),
        (b"\\h+", "", b"a \t\xa0b", false),
        (b"\\d+", "", b"a12b", false),
        (b"\\W+", "", b"ab, cd", false),
        (b"(a)|(b)", "", b"b", false),
        (b"(a)?b", "", b"b", false),
        (b"(?:(a)|b)+", "", b"ab", false),
        (b"(a)+", "", b"aaa", false),
        (b"\\w*", "", b"ab cd", true),
        (b"x*", "", b"axxb", true),
        (b"(\\w)(\\d)?", "", b"a1b c", true),
        (b"", "", b"abc", true),
        (b"\\b", "", b"ab cd", true),
        (b"(?:a|b)+?c", "", b"ababc", false),
        (b"\\$\\@\\%\\&", "", b"$@%&", false),
        (b"\\Qa.b\\E", "", b"a.b", false),
        (b"[\\$\\@]", "", b"x@", false),
        (b"[\\x80-\\xff]+", "", b"a\xc3\xa9b", false),
        (b"\\.\\s*$", "", b"end. \n", false),
        (b"^\\+.*\\bfoo\\b", "", b"+ a foo", false),
        (b"((?:\\s*\\#\\#\\s*[A-Za-z_][A-Za-z\\d_]*)*)", "", b" ## a ## b", false),
        (b"(?:(?:\\s|\\*|\\[\\])+\\s*const|(?:\\s|\\*\\s*(?:const\\s*)?|\\[\\])+|(?:\\s*\\[\\s*\\])+){0,4}", "x", b" * const *", false),
        (b"^.\\s*(?:if|while)\\s*\\(", "", b"+\tif (x)", false),
        (b"(a)(?=(b))", "", b"ab", false),
        (b"(?!(a))\\w", "", b"ab", false),
        (b"(?<=(a))b", "", b"ab", false),
        (b"a(?=b)|ac", "", b"ac", false),
        (b"(?:a(?=b))+", "", b"abab", false),
        (b"(x)?(?(1)y|z)", "", b"z", false),
        (b"(?:(a)|b)+", "", b"a", false),
        (b"(?:(a)|b|(c))+", "", b"cab", false),
        (b"(?:(c)|b|(a))+", "", b"cab", false),
        (b"(?:(a)|(b)|(c))+", "", b"cba", false),
        (b"(?:(x)|(a)|b)+", "", b"xab", false),
        (b"(?:(?:(a)c)?a)+", "", b"acaa", false),
        (b"(?:(a)c|a)+", "", b"acab", false),
        (b"(?:(a)(c)?|b)+", "", b"acab", false),
        (b"(?:(a)|b)+c", "", b"abd abc", false),
        (b"((a)|b)+", "", b"ab", false),
    ];
    assert_regexes_match_perl(cases);
}
