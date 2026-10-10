//! The port checked against PPI and `Perl::Critic` themselves: the Perl
//! files in `testdata/` (each `caseNNN.pl` also as a `.pm`, a module to
//! the policies that care) and mutated copies of them are parsed by both,
//! tree against tree with every token's location, and critiqued by both
//! under several profiles, report against report.
//!
//! `Perl::Critic` 1.156 and PPI 1.284, the versions ported, must be
//! installed (CI installs them with perlcritic): a missing or different
//! version fails these tests, it never skips them.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::ppi::{DOCUMENT, Doc, parse};
use super::{Outcome, check_file, profile};

const PERL_CRITIC_VERSION: &str = "1.156";
const PPI_VERSION: &str = "1.284";

/// Mutated copies made of each fixture.
const MUTANTS_PER_FIXTURE: usize = 4;

/// The tree of each file, one element per line, as `dump_tree` prints it.
const DUMP_PL: &str = r#"
use strict; use warnings; use PPI;
sub esc { my $s = shift; my $o = "'"; for my $c (split //, $s) { my $n = ord $c;
  if ($c eq "\n") { $o .= "\\n" } elsif ($c eq "\t") { $o .= "\\t" } elsif ($c eq "\\") { $o .= "\\\\" }
  elsif ($c eq "'") { $o .= "\\'" } elsif ($n >= 0x20 && $n <= 0x7e) { $o .= $c } else { $o .= sprintf("\\x%02x", $n) } }
  return "$o'"; }
sub dump_el { my ($e, $d) = @_; my $in = "  " x $d;
  if ($e->isa('PPI::Token')) { my $l = $e->{_location}; my $loc = $l ? " \@$l->[0]:$l->[1]:$l->[3]" : "";
    print "$in", ref($e), " ", esc($e->{content}), "$loc\n"; return }
  print "$in", ref($e), "\n";
  if ($e->isa('PPI::Structure')) { dump_el($e->start, $d+1) if $e->start; }
  dump_el($_, $d+1) for @{$e->{children}};
  if ($e->isa('PPI::Structure')) { dump_el($e->finish, $d+1) if $e->finish; } }
for my $f (@ARGV) { print "== $f\n"; my $doc = PPI::Document->new($f);
  if (!$doc) { print "ERROR: ", PPI::Document::errstr(), "\n"; next }
  $doc->index_locations; print "PPI::Document\n"; dump_el($_, 1) for @{$doc->{children}}; }
"#;

/// Each file's report, as `perlcritic --profile PROFILE FILE...` prints it
/// on stdout, its `Problem while critiquing` warning, or FATAL where a
/// policy dies (which ends a perlcritic run).
const CRITIC_PL: &str = r#"
use strict; use warnings;
use Perl::Critic; use Perl::Critic::Violation; use Perl::Critic::Utils qw(verbosity_to_format);
my ($profile, @files) = @ARGV;
my $critic = Perl::Critic->new(-profile => $profile);
my $verbosity = $critic->config->verbose();
$verbosity = 5 if $verbosity eq '4';
Perl::Critic::Violation::set_format(verbosity_to_format($verbosity));
for my $f (@files) {
    print "== $f\n";
    my @v = eval { $critic->critique($f) };
    if (Perl::Critic::Exception::Parse->caught()) { print "Problem while critiquing \"$f\": $@\n"; }
    elsif ($@) { print "FATAL\n"; }
    else { print @v; }
}
"#;

fn testdata_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/perlcritic/testdata")
}

