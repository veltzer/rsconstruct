//! The checker: checkpatch.pl's state, its report functions and the
//! regular-expression plumbing every check goes through.
//!
//! Patterns are written as checkpatch.pl writes them and interpolated the
//! way Perl does, against the current pattern variables. A pattern that
//! interpolates one of the type patterns `possible()` rebuilds is compiled
//! again after each rebuild; every other pattern is compiled once.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::globals::Vars;
use super::perlre::{Flags, Regex};
use super::re::{Caps, interp};

/// The stack of the thread a file is checked on.
const STACK_SIZE: usize = 1 << 30;

type Fast = std::hash::BuildHasherDefault<super::perlre::FastHasher>;
type PatternCache = HashMap<(usize, usize, usize, u64), Rc<Regex>, Fast>;
type TextCache = HashMap<(Vec<u8>, &'static str), Rc<Regex>>;

/// The pattern variables `build_types` recomputes.
const DYNAMIC: &[&str] = &[
    "Modifier",
    "BasicType",
    "NonptrType",
    "NonptrTypeMisordered",
    "NonptrTypeWithAttr",
    "Type",
    "TypeMisordered",
    "Declare",
    "DeclareMisordered",
];

/// What checking one file produced.
pub enum Outcome {
    /// What checkpatch.pl prints on stdout, and whether the file is clean.
    Report { output: Vec<u8>, clean: bool },
    /// checkpatch.pl died (a pattern built from the file did not compile).
    Fatal(String),
}

pub struct Checker {
    pub(super) v: Vars,
    /// The generation of the type patterns: bumped when `possible()`
    /// changes them.
    generation: u64,
    generations: u64,
    /// Compiled patterns by the template's address and length, the flags'
    /// address and the generation.
    cache: RefCell<PatternCache>,
    text_cache: RefCell<TextCache>,
    dynamic: RefCell<HashMap<(usize, usize), bool, Fast>>,
    fatal: RefCell<Option<String>>,

    // checkpatch.pl's globals.
    pub(super) av_preprocessor: bool,
    pub(super) av_pending: u8,
    pub(super) av_paren_type: Vec<u8>,
    pub(super) av_pend_colon: u8,
    pub(super) sanitise_quote: &'static [u8],
    pub(super) report: Vec<Vec<u8>>,
    pub(super) clean: bool,
    pub(super) cnt_error: u64,
    pub(super) cnt_warn: u64,
    pub(super) cnt_chk: u64,
    pub(super) cnt_lines: u64,
    pub(super) check: bool,
    pub(super) emitted_corrupt: u64,
    pub(super) camelcase: HashSet<Vec<u8>>,
    pub(super) maybe_linker_symbol: HashMap<Vec<u8>, u64>,
    pub(super) rpt_cleaners: bool,
    pub(super) rawlines: Vec<Vec<u8>>,
    pub(super) lines: Vec<Vec<u8>>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            v: Vars::new(),
            generation: 1,
            generations: 1,
            cache: RefCell::new(HashMap::default()),
            text_cache: RefCell::new(HashMap::new()),
            dynamic: RefCell::new(HashMap::default()),
            fatal: RefCell::new(None),
            av_preprocessor: false,
            av_pending: b'_',
            av_paren_type: vec![b'E'],
            av_pend_colon: b'O',
            sanitise_quote: b"",
            report: Vec::new(),
            clean: true,
            cnt_error: 0,
            cnt_warn: 0,
            cnt_chk: 0,
            cnt_lines: 0,
            check: false,
            emitted_corrupt: 0,
            camelcase: HashSet::new(),
            maybe_linker_symbol: HashMap::new(),
            rpt_cleaners: false,
            rawlines: Vec::new(),
            lines: Vec::new(),
        }
    }

    /// Check one file as `checkpatch.pl --no-tree -f NAME` does: `name` as
    /// given on its command line, `block` the size of diff's first read
    /// (the file's block size), within which a NUL byte makes it binary,
    /// and `mtime` the file's modification time as diff labels it
    /// (`timestamp`), which a message about the first line can quote.
    ///
    /// The matcher recurses as deep as the text a pattern loops over, so
    /// the check runs on a thread of its own with a stack sized for the
    /// longest statements; the memory is reserved, not used, until needed.
    pub fn check_file(name: &[u8], content: &[u8], block: usize, mtime: &str) -> Outcome {
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .name("checkpatch".to_string())
                .stack_size(STACK_SIZE)
                .spawn_scoped(s, || Self::check_file_here(name, content, block, mtime))
                .map_or_else(
                    |e| Outcome::Fatal(format!("cannot start a checkpatch thread: {e}")),
                    |t| {
                        t.join().unwrap_or_else(|panic| {
                            std::panic::resume_unwind(panic);
                        })
                    },
                )
        })
    }

    fn check_file_here(name: &[u8], content: &[u8], block: usize, mtime: &str) -> Outcome {
        let mut c = Self::new();
        c.rawlines = super::diff::rawlines(name, content, block, mtime);
        if c.rawlines.is_empty() {
            // checkpatch.pl exits 0, silently, on an empty input.
            return Outcome::Report {
                output: Vec::new(),
                clean: true,
            };
        }
        let clean = c.process(name);
        if let Some(e) = c.fatal.borrow_mut().take() {
            return Outcome::Fatal(e);
        }
        let mut output = c.output_tail(name, clean);
        if !clean {
            output.extend_from_slice(
                b"\nNOTE: If any of the errors are false positives, please report\n      them to the maintainer, see CHECKPATCH in MAINTAINERS.\n",
            );
        }
        Outcome::Report { output, clean }
    }

    /// After `process`: the report, its summary and the closing notes.
    fn output_tail(&mut self, name: &[u8], clean: bool) -> Vec<u8> {
        let mut out = self.report.concat();
        out.extend_from_slice(
            format!(
                "total: {} errors, {} warnings, {}{} lines checked\n",
                self.cnt_error,
                self.cnt_warn,
                if self.check {
                    format!("{} checks, ", self.cnt_chk)
                } else {
                    String::new()
                },
                self.cnt_lines
            )
            .as_bytes(),
        );
        if !clean {
            out.extend_from_slice(
                b"\nNOTE: For some of the reported defects, checkpatch may be able to\n      mechanically convert to the typical style using --fix or --fix-inplace.\n",
            );
        }
        if self.rpt_cleaners {
            self.rpt_cleaners = false;
            out.extend_from_slice(
                b"\nNOTE: Whitespace errors detected.\n      You may wish to use scripts/cleanpatch or scripts/cleanfile\n",
            );
        }
        out.push(b'\n');
        out.extend_from_slice(name);
        if clean {
            out.extend_from_slice(b" has no obvious style problems and is ready for submission.\n");
        } else {
            out.extend_from_slice(b" has style problems, please review.\n");
        }
        out
    }

    // ------------------------------------------------------------ reports

    fn report(&mut self, level: &str, msg: &[u8]) {
        let mut out = format!("{level}: ").into_bytes();
        out.extend_from_slice(msg);
        out.push(b'\n');
        self.report.push(out);
    }

    pub(super) fn error(&mut self, _type: &str, msg: &[u8]) -> bool {
        self.report("ERROR", msg);
        self.clean = false;
        self.cnt_error += 1;
        true
    }

    pub(super) fn warn(&mut self, _type: &str, msg: &[u8]) -> bool {
        self.report("WARNING", msg);
        self.clean = false;
        self.cnt_warn += 1;
        true
    }

    pub(super) fn chk(&mut self, _type: &str, msg: &[u8]) -> bool {
        if !self.check {
            return false;
        }
        self.report("CHECK", msg);
        self.clean = false;
        self.cnt_chk += 1;
        true
    }

    // ------------------------------------------------------------ patterns

    /// The type patterns changed: patterns using them compile again.
    pub(super) const fn types_changed(&mut self) {
        self.generations += 1;
        self.generation = self.generations;
    }

    fn is_dynamic(&self, tpl: &'static str) -> bool {
        let key = (tpl.as_ptr() as usize, tpl.len());
        if let Some(d) = self.dynamic.borrow().get(&key) {
            return *d;
        }
        let d = DYNAMIC.iter().any(|name| {
            let bytes = tpl.as_bytes();
            let mut i = 0;
            while let Some(at) = bytes[i..].iter().position(|&b| b == b'$') {
                let rest = &bytes[i + at + 1..];
                let rest = rest.strip_prefix(b"{").unwrap_or(rest);
                if rest.starts_with(name.as_bytes())
                    && !rest[name.len()..]
                        .first()
                        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    return true;
                }
                i += at + 1;
            }
            false
        });
        self.dynamic.borrow_mut().insert(key, d);
        d
    }

    /// The compiled `qr/tpl/flags` of a pattern written in the source.
    pub(super) fn re(&self, tpl: &'static str, flags: &'static str) -> Rc<Regex> {
        let generation = if self.is_dynamic(tpl) {
            self.generation
        } else {
            0
        };
        let key = (
            tpl.as_ptr() as usize,
            tpl.len(),
            flags.as_ptr() as usize,
            generation,
        );
        if let Some(re) = self.cache.borrow().get(&key) {
            return Rc::clone(re);
        }
        let text = interp(tpl.as_bytes(), &|n| self.v.get(n));
        let re =
            Rc::new(Regex::new(&text, Flags::parse(flags)).unwrap_or_else(|e| {
                panic!("checkpatch pattern /{tpl}/{flags} does not compile: {e}")
            }));
        self.cache.borrow_mut().insert(key, Rc::clone(&re));
        re
    }

    /// A pattern that interpolates values computed from the file, `locals`
    /// before the pattern variables. One that does not compile makes the
    /// run fatal, as Perl dies on it.
    pub(super) fn rel(
        &self,
        tpl: &str,
        flags: &'static str,
        locals: &[(&str, &[u8])],
    ) -> Rc<Regex> {
        let text = interp(tpl.as_bytes(), &|n| {
            locals
                .iter()
                .find(|(name, _)| *name == n)
                .map(|(_, v)| v.to_vec())
                .or_else(|| self.v.get(n))
        });
        let key = (text, flags);
        if let Some(re) = self.text_cache.borrow().get(&key) {
            return Rc::clone(re);
        }
        let re = match Regex::new(&key.0, Flags::parse(flags)) {
            Ok(re) => re,
            Err(e) => {
                self.fatal.borrow_mut().get_or_insert_with(|| {
                    format!("{e} in regex m/{}/", String::from_utf8_lossy(&key.0))
                });
                Regex::new(b"(?!)", Flags::parse("")).expect("never-matching pattern")
            }
        };
        let re = Rc::new(re);
        self.text_cache.borrow_mut().insert(key, Rc::clone(&re));
        re
    }

    /// `$s =~ /tpl/`.
    pub(super) fn m<'a>(&self, tpl: &'static str, s: &'a [u8]) -> Option<Caps<'a>> {
        self.mf(tpl, "", s)
    }

    /// `$s =~ /tpl/flags`.
    pub(super) fn mf<'a>(
        &self,
        tpl: &'static str,
        flags: &'static str,
        s: &'a [u8],
    ) -> Option<Caps<'a>> {
        self.re(tpl, flags).find(s).map(|c| Caps { c, s })
    }

    /// `$s =~ /tpl/` interpolating `locals`.
    pub(super) fn ml<'a>(
        &self,
        tpl: &str,
        flags: &'static str,
        locals: &[(&str, &[u8])],
        s: &'a [u8],
    ) -> Option<Caps<'a>> {
        self.rel(tpl, flags, locals).find(s).map(|c| Caps { c, s })
    }

    pub(super) fn t(&self, tpl: &'static str, s: &[u8]) -> bool {
        self.tf(tpl, "", s)
    }

    pub(super) fn tf(&self, tpl: &'static str, flags: &'static str, s: &[u8]) -> bool {
        self.re(tpl, flags).find(s).is_some()
    }

    pub(super) fn tl(&self, tpl: &str, locals: &[(&str, &[u8])], s: &[u8]) -> bool {
        self.rel(tpl, "", locals).find(s).is_some()
    }

    /// Every match of `$s =~ /tpl/g`.
    pub(super) fn all<'a>(
        &self,
        tpl: &'static str,
        flags: &'static str,
        s: &'a [u8],
    ) -> Vec<Caps<'a>> {
        all(&self.re(tpl, flags), s)
    }

    /// `$s =~ s/tpl/repl/` (`global` for `/g`); the number of substitutions.
    pub(super) fn s(
        &self,
        tpl: &'static str,
        flags: &'static str,
        s: &mut Vec<u8>,
        global: bool,
        repl: &mut dyn FnMut(&Caps) -> Vec<u8>,
    ) -> usize {
        subst(&self.re(tpl, flags), s, global, repl)
    }
}