/// Every fixture: its path under `testdata/` and contents.
fn fixtures() -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("read testdata") {
            let path = entry.expect("testdata entry").path();
            let name = format!(
                "{prefix}{}",
                path.file_name().expect("name").to_string_lossy()
            );
            if path.is_dir() {
                walk(&path, &format!("{name}/"), out);
            } else {
                out.push((name, std::fs::read(&path).expect("read fixture")));
            }
        }
    }
    let mut files = Vec::new();
    walk(&testdata_dir(), "", &mut files);
    files.sort();
    files
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
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One to three of: a line deleted, duplicated or swapped, a character
/// deleted, a stretch doubled, a piece of Perl inserted. PPI's guesses
/// (regex or division, hash or block, prototype or signature) and the
/// policies' corner cases are where a port diverges first.
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
        b"`",
        b"/",
        b"<",
        b"<<",
        b"<<EOF\n",
        b"<<~EOT\n",
        b"=",
        b"#",
        b"$",
        b"@",
        b"%",
        b"&",
        b"*",
        b"-",
        b"::",
        b"x",
        b" x ",
        b"q{",
        b"qq(",
        b"qw(",
        b"s/",
        b"tr/",
        b"m{",
        b"qr<",
        b"sub ",
        b"sub foo ",
        b"sub (",
        b"my ",
        b"local ",
        b"if ",
        b"foreach ",
        b"for my $x ",
        b"=pod\n",
        b"=cut\n",
        b"__END__\n",
        b"__DATA__\n",
        b"\n",
        b" ",
        b"\\",
        b"->",
        b"=>",
        b",",
        b".",
        b"..",
        b"?",
        b":",
        b"$_",
        b"@_",
        b"$#a",
        b"${",
        b"$$",
        b"0777",
        b"0x1F",
        b"1.5e3",
        b"v5.10",
        b"open(FH, $f);",
        b"opendir(DH, $d);",
        b"return undef;",
        b"return sort @a;",
        b"eval $x;",
        b"bless {};",
        b"select(undef, undef, undef, 0.1);",
        b"<*.c>",
        b"-t STDIN",
        b"binmode $fh, ':utf8';",
        b"no strict 'refs';",
        b"use strict;",
        b"use 5.012;",
        b"use v5.36;",
        b"require 'foo.pl';",
        b"package Foo::Bar;",
        b"map { $_++ } @a;",
        b"my $x = 1 if $y;",
        b"for $i (@a) {}",
        b"sub foo($$) {}",
        b"use feature 'signatures';",
        b"use feature 'try';",
        b"try {",
        b"use Switch;",
        b"## no critic\n",
        b"## no critic (RequireUseStrict)\n",
        b"## use critic\n",
        b"#line 10 \"x.pl\"\n",
        b"\xc3\xa9",
        b"BEGIN {",
        b"LABEL: ",
        b"print <<'X';\nfoo\nX\n",
        b"$a->@*",
        b": ",
    ];
    let mut data = source.to_vec();
    for _ in 0..=rng.below(3) {
        let mut lines: Vec<Vec<u8>> = data.split(|&c| c == b'\n').map(<[u8]>::to_vec).collect();
        match rng.below(7) {
            0 if lines.len() > 1 => {
                lines.remove(rng.below(lines.len()));
                data = lines.join(&b'\n');
            }
            1 => {
                let i = rng.below(lines.len());
                let copy = lines[i].clone();
                lines.insert(i, copy);
                data = lines.join(&b'\n');
            }
            2 if lines.len() > 1 => {
                let (i, j) = (rng.below(lines.len()), rng.below(lines.len()));
                lines.swap(i, j);
                data = lines.join(&b'\n');
            }
            3 if !data.is_empty() => {
                data.remove(rng.below(data.len()));
            }
            4 if !data.is_empty() => {
                let i = rng.below(data.len());
                let j = (i + 1 + rng.below(40)).min(data.len());
                let stretch = data[i..j].to_vec();
                data.splice(i..i, stretch);
            }
            _ => {
                let i = rng.below(data.len() + 1);
                let insert = INSERTS[rng.below(INSERTS.len())];
                data.splice(i..i, insert.iter().copied());
            }
        }
    }
    data
}

/// The fixtures (and mutants of them), written to a fresh directory; the
/// file names are relative to it.
struct Corpus {
    dir: tempfile::TempDir,
    files: Vec<String>,
}

fn corpus(mutants: bool) -> Corpus {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut rng = Lcg(0x5EED_CAFE);
    let mut files = Vec::new();
    let mut write = |name: String, bytes: &[u8]| {
        let path = dir.path().join(&name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&path, bytes).expect("write");
        files.push(name);
    };
    for (name, contents) in fixtures() {
        let mut names = vec![name.clone()];
        if let Some(stem) = name.strip_suffix(".pl").filter(|s| s.starts_with("case")) {
            names.push(format!("{stem}.pm"));
        }
        for name in names {
            write(name.clone(), &contents);
            if mutants {
                for i in 0..MUTANTS_PER_FIXTURE {
                    write(format!("mutants/M{i}-{name}"), &mutate(&contents, &mut rng));
                }
            }
        }
    }
    files.sort();
    Corpus { dir, files }
}