/// Every match of `/re/g` on `s`.
pub fn all<'a>(re: &Regex, s: &'a [u8]) -> Vec<Caps<'a>> {
    re.find_all(s).into_iter().map(|c| Caps { c, s }).collect()
}

/// `$s =~ s/re/repl/` (`global` for `/g`), as Perl substitutes.
pub fn subst(
    re: &Regex,
    s: &mut Vec<u8>,
    global: bool,
    repl: &mut dyn FnMut(&Caps) -> Vec<u8>,
) -> usize {
    let hay = std::mem::take(s);
    let mut out = Vec::with_capacity(hay.len());
    let mut copied = 0;
    let mut count = 0;
    let mut pos = 0;
    let mut last_empty_at = None;
    while pos <= hay.len() {
        let Some(c) = re.find_g(&hay, pos, last_empty_at) else {
            break;
        };
        let (start, end) = (c.start(), c.end());
        out.extend_from_slice(&hay[copied..start]);
        out.extend(repl(&Caps { c, s: &hay }));
        copied = end;
        count += 1;
        if !global {
            break;
        }
        last_empty_at = (start == end).then_some(end);
        pos = end;
    }
    out.extend_from_slice(&hay[copied..]);
    *s = out;
    count
}

/// `split(/re/, $s)`: captured separators included, trailing empty
/// fields removed.
pub fn split(re: &Regex, s: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut field_start = 0;
    // Perl's split wants each separator to end past the start of its
    // field: an empty match there is refused, as `m//g` refuses one where
    // the previous empty match was.
    while field_start < s.len() {
        let Some(c) = re.find_g(s, field_start, Some(field_start)) else {
            break;
        };
        out.push(s[field_start..c.start()].to_vec());
        for g in 1..c.len() {
            out.push(c.text(s, g).map(<[u8]>::to_vec).unwrap_or_default());
        }
        field_start = c.end();
    }
    out.push(s[field_start..].to_vec());
    while out.last().is_some_and(Vec::is_empty) {
        out.pop();
    }
    out
}

/// `split(' ', $s)`: awk-style fields.
pub fn split_ws(s: &[u8]) -> Vec<Vec<u8>> {
    s.split(|b| perl_space(*b))
        .filter(|f| !f.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// `\s`.
pub const fn perl_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// Perl's `substr($s, $off, $len)` as an rvalue; `None` is undef.
pub fn substr(s: &[u8], off: i64, len: Option<i64>) -> Option<&[u8]> {
    let l = i64::try_from(s.len()).expect("a line fits i64");
    let start = if off < 0 { off + l } else { off };
    if start > l {
        return None;
    }
    let end = match len {
        None => l,
        Some(n) if n < 0 => l + n,
        Some(n) => start + n,
    };
    let start = start.max(0);
    let end = end.min(l);
    if end <= start {
        return Some(&[]);
    }
    Some(&s[usize::try_from(start).expect("start")..usize::try_from(end).expect("end")])
}

/// `substr` interpolated: undef is empty.
pub fn substr_s(s: &[u8], off: i64, len: Option<i64>) -> Vec<u8> {
    substr(s, off, len).map(<[u8]>::to_vec).unwrap_or_default()
}

/// `$array[$i]`, negative from the end.
pub fn at<T>(v: &[T], i: i64) -> Option<&T> {
    let l = i64::try_from(v.len()).expect("length");
    let i = if i < 0 { i + l } else { i };
    if i < 0 {
        return None;
    }
    v.get(usize::try_from(i).expect("index"))
}

/// `tr/c//` on `s`: how many `c`.
#[allow(clippy::naive_bytecount)] // Lines are short; no crate for this.
pub fn count(s: &[u8], c: u8) -> i64 {
    i64::try_from(s.iter().filter(|&&b| b == c).count()).expect("count")
}

/// `length`.
pub fn len(s: &[u8]) -> i64 {
    i64::try_from(s.len()).expect("length")
}

/// Concatenate byte strings.
pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}