/// Run a Perl script over the corpus; its output, by file.
fn run_perl(script: &str, args: &[&str], corpus: &Corpus) -> BTreeMap<String, String> {
    let check = |module: &str, expected: &str| {
        let out = Command::new("perl")
            .args([format!("-M{module}"), "-e".to_string(), format!("print ${module}::VERSION")])
            .output()
            .unwrap_or_else(|e| panic!("perl is not installed ({e}): install it; missing tools fail tests, they never skip"));
        let version = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            version,
            expected,
            "the engine ports {module} {expected}, the installed one is {version:?} (stderr: {})",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    check("Perl::Critic", PERL_CRITIC_VERSION);
    check("PPI", PPI_VERSION);
    let script_path = corpus.dir.path().join("reference.pl");
    std::fs::write(&script_path, script).expect("write the script");
    let out = Command::new("perl")
        .current_dir(corpus.dir.path())
        .arg(&script_path)
        .args(args)
        .args(&corpus.files)
        .output()
        .expect("run perl");
    assert!(
        out.status.success(),
        "the reference script failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    split_by_file(&String::from_utf8_lossy(&out.stdout))
}

fn split_by_file(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.split_inclusive('\n') {
        if let Some(name) = line.strip_prefix("== ") {
            if let Some((file, body)) = current.take() {
                out.insert(file, body);
            }
            current = Some((name.trim_end_matches('\n').to_string(), String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
        }
    }
    if let Some((file, body)) = current {
        out.insert(file, body);
    }
    out
}

/// The tree of a source, one element per line, as `DUMP_PL` prints PPI's.
fn dump_tree(source: &[u8], name: &str) -> String {
    match parse(source, name.as_bytes()) {
        Ok(doc) => {
            let mut out = String::new();
            dump_node(&doc, DOCUMENT, 0, &mut out);
            out
        }
        Err(e) => format!("ERROR: {e}\n"),
    }
}

fn dump_node(doc: &Doc, id: usize, depth: usize, out: &mut String) {
    let elem = &doc.elems[id];
    let indent = "  ".repeat(depth);
    if elem.class.is_token() {
        let loc = elem
            .location
            .map(|l| format!(" @{}:{}:{}", l.line, l.character, l.logical_line))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{indent}{} {}{loc}",
            elem.class.name(),
            escape(&elem.content)
        );
        return;
    }
    let _ = writeln!(out, "{indent}{}", elem.class.name());
    if let Some(start) = elem.start {
        dump_node(doc, start, depth + 1, out);
    }
    for &child in &elem.children {
        dump_node(doc, child, depth + 1, out);
    }
    if let Some(finish) = elem.finish {
        dump_node(doc, finish, depth + 1, out);
    }
}

fn escape(s: &[u8]) -> String {
    let mut out = String::from("'");
    for &c in s {
        match c {
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            0x20..=0x7e => out.push(c as char),
            _ => {
                let _ = write!(out, "\\x{c:02x}");
            }
        }
    }
    out.push('\'');
    out
}

/// The engine's report on a file, in `CRITIC_PL`'s form.
fn engine_report(name: &str, source: &[u8], profile: &profile::Profile) -> String {
    match check_file(name, source, profile) {
        Outcome::Violations(lines) => String::from_utf8_lossy(&lines.concat()).into_owned(),
        Outcome::ParseError(message) => format!("{message}\n"),
        Outcome::Fatal(_) => "FATAL\n".to_string(),
    }
}

fn assert_same(expected: &BTreeMap<String, String>, actual: &BTreeMap<String, String>, what: &str) {
    let differences: Vec<String> = expected
        .iter()
        .filter(|(file, e)| actual.get(*file) != Some(e))
        .map(|(file, e)| {
            format!(
                "{file}\n--- reference\n{e}--- engine\n{}",
                actual.get(file).map_or("(none)", String::as_str)
            )
        })
        .collect();
    assert!(
        differences.is_empty() && expected.len() == actual.len(),
        "{what}: {} of {} files differ:\n{}",
        differences.len(),
        expected.len(),
        differences.join("\n")
    );
}

fn critique_matches_perl_critic(profile_text: &str, mutants: bool) {
    let corpus = corpus(mutants);
    let profile_path = corpus.dir.path().join("profile.rc");
    std::fs::write(&profile_path, profile_text).expect("write the profile");
    let expected = run_perl(
        CRITIC_PL,
        &[profile_path.to_str().expect("utf-8 path")],
        &corpus,
    );
    let profile = profile::load(profile_text).expect("the profile");
    let actual: BTreeMap<String, String> = corpus
        .files
        .iter()
        .map(|name| {
            let source = std::fs::read(corpus.dir.path().join(name)).expect("read");
            (name.clone(), engine_report(name, &source, &profile))
        })
        .collect();
    assert_same(&expected, &actual, "perlcritic");
}

#[test]
fn trees_match_ppi() {
    let corpus = corpus(true);
    let expected = run_perl(DUMP_PL, &[], &corpus);
    let actual: BTreeMap<String, String> = corpus
        .files
        .iter()
        .map(|name| {
            let source = std::fs::read(corpus.dir.path().join(name)).expect("read");
            (name.clone(), dump_tree(&source, name))
        })
        .collect();
    assert_same(&expected, &actual, "PPI");
}

/// The fleet's shared `.perlcriticrc`.
#[test]
fn fleet_profile_matches_perl_critic() {
    critique_matches_perl_critic(
        "# The fleet's profile.\nseverity = 5\nverbose = %f:%l:%c: [%p] %m\\n\n",
        true,
    );
}

/// Every format field, widths and truncation included.
#[test]
fn format_fields_match_perl_critic() {
    critique_matches_perl_critic(
        "verbose = %f|%F|%g|%G|%l|%L|%c|%C|%s|%p|%P|%m|%e|%r|%-30m|%5l|%.6p|%{x}s|%%|%n|%t|%q\\n\n",
        false,
    );
}

/// Policy parameters, a disabled policy, and severities.
#[test]
fn policy_parameters_match_perl_critic() {
    critique_matches_perl_critic(
        "severity = 5
verbose = 8

[BuiltinFunctions::ProhibitStringyEval]
allow_includes = 1

[ValuesAndExpressions::ProhibitLeadingZeros]
strict = 1

[TestingAndDebugging::ProhibitNoStrict]
allow = refs VARS

[TestingAndDebugging::RequireUseStrict]
equivalent_modules = My::Strict Foo
maximum_violations_per_document = 3

[Modules::ProhibitEvilModules]
modules = Foo::Bar /^Evil/ {Evil found} Switch

[ControlStructures::ProhibitMutatingListFunctions]
add_list_funcs = mymap first

[-InputOutput::ProhibitTwoArgOpen]

[InputOutput::ProhibitInteractiveTest]
severity = 4

[Perl::Critic::Policy::Subroutines::ProhibitNestedSubs]
severity = gentle
",
        true,
    );
}

/// `only`, `include`, `exclude`, `force`, `top`, program extensions and a
/// numbered format.
#[test]
fn global_options_match_perl_critic() {
    critique_matches_perl_critic(
        "only = 1
include = NoStrict ReturnSort
exclude = TwoArg
verbose = 9

[Subroutines::ProhibitExplicitReturnUndef]
[InputOutput::ProhibitTwoArgOpen]
[TestingAndDebugging::RequireUseStrict]
[Modules::RequireFilenameMatchesPackage]
",
        false,
    );
    critique_matches_perl_critic(
        "severity = gentle\nforce = 1\ntop = 3\nprogram-extensions = .pm\nverbose = 5\n",
        false,
    );
}

/// The fixtures cover what they are for: every policy reports somewhere,
/// a file fails to parse, and a policy dies.
#[test]
fn fixtures_exercise_every_policy_and_failure() {
    let corpus = corpus(false);
    let profile = profile::load("verbose = [%p]\\n\n").expect("the profile");
    let mut all = String::new();
    for name in &corpus.files {
        let source = std::fs::read(corpus.dir.path().join(name)).expect("read");
        all.push_str(&engine_report(name, &source, &profile));
    }
    for policy in super::policies::Policy::ALL {
        assert!(
            all.contains(&format!("[{}]", policy.short_name())),
            "no fixture triggers {}",
            policy.short_name()
        );
    }
    assert!(
        all.contains("Can't parse code: Encountered unexpected character"),
        "no fixture fails to parse"
    );
    assert!(all.contains("FATAL"), "no fixture makes a policy die");
}

#[test]
fn profiles_enabling_unported_policies_are_errors() {
    let err = profile::load("severity = 4\n").expect_err("severity 4 enables unported policies");
    assert!(
        format!("{err:#}").contains("does not implement policy"),
        "{err:#}"
    );
    let err = profile::load("[CodeLayout::ProhibitHardTabs]\nseverity = 5\n")
        .expect_err("an unported policy raised to the threshold");
    assert!(
        format!("{err:#}").contains("CodeLayout::ProhibitHardTabs"),
        "{err:#}"
    );
    let err = profile::load("theme = core\n").expect_err("themes");
    assert!(format!("{err:#}").contains("theme"), "{err:#}");
    let err = profile::load("colour-blind = 1\n").expect_err("an unknown option");
    assert!(format!("{err:#}").contains("colour-blind"), "{err:#}");
    // Naming a policy enables it, but its severity still has to reach
    // the threshold (Perl::Critic's `_load_policies` checks it after).
    profile::load("[CodeLayout::ProhibitHardTabs]\nmax_tabs = 2\n").expect("below the threshold");
    profile::load("[-CodeLayout::ProhibitHardTabs]\n").expect("a disabled unported policy");
    profile::load("severity = 4\n[-CodeLayout::ProhibitHardTabs]\n")
        .expect_err("other severity-4 policies");
}
