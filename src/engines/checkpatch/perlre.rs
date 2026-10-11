//! Perl's regular expressions, as checkpatch.pl uses them: a backtracking
//! matcher over bytes with Perl's syntax and semantics for a byte string
//! (`\w`, `\s`, `\d` and the POSIX classes are ASCII; `/i` folds ASCII
//! letters only).
//!
//! Supported: literals and escapes, classes (ranges, negation, POSIX
//! classes, `\d\w\s\h\v` and their negations), `.`, the anchors `^ $ \A
//! \z \Z \b \B \G`, capturing and non-capturing groups, named groups,
//! inline flags (`(?i)`, `(?x-s:...)`, `(?^x:...)`), alternation, greedy,
//! lazy and possessive quantifiers, atomic groups, lookahead and
//! lookbehind, backreferences (`\1`, `\g{-1}`, `\k<name>`) and recursion
//! into a group (`(?-1)`, `(?1)`, `(?R)`). Patterns are compiled from the
//! same strings Perl compiles, `qr//` objects interpolated as Perl
//! stringifies them (`(?^x:...)`).

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;

/// Pattern flags: `/i`, `/x`, `/s`, `/m`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Flags {
    pub i: bool,
    pub x: bool,
    pub s: bool,
    pub m: bool,
}

impl Flags {
    /// The flags of a `/.../flags` suffix.
    pub fn parse(s: &str) -> Self {
        let mut f = Self::default();
        for c in s.chars() {
            match c {
                'i' => f.i = true,
                'x' => f.x = true,
                's' => f.s = true,
                'm' => f.m = true,
                _ => {}
            }
        }
        f
    }
}

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A set of bytes.
#[derive(Clone, PartialEq, Eq)]
struct Set([u64; 4]);

impl fmt::Debug for Set {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Set(..)")
    }
}

impl Set {
    const fn empty() -> Self {
        Self([0; 4])
    }

    fn add(&mut self, b: u8) {
        self.0[usize::from(b >> 6)] |= 1 << (b & 63);
    }

    fn add_range(&mut self, lo: u8, hi: u8) {
        for b in lo..=hi {
            self.add(b);
        }
    }

    fn add_set(&mut self, other: &Self) {
        for i in 0..4 {
            self.0[i] |= other.0[i];
        }
    }

    fn negate(&mut self) {
        for w in &mut self.0 {
            *w = !*w;
        }
    }

    const fn has(&self, b: u8) -> bool {
        self.0[(b >> 6) as usize] & (1 << (b & 63)) != 0
    }

    fn of(pred: impl Fn(u8) -> bool) -> Self {
        let mut s = Self::empty();
        for b in 0..=255u8 {
            if pred(b) {
                s.add(b);
            }
        }
        s
    }

    /// Add the other case of every ASCII letter.
    fn fold(&mut self) {
        for b in b'a'..=b'z' {
            let u = b.to_ascii_uppercase();
            if self.has(b) || self.has(u) {
                self.add(b);
                self.add(u);
            }
        }
    }
}

pub const fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Greed {
    Greedy,
    Lazy,
    Possessive,
}

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Set(Set),
    /// A literal run (folded when case-insensitive: both cases accepted).
    Lit(Vec<u8>, bool),
    /// `^`: multi-line or not.
    Bol(bool),
    /// `$`: multi-line or not.
    Eol(bool),
    StartAbs,
    EndAbs,
    EndZ,
    WordB(bool),
    GPos,
    Group(Option<usize>, Box<Self>),
    Concat(Vec<Self>),
    /// Alternatives, with the capturing groups inside each: trying the
    /// next alternative undefines the failed one's groups, as Perl does.
    Alt(Vec<Self>, Vec<Vec<usize>>),
    Repeat(Box<Self>, usize, Option<usize>, Greed),
    /// A lookaround; a lookbehind knows the widths its node can match.
    Look {
        ahead: bool,
        negate: bool,
        node: Box<Self>,
        min: usize,
        max: Option<usize>,
    },
    Atomic(Box<Self>),
    Backref(usize, bool),
    /// A backreference by relative or named reference, resolved after
    /// parsing.
    NamedBackref(String, bool),
    Recurse(usize),
    /// `(?(N)yes|no)`.
    Cond(usize, Box<Self>, Box<Self>),
}

/// A compiled pattern.
#[derive(Clone, Debug)]
pub struct Regex {
    node: Node,
    /// Capturing groups, 1-based (`groups[0]` unused).
    groups: Vec<Node>,
    ngroups: usize,
    /// The pattern can only match at the start of the string (or `\G`).
    anchored_start: bool,
    /// Nothing reads a capture while matching (no backreference, no
    /// conditional), so a loop may remember where it failed.
    memo_ok: bool,
    /// The bytes a match must begin with, when it must consume one.
    first: Option<Set>,
    /// Literals every match contains: a subject without one cannot match.
    required: Vec<Vec<u8>>,
}

/// One activation of a loop: its body, bounds and the failures seen.
struct Loop<'n, 'm> {
    inner: &'n Node,
    min: usize,
    max: Option<usize>,
    memo: Option<&'m RefCell<FastSet>>,
    failures: std::cell::Cell<u32>,
}

/// How many times a loop fails before it remembers where.
const MEMO_AFTER: u32 = 64;

type FastSet = HashSet<(usize, usize), std::hash::BuildHasherDefault<FastHasher>>;

/// A multiply-rotate hasher for the integer keys of the engine's tables:
/// they are never attacker-chosen collisions worth `SipHash`'s cost.
#[derive(Default)]
pub struct FastHasher(u64);

impl std::hash::Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}

/// The bytes a match of `node` can begin with, and whether it can match
/// without consuming one (then it may begin with anything).
fn first_bytes(node: &Node) -> (Set, bool) {
    let any = || {
        let mut s = Set::empty();
        s.negate();
        (s, true)
    };
    match node {
        Node::Empty
        | Node::Bol(_)
        | Node::Eol(_)
        | Node::StartAbs
        | Node::EndAbs
        | Node::EndZ
        | Node::WordB(_)
        | Node::GPos
        | Node::Look { .. } => (Set::empty(), true),
        Node::Set(s) => (s.clone(), false),
        Node::Lit(bytes, ci) => match bytes.first() {
            None => (Set::empty(), true),
            Some(&b) => {
                let mut s = Set::empty();
                s.add(b);
                if *ci {
                    s.fold();
                }
                (s, false)
            }
        },
        Node::Group(_, inner) | Node::Atomic(inner) => first_bytes(inner),
        Node::Concat(items) => {
            let mut set = Set::empty();
            for item in items {
                let (s, nullable) = first_bytes(item);
                set.add_set(&s);
                if !nullable {
                    return (set, false);
                }
            }
            (set, true)
        }
        Node::Alt(items, _) => {
            let mut set = Set::empty();
            let mut nullable = false;
            for item in items {
                let (s, n) = first_bytes(item);
                set.add_set(&s);
                nullable |= n;
            }
            (set, nullable)
        }
        Node::Repeat(inner, min, ..) => {
            let (s, n) = first_bytes(inner);
            (s, n || *min == 0)
        }
        Node::Backref(..) | Node::NamedBackref(..) | Node::Recurse(_) | Node::Cond(..) => any(),
    }
}

/// Literals every match of `node` contains (Perl's required substrings).
fn required(node: &Node) -> Vec<Vec<u8>> {
    match node {
        Node::Lit(bytes, false) if !bytes.is_empty() => vec![bytes.clone()],
        Node::Set(s) => {
            let mut members = (0..=255u8).filter(|&b| s.has(b));
            match (members.next(), members.next()) {
                (Some(b), None) => vec![vec![b]],
                _ => Vec::new(),
            }
        }
        Node::Group(_, inner) | Node::Atomic(inner) => required(inner),
        Node::Repeat(inner, min, ..) if *min >= 1 => required(inner),
        Node::Concat(items) => items.iter().flat_map(required).collect(),
        Node::Alt(items, _) => {
            let mut iter = items.iter().map(required);
            let Some(mut common) = iter.next() else {
                return Vec::new();
            };
            for other in iter {
                common.retain(|lit| other.contains(lit));
            }
            common
        }
        _ => Vec::new(),
    }
}

fn contains(hay: &[u8], lit: &[u8]) -> bool {
    match lit {
        [] => true,
        [b] => hay.contains(b),
        [b, rest @ ..] => hay
            .iter()
            .enumerate()
            .any(|(i, c)| c == b && hay.get(i + 1..i + lit.len()).is_some_and(|w| w == rest)),
    }
}

/// Whether matching `node` reads a capture.
fn reads_captures(node: &Node) -> bool {
    match node {
        Node::Backref(..) | Node::NamedBackref(..) | Node::Cond(..) => true,
        Node::Group(_, inner)
        | Node::Repeat(inner, ..)
        | Node::Atomic(inner)
        | Node::Look { node: inner, .. } => reads_captures(inner),
        Node::Concat(items) | Node::Alt(items, _) => items.iter().any(reads_captures),
        _ => false,
    }
}

/// The positions of a match and its groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Captures {
    pub pos: Vec<Option<(usize, usize)>>,
}

impl Captures {
    pub fn get(&self, i: usize) -> Option<(usize, usize)> {
        self.pos.get(i).copied().flatten()
    }

    pub fn start(&self) -> usize {
        self.pos[0].map_or(0, |p| p.0)
    }

    pub fn end(&self) -> usize {
        self.pos[0].map_or(0, |p| p.1)
    }

    /// `$i`'s text, `None` when the group did not take part.
    pub fn text<'a>(&self, hay: &'a [u8], i: usize) -> Option<&'a [u8]> {
        self.get(i).map(|(s, e)| &hay[s..e])
    }

    /// `$i` as Perl interpolates it: empty when undefined.
    pub fn str(&self, hay: &[u8], i: usize) -> Vec<u8> {
        self.text(hay, i).map(<[u8]>::to_vec).unwrap_or_default()
    }

    pub const fn len(&self) -> usize {
        self.pos.len()
    }
}

// ---------------------------------------------------------------- parser

struct Parser<'a> {
    p: &'a [u8],
    i: usize,
    ngroups: usize,
    names: Vec<(String, usize)>,
    /// Group numbers by nesting, for `(?-1)`: the groups opened so far.
    opened: Vec<usize>,
}

impl Parser<'_> {
    fn err<T>(&self, msg: &str) -> Result<T, Error> {
        Err(Error(format!(
            "{msg} at offset {} in /{}/",
            self.i,
            String::from_utf8_lossy(self.p)
        )))
    }

    fn peek(&self) -> Option<u8> {
        self.p.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<u8> {
        self.p.get(self.i + k).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    /// Skip `/x` whitespace and comments.
    fn skip_x(&mut self, flags: Flags) {
        if !flags.x {
            return;
        }
        loop {
            match self.peek() {
                Some(c) if is_space(c) => self.i += 1,
                Some(b'#') => {
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        if c == b'\n' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn parse_alt(&mut self, flags: &mut Flags) -> Result<Node, Error> {
        let mut alts = vec![self.parse_concat(flags)?];
        while self.eat(b'|') {
            alts.push(self.parse_concat(flags)?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().expect("one")
        } else {
            let groups = alts
                .iter()
                .map(|a| {
                    let mut found = Vec::new();
                    collect_groups(a, &mut found);
                    found
                        .iter()
                        .filter_map(|g| match g {
                            Node::Group(Some(n), _) => Some(*n),
                            _ => None,
                        })
                        .collect()
                })
                .collect();
            Node::Alt(alts, groups)
        })
    }

    fn parse_concat(&mut self, flags: &mut Flags) -> Result<Node, Error> {
        let mut items: Vec<Node> = Vec::new();
        loop {
            self.skip_x(*flags);
            match self.peek() {
                None | Some(b'|' | b')') => break,
                _ => {}
            }
            // An inline flag group `(?i)` changes the flags for the rest
            // of the enclosing group.
            if let Some(new_flags) = self.inline_flags(*flags) {
                *flags = new_flags;
                continue;
            }
            let atom = self.parse_atom(*flags)?;
            self.skip_x(*flags);
            let atom = self.parse_quantifier(atom, *flags);
            items.push(atom);
        }
        // Merge literal runs.
        let mut merged: Vec<Node> = Vec::new();
        for item in items {
            if let (Some(Node::Lit(prev, pci)), Node::Lit(cur, ci)) = (merged.last_mut(), &item)
                && *pci == *ci
            {
                prev.extend_from_slice(cur);
                continue;
            }
            merged.push(item);
        }
        Ok(match merged.len() {
            0 => Node::Empty,
            1 => merged.pop().expect("one"),
            _ => Node::Concat(merged),
        })
    }

    /// `(?flags)` (no colon): the new flags, the group consumed.
    fn inline_flags(&mut self, flags: Flags) -> Option<Flags> {
        if !(self.peek() == Some(b'(') && self.peek_at(1) == Some(b'?')) {
            return None;
        }
        let save = self.i;
        self.i += 2;
        let Some(new) = self.flag_spec(flags) else {
            self.i = save;
            return None;
        };
        if self.eat(b')') {
            return Some(new);
        }
        self.i = save;
        None
    }

    /// `^?[imsx]*(-[imsx]*)?`: the resulting flags, or `None` if not a flag
    /// spec.
    fn flag_spec(&mut self, flags: Flags) -> Option<Flags> {
        let start = self.i;
        let mut f = if self.eat(b'^') {
            Flags::default()
        } else {
            flags
        };
        let mut on = true;
        let mut any = self.i > start;
        while let Some(c) = self.peek() {
            match c {
                b'i' => f.i = on,
                b'x' => f.x = on,
                b's' => f.s = on,
                b'm' => f.m = on,
                b'a' | b'u' | b'l' | b'd' | b'n' | b'p' | b'o' => {}
                b'-' if on => on = false,
                _ => break,
            }
            any = true;
            self.i += 1;
        }
        if !any || !matches!(self.peek(), Some(b':' | b')')) {
            self.i = start;
            return None;
        }
        Some(f)
    }

    fn parse_quantifier(&mut self, atom: Node, flags: Flags) -> Node {
        let (min, max) = match self.peek() {
            Some(b'*') => {
                self.i += 1;
                (0, None)
            }
            Some(b'+') => {
                self.i += 1;
                (1, None)
            }
            Some(b'?') => {
                self.i += 1;
                (0, Some(1))
            }
            Some(b'{') => match self.braces() {
                Some(q) => q,
                None => return atom,
            },
            _ => return atom,
        };
        self.skip_x(flags);
        let greed = if self.eat(b'?') {
            Greed::Lazy
        } else if self.eat(b'+') {
            Greed::Possessive
        } else {
            Greed::Greedy
        };
        if matches!(
            atom,
            Node::Bol(_)
                | Node::Eol(_)
                | Node::StartAbs
                | Node::EndAbs
                | Node::EndZ
                | Node::WordB(_)
                | Node::GPos
        ) {
            // A quantified assertion: Perl matches it at most once.
            return Node::Repeat(Box::new(atom), min.min(1), max.map(|m| m.min(1)), greed);
        }
        Node::Repeat(Box::new(atom), min, max, greed)
    }

    /// `{n}`, `{n,}`, `{n,m}`, `{,m}`; `None` (nothing consumed) when the
    /// brace is a literal.
    fn braces(&mut self) -> Option<(usize, Option<usize>)> {
        let save = self.i;
        self.i += 1;
        let num = |s: &mut Self| -> Option<usize> {
            let st = s.i;
            while s.peek().is_some_and(|c| c.is_ascii_digit()) {
                s.i += 1;
            }
            if s.i == st {
                None
            } else {
                std::str::from_utf8(&s.p[st..s.i]).ok()?.parse().ok()
            }
        };
        let min = num(self);
        let result = if self.eat(b'}') {
            min.map(|n| (n, Some(n)))
        } else if self.eat(b',') {
            let max = num(self);
            if self.eat(b'}') && (min.is_some() || max.is_some()) {
                Some((min.unwrap_or(0), max))
            } else {
                None
            }
        } else {
            None
        };
        if result.is_none() {
            self.i = save;
        }
        result
    }

    fn parse_atom(&mut self, flags: Flags) -> Result<Node, Error> {
        let c = self.peek().expect("an atom");
        self.i += 1;
        match c {
            b'.' => Ok(Node::Set(if flags.s {
                Set::of(|_| true)
            } else {
                Set::of(|b| b != b'\n')
            })),
            b'^' => Ok(Node::Bol(flags.m)),
            b'$' => Ok(Node::Eol(flags.m)),
            b'[' => self.parse_class(flags),
            b'\\' => self.parse_escape(flags),
            b'(' => self.parse_group(flags),
            b'*' | b'+' | b'?' => self.err("quantifier follows nothing"),
            b'{' if self.p.get(self.i).is_some_and(u8::is_ascii_digit) => {
                self.err("quantifier follows nothing")
            }
            _ => Ok(self.lit(c, flags)),
        }
    }

    fn lit(&self, c: u8, flags: Flags) -> Node {
        if flags.i && c.is_ascii_alphabetic() {
            Node::Lit(vec![c.to_ascii_lowercase()], true)
        } else {
            Node::Lit(vec![c], false)
        }
    }

    fn parse_group(&mut self, flags: Flags) -> Result<Node, Error> {
        if self.eat(b'?') {
            match self.peek() {
                Some(b'#') => {
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        if c == b')' {
                            break;
                        }
                    }
                    return Ok(Node::Empty);
                }
                Some(b':') => {
                    self.i += 1;
                    let mut f = flags;
                    let inner = self.parse_alt(&mut f)?;
                    return self.close(Node::Group(None, Box::new(inner)));
                }
                Some(b'=' | b'!') => {
                    let negate = self.peek() == Some(b'!');
                    self.i += 1;
                    let mut f = flags;
                    let inner = self.parse_alt(&mut f)?;
                    return self.close(Node::Look {
                        ahead: true,
                        negate,
                        node: Box::new(inner),
                        min: 0,
                        max: None,
                    });
                }
                Some(b'<') if matches!(self.peek_at(1), Some(b'=' | b'!')) => {
                    let negate = self.peek_at(1) == Some(b'!');
                    self.i += 2;
                    let mut f = flags;
                    let inner = self.parse_alt(&mut f)?;
                    let (min, max) = width(&inner);
                    return self.close(Node::Look {
                        ahead: false,
                        negate,
                        node: Box::new(inner),
                        min,
                        max,
                    });
                }
                Some(b'>') => {
                    self.i += 1;
                    let mut f = flags;
                    let inner = self.parse_alt(&mut f)?;
                    return self.close(Node::Atomic(Box::new(inner)));
                }
                Some(b'<' | b'\'' | b'P') => {
                    if self.peek() == Some(b'P') {
                        self.i += 1;
                        if self.peek() != Some(b'<') {
                            return self.err("unsupported (?P");
                        }
                    }
                    let close = if self.peek() == Some(b'\'') {
                        b'\''
                    } else {
                        b'>'
                    };
                    self.i += 1;
                    let st = self.i;
                    while self.peek().is_some_and(|c| c != close) {
                        self.i += 1;
                    }
                    let name = String::from_utf8_lossy(&self.p[st..self.i]).into_owned();
                    self.i += 1;
                    self.ngroups += 1;
                    let n = self.ngroups;
                    self.names.push((name, n));
                    self.opened.push(n);
                    let mut f = flags;
                    let inner = self.parse_alt(&mut f)?;
                    return self.close(Node::Group(Some(n), Box::new(inner)));
                }
                Some(b'(') => {
                    // `(?(N)yes|no)`.
                    self.i += 1;
                    let n = self.digits();
                    if !self.eat(b')') {
                        return self.err("unsupported condition");
                    }
                    let mut f = flags;
                    let yes = self.parse_concat(&mut f)?;
                    let no = if self.eat(b'|') {
                        self.parse_concat(&mut f)?
                    } else {
                        Node::Empty
                    };
                    return self.close(Node::Cond(n, Box::new(yes), Box::new(no)));
                }
                Some(b'R' | b'0') => {
                    self.i += 1;
                    if !self.eat(b')') {
                        return self.err("bad recursion");
                    }
                    return Ok(Node::Recurse(0));
                }
                Some(b'-' | b'+') if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) => {
                    let sign = self.peek() == Some(b'-');
                    self.i += 1;
                    let n = self.digits();
                    if !self.eat(b')') {
                        return self.err("bad recursion");
                    }
                    let target = if sign {
                        // The n-th group opened before this point.
                        self.ngroups + 1 - n
                    } else {
                        self.ngroups + n
                    };
                    return Ok(Node::Recurse(target));
                }
                Some(d) if d.is_ascii_digit() => {
                    let n = self.digits();
                    if !self.eat(b')') {
                        return self.err("bad recursion");
                    }
                    return Ok(Node::Recurse(n));
                }
                _ => {
                    let Some(new_flags) = self.flag_spec(flags) else {
                        return self.err("unknown group");
                    };
                    if self.eat(b':') {
                        let mut f = new_flags;
                        let inner = self.parse_alt(&mut f)?;
                        return self.close(Node::Group(None, Box::new(inner)));
                    }
                    return self.err("unexpected flag group");
                }
            }
        }
        self.ngroups += 1;
        let n = self.ngroups;
        self.opened.push(n);
        let mut f = flags;
        let inner = self.parse_alt(&mut f)?;
        self.close(Node::Group(Some(n), Box::new(inner)))
    }

    fn close(&mut self, node: Node) -> Result<Node, Error> {
        if !self.eat(b')') {
            return self.err("missing )");
        }
        Ok(node)
    }

    fn digits(&mut self) -> usize {
        let mut n = 0;
        while let Some(c) = self.peek().filter(u8::is_ascii_digit) {
            n = n * 10 + usize::from(c - b'0');
            self.i += 1;
        }
        n
    }

    /// The set of an escape usable in and out of classes, or `None`.
    fn class_escape(c: u8) -> Option<Set> {
        Some(match c {
            b'd' => Set::of(|b| b.is_ascii_digit()),
            b'D' => Set::of(|b| !b.is_ascii_digit()),
            b'w' => Set::of(is_word),
            b'W' => Set::of(|b| !is_word(b)),
            b's' => Set::of(is_space),
            b'S' => Set::of(|b| !is_space(b)),
            b'h' => Set::of(|b| matches!(b, b' ' | b'\t' | 0xA0)),
            b'H' => Set::of(|b| !matches!(b, b' ' | b'\t' | 0xA0)),
            b'v' => Set::of(|b| matches!(b, b'\n' | 0x0B | 0x0C | b'\r' | 0x85)),
            b'V' => Set::of(|b| !matches!(b, b'\n' | 0x0B | 0x0C | b'\r' | 0x85)),
            b'N' => Set::of(|b| b != b'\n'),
            _ => return None,
        })
    }

    /// A single-byte escape (`\n`, `\x41`, `\\`, ...).
    fn byte_escape(&mut self, c: u8) -> u8 {
        match c {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'f' => 0x0C,
            b'e' => 0x1B,
            b'a' => 0x07,
            b'0' => {
                let mut v = 0u32;
                let mut k = 0;
                while k < 2 && self.peek().is_some_and(|d| (b'0'..=b'7').contains(&d)) {
                    v = v * 8 + u32::from(self.peek().expect("digit") - b'0');
                    self.i += 1;
                    k += 1;
                }
                v as u8
            }
            b'x' => {
                let braced = self.eat(b'{');
                let st = self.i;
                if braced {
                    while self.peek().is_some_and(|d| d != b'}') {
                        self.i += 1;
                    }
                    let hex = String::from_utf8_lossy(&self.p[st..self.i])
                        .trim()
                        .to_string();
                    self.i += 1;
                    u32::from_str_radix(&hex, 16).unwrap_or(0) as u8
                } else {
                    while self.i - st < 2 && self.peek().is_some_and(|d| d.is_ascii_hexdigit()) {
                        self.i += 1;
                    }
                    u8::from_str_radix(std::str::from_utf8(&self.p[st..self.i]).unwrap_or("0"), 16)
                        .unwrap_or(0)
                }
            }
            b'c' => {
                let x = self.peek().unwrap_or(b'@');
                self.i += 1;
                x.to_ascii_uppercase() ^ 0x40
            }
            other => other,
        }
    }

    fn parse_escape(&mut self, flags: Flags) -> Result<Node, Error> {
        let Some(c) = self.peek() else {
            return self.err("trailing \\");
        };
        self.i += 1;
        if let Some(set) = Self::class_escape(c) {
            return Ok(Node::Set(set));
        }
        match c {
            b'A' => Ok(Node::StartAbs),
            b'z' => Ok(Node::EndAbs),
            b'Z' => Ok(Node::EndZ),
            b'b' => Ok(Node::WordB(true)),
            b'B' => Ok(Node::WordB(false)),
            b'G' => Ok(Node::GPos),
            b'1'..=b'9' => {
                // \1-\9 are backreferences; \10 and up only when that many
                // groups precede, else an octal escape.
                self.i -= 1;
                let st = self.i;
                let n = self.digits();
                if n < 10 || n <= self.ngroups {
                    return Ok(Node::Backref(n, flags.i));
                }
                self.i = st;
                let mut v = 0u32;
                while self.i - st < 3 && self.peek().is_some_and(|d| (b'0'..=b'7').contains(&d)) {
                    v = v * 8 + u32::from(self.peek().expect("digit") - b'0');
                    self.i += 1;
                }
                Ok(self.lit(v as u8, flags))
            }
            b'g' => {
                let braced = self.eat(b'{');
                let neg = self.eat(b'-');
                let n = self.digits();
                if braced {
                    self.eat(b'}');
                }
                let n = if neg { self.ngroups + 1 - n } else { n };
                Ok(Node::Backref(n, flags.i))
            }
            b'k' => {
                let close = match self.peek() {
                    Some(b'<') => b'>',
                    Some(b'{') => b'}',
                    _ => b'\'',
                };
                self.i += 1;
                let st = self.i;
                while self.peek().is_some_and(|d| d != close) {
                    self.i += 1;
                }
                let name = String::from_utf8_lossy(&self.p[st..self.i]).into_owned();
                self.i += 1;
                Ok(Node::NamedBackref(name, flags.i))
            }
            _ => {
                let b = self.byte_escape(c);
                Ok(self.lit(b, flags))
            }
        }
    }

    fn parse_class(&mut self, flags: Flags) -> Result<Node, Error> {
        let mut set = Set::empty();
        let negate = self.eat(b'^');
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return self.err("unterminated class");
            };
            if c == b']' && !first {
                self.i += 1;
                break;
            }
            first = false;
            // POSIX class.
            if c == b'['
                && self.peek_at(1) == Some(b':')
                && let Some(end) = self.p[self.i + 2..].windows(2).position(|w| w == b":]")
            {
                let mut name = &self.p[self.i + 2..self.i + 2 + end];
                let neg = name.first() == Some(&b'^');
                if neg {
                    name = &name[1..];
                }
                let mut s = posix_class(name).ok_or_else(|| {
                    Error(format!(
                        "unknown POSIX class {}",
                        String::from_utf8_lossy(name)
                    ))
                })?;
                if neg {
                    s.negate();
                }
                set.add_set(&s);
                self.i += 2 + end + 2;
                continue;
            }
            let lo = self.class_atom(&mut set)?;
            let Some(lo) = lo else {
                continue;
            };
            if self.peek() == Some(b'-') && self.peek_at(1).is_some_and(|d| d != b']') {
                let save = self.i;
                self.i += 1;
                let mut dummy = Set::empty();
                if let Some(hi) = self.class_atom(&mut dummy)? {
                    if hi < lo {
                        return self.err("invalid range");
                    }
                    set.add_range(lo, hi);
                } else {
                    // `[a-\d]`: the dash is literal.
                    self.i = save;
                    set.add(lo);
                }
                continue;
            }
            set.add(lo);
        }
        if flags.i {
            set.fold();
        }
        if negate {
            set.negate();
        }
        Ok(Node::Set(set))
    }

    /// One class member: a byte (returned, for ranges) or a set (added).
    fn class_atom(&mut self, set: &mut Set) -> Result<Option<u8>, Error> {
        let c = self.peek().expect("a class atom");
        self.i += 1;
        if c != b'\\' {
            return Ok(Some(c));
        }
        let Some(e) = self.peek() else {
            return self.err("trailing \\");
        };
        self.i += 1;
        if let Some(s) = Self::class_escape(e) {
            set.add_set(&s);
            return Ok(None);
        }
        if e == b'b' {
            return Ok(Some(0x08));
        }
        Ok(Some(self.byte_escape(e)))
    }
}

fn posix_class(name: &[u8]) -> Option<Set> {
    Some(match name {
        b"alpha" => Set::of(|b| b.is_ascii_alphabetic()),
        b"digit" => Set::of(|b| b.is_ascii_digit()),
        b"alnum" => Set::of(|b| b.is_ascii_alphanumeric()),
        b"upper" => Set::of(|b| b.is_ascii_uppercase()),
        b"lower" => Set::of(|b| b.is_ascii_lowercase()),
        b"space" => Set::of(is_space),
        b"blank" => Set::of(|b| b == b' ' || b == b'\t'),
        b"punct" => Set::of(|b| b.is_ascii_punctuation()),
        b"print" => Set::of(|b| (0x20..=0x7E).contains(&b)),
        b"graph" => Set::of(|b| (0x21..=0x7E).contains(&b)),
        b"cntrl" => Set::of(|b| b < 0x20 || b == 0x7F),
        b"xdigit" => Set::of(|b| b.is_ascii_hexdigit()),
        b"word" => Set::of(is_word),
        b"ascii" => Set::of(|b| b < 0x80),
        _ => return None,
    })
}

/// The shortest and longest text a node can match (`None`: unbounded).
fn width(node: &Node) -> (usize, Option<usize>) {
    match node {
        Node::Set(_) => (1, Some(1)),
        Node::Lit(b, _) => (b.len(), Some(b.len())),
        Node::Group(_, inner) | Node::Atomic(inner) => width(inner),
        Node::Concat(items) => items.iter().fold((0, Some(0)), |(lo, hi), item| {
            let (a, b) = width(item);
            (lo + a, hi.zip(b).map(|(x, y)| x + y))
        }),
        Node::Alt(items, _) => {
            let ws: Vec<_> = items.iter().map(width).collect();
            let lo = ws.iter().map(|w| w.0).min().unwrap_or(0);
            let hi = ws.iter().try_fold(0, |acc, w| w.1.map(|x| acc.max(x)));
            (lo, hi)
        }
        Node::Repeat(inner, min, max, _) => {
            let (a, b) = width(inner);
            (a * min, b.zip(*max).map(|(x, m)| x * m))
        }
        Node::Backref(..) | Node::NamedBackref(..) | Node::Recurse(_) => (0, None),
        _ => (0, Some(0)),
    }
}

/// Collect the capturing groups' nodes by number.
fn collect_groups(node: &Node, out: &mut Vec<Node>) {
    match node {
        Node::Group(n, inner) => {
            if let Some(n) = n {
                if out.len() <= *n {
                    out.resize(*n + 1, Node::Empty);
                }
                out[*n] = node.clone();
            }
            collect_groups(inner, out);
        }
        Node::Concat(items) | Node::Alt(items, _) => {
            for item in items {
                collect_groups(item, out);
            }
        }
        Node::Repeat(inner, ..) | Node::Atomic(inner) | Node::Look { node: inner, .. } => {
            collect_groups(inner, out);
        }
        Node::Cond(_, yes, no) => {
            collect_groups(yes, out);
            collect_groups(no, out);
        }
        _ => {}
    }
}

fn resolve_names(node: &mut Node, names: &[(String, usize)]) -> Result<(), Error> {
    match node {
        Node::NamedBackref(name, ci) => {
            let n = names
                .iter()
                .find(|(nm, _)| nm == name)
                .map(|(_, n)| *n)
                .ok_or_else(|| Error(format!("reference to nonexistent named group {name}")))?;
            *node = Node::Backref(n, *ci);
        }
        Node::Group(_, inner)
        | Node::Repeat(inner, ..)
        | Node::Atomic(inner)
        | Node::Look { node: inner, .. } => {
            resolve_names(inner, names)?;
        }
        Node::Cond(_, yes, no) => {
            resolve_names(yes, names)?;
            resolve_names(no, names)?;
        }
        Node::Concat(items) | Node::Alt(items, _) => {
            for item in items {
                resolve_names(item, names)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn starts_anchored(node: &Node) -> bool {
    match node {
        Node::StartAbs | Node::Bol(false) => true,
        Node::Concat(items) => items.first().is_some_and(starts_anchored),
        Node::Group(_, inner) | Node::Atomic(inner) => starts_anchored(inner),
        Node::Alt(items, _) => items.iter().all(starts_anchored),
        _ => false,
    }
}

impl Regex {
    /// Compile `pattern` with the flags of its `/.../flags`.
    pub fn new(pattern: &[u8], flags: Flags) -> Result<Self, Error> {
        let mut parser = Parser {
            p: pattern,
            i: 0,
            ngroups: 0,
            names: Vec::new(),
            opened: Vec::new(),
        };
        let mut f = flags;
        let mut node = parser.parse_alt(&mut f)?;
        if parser.i < pattern.len() {
            return parser.err("unmatched )");
        }
        resolve_names(&mut node, &parser.names)?;
        let mut groups = vec![Node::Empty];
        collect_groups(&node, &mut groups);
        groups.resize(parser.ngroups + 1, Node::Empty);
        groups[0] = Node::Group(Some(0), Box::new(node.clone()));
        let anchored_start = starts_anchored(&node);
        let memo_ok = !reads_captures(&node);
        let (first, nullable) = first_bytes(&node);
        let first = (!nullable).then_some(first);
        let mut required = required(&node);
        required.sort();
        required.dedup();
        Ok(Self {
            first,
            required,
            node,
            groups,
            ngroups: parser.ngroups,
            anchored_start,
            memo_ok,
        })
    }

    /// The leftmost match starting at or after `start` (`\G` at `gpos`).
    pub fn find_at(&self, hay: &[u8], start: usize, gpos: usize) -> Option<Captures> {
        // Lookbehinds may read before `start`: the whole subject is searched.
        // An anchored pattern tries one position: searching would cost more.
        if !self.anchored_start && !self.required.iter().all(|lit| contains(hay, lit)) {
            return None;
        }
        let mut pos = start;
        let mut st = State {
            hay,
            caps: vec![usize::MAX; 2 * (self.ngroups + 1)],
            gpos,
        };
        loop {
            if let Some(first) = &self.first {
                // Perl's start class: a match begins with one of these.
                match hay
                    .get(pos..)
                    .and_then(|rest| rest.iter().position(|&b| first.has(b)))
                {
                    Some(skip) if !self.anchored_start || skip == 0 => pos += skip,
                    _ => return None,
                }
            }
            if pos > hay.len() {
                return None;
            }
            st.caps.fill(usize::MAX);
            let mut end = None;
            let found = self.m(&self.node, pos, &mut st, &mut |p, _| {
                end = Some(p);
                true
            });
            if found {
                st.caps[0] = pos;
                st.caps[1] = end.expect("an end");
                return Some(st.captures());
            }
            if self.anchored_start {
                return None;
            }
            pos += 1;
        }
    }

    /// `$hay =~ /re/`.
    pub fn find(&self, hay: &[u8]) -> Option<Captures> {
        self.find_at(hay, 0, 0)
    }

    /// Every match of `/re/g`, as Perl iterates them (a zero-length match
    /// is not repeated at the same position).
    pub fn find_all(&self, hay: &[u8]) -> Vec<Captures> {
        let mut out = Vec::new();
        let mut pos = 0;
        let mut last_empty_at = None;
        while pos <= hay.len() {
            let Some(c) = self.find_g(hay, pos, last_empty_at) else {
                break;
            };
            let (s, e) = (c.start(), c.end());
            last_empty_at = if s == e { Some(e) } else { None };
            pos = e;
            out.push(c);
        }
        out
    }

    /// One step of `m//g` from `pos`: a zero-length match at
    /// `last_empty_at` is not allowed again.
    pub fn find_g(&self, hay: &[u8], pos: usize, last_empty_at: Option<usize>) -> Option<Captures> {
        let mut start = pos;
        loop {
            let c = self.find_at(hay, start, pos)?;
            if Some(c.start()) == last_empty_at && c.start() == c.end() {
                // Retry, refusing the empty match here: Perl tries the
                // next alternative; approximating by trying a non-empty
                // match at the same start first.
                if let Some(c2) = self.find_nonempty_at(hay, c.start(), pos) {
                    return Some(c2);
                }
                start = c.start() + 1;
                if start > hay.len() {
                    return None;
                }
                continue;
            }
            return Some(c);
        }
    }

    fn find_nonempty_at(&self, hay: &[u8], at: usize, gpos: usize) -> Option<Captures> {
        let mut st = State {
            hay,
            caps: vec![usize::MAX; 2 * (self.ngroups + 1)],
            gpos,
        };
        let mut end = None;
        let found = self.m(&self.node, at, &mut st, &mut |p, _| {
            if p == at {
                return false;
            }
            end = Some(p);
            true
        });
        if !found {
            return None;
        }
        st.caps[0] = at;
        st.caps[1] = end.expect("an end");
        Some(st.captures())
    }
}

// --------------------------------------------------------------- matcher

struct State<'h> {
    hay: &'h [u8],
    caps: Vec<usize>,
    gpos: usize,
}

impl State<'_> {
    fn captures(&self) -> Captures {
        let n = self.caps.len() / 2;
        Captures {
            pos: (0..n)
                .map(|i| {
                    let (s, e) = (self.caps[2 * i], self.caps[2 * i + 1]);
                    (s != usize::MAX && e != usize::MAX).then_some((s, e))
                })
                .collect(),
        }
    }
}

type Cont<'c, 'h> = dyn FnMut(usize, &mut State<'h>) -> bool + 'c;

impl Regex {
    /// Match `node` at `pos`, calling `k` with each end position in Perl's
    /// order until it accepts one.
    fn m<'h>(&self, node: &Node, pos: usize, st: &mut State<'h>, k: &mut Cont<'_, 'h>) -> bool {
        let hay = st.hay;
        match node {
            Node::Empty => k(pos, st),
            Node::Set(set) => pos < hay.len() && set.has(hay[pos]) && k(pos + 1, st),
            Node::Lit(bytes, ci) => {
                let end = pos + bytes.len();
                if end > hay.len() {
                    return false;
                }
                let ok = if *ci {
                    hay[pos..end]
                        .iter()
                        .zip(bytes)
                        .all(|(a, b)| a.to_ascii_lowercase() == *b)
                } else {
                    hay[pos..end].iter().zip(bytes).all(|(a, b)| a == b)
                };
                ok && k(end, st)
            }
            Node::Bol(multi) => {
                let ok = pos == 0 || (*multi && hay[pos - 1] == b'\n' && pos < hay.len());
                ok && k(pos, st)
            }
            Node::Eol(multi) => {
                let ok = pos == hay.len()
                    || (pos + 1 == hay.len() && hay[pos] == b'\n')
                    || (*multi && hay[pos] == b'\n');
                ok && k(pos, st)
            }
            Node::StartAbs => pos == 0 && k(pos, st),
            Node::EndAbs => pos == hay.len() && k(pos, st),
            Node::EndZ => {
                (pos == hay.len() || (pos + 1 == hay.len() && hay[pos] == b'\n')) && k(pos, st)
            }
            Node::WordB(want) => {
                let before = pos > 0 && is_word(hay[pos - 1]);
                let after = pos < hay.len() && is_word(hay[pos]);
                ((before != after) == *want) && k(pos, st)
            }
            Node::GPos => pos == st.gpos && k(pos, st),
            Node::Group(n, inner) => match n {
                None => self.m(inner, pos, st, k),
                Some(n) => {
                    let n = *n;
                    let (old_s, old_e) = (st.caps[2 * n], st.caps[2 * n + 1]);
                    let ok = self.m(inner, pos, st, &mut |p, st| {
                        let (ps, pe) = (st.caps[2 * n], st.caps[2 * n + 1]);
                        st.caps[2 * n] = pos;
                        st.caps[2 * n + 1] = p;
                        if k(p, st) {
                            return true;
                        }
                        st.caps[2 * n] = ps;
                        st.caps[2 * n + 1] = pe;
                        false
                    });
                    if !ok {
                        st.caps[2 * n] = old_s;
                        st.caps[2 * n + 1] = old_e;
                    }
                    ok
                }
            },
            Node::Concat(items) => self.concat(items, 0, pos, st, k),
            Node::Alt(alts, groups) if groups.iter().all(Vec::is_empty) => {
                // No group inside: a failed alternative leaves no capture.
                alts.iter().any(|alt| self.m(alt, pos, st, k))
            }
            Node::Alt(alts, groups) => {
                let before = st.caps.clone();
                for (alt, inside) in alts.iter().zip(groups) {
                    if self.m(alt, pos, st, k) {
                        return true;
                    }
                    for &g in inside {
                        st.caps[2 * g] = usize::MAX;
                        st.caps[2 * g + 1] = usize::MAX;
                    }
                }
                st.caps = before;
                false
            }
            Node::Repeat(inner, min, max, greed) => {
                if let Some(set) = single_byte(inner) {
                    return self.repeat_simple(&set, pos, *min, *max, *greed, st, k);
                }
                let memo = RefCell::new(FastSet::default());
                let lp = Loop {
                    inner,
                    min: *min,
                    max: *max,
                    memo: self.memo_ok.then_some(&memo),
                    failures: std::cell::Cell::new(0),
                };
                match greed {
                    Greed::Possessive => {
                        let mut end = None;
                        let mut saved = Vec::new();
                        if self.repeat(&lp, 0, pos, Greed::Greedy, st, &mut |p, st| {
                            end = Some(p);
                            saved.clone_from(&st.caps);
                            true
                        }) {
                            let caps = std::mem::take(&mut saved);
                            let before = std::mem::replace(&mut st.caps, caps);
                            if k(end.expect("an end"), st) {
                                return true;
                            }
                            st.caps = before;
                        }
                        false
                    }
                    _ => self.repeat(&lp, 0, pos, *greed, st, k),
                }
            }
            Node::Atomic(inner) => {
                let before = st.caps.clone();
                let mut end = None;
                let mut saved = Vec::new();
                if self.m(inner, pos, st, &mut |p, st| {
                    end = Some(p);
                    saved.clone_from(&st.caps);
                    true
                }) {
                    st.caps = saved;
                    if k(end.expect("an end"), st) {
                        return true;
                    }
                }
                st.caps = before;
                false
            }
            Node::Look {
                ahead,
                negate,
                node,
                min,
                max,
            } => {
                let before = st.caps.clone();
                let mut saved = Vec::new();
                let found = if *ahead {
                    self.m(node, pos, st, &mut |_, st| {
                        saved.clone_from(&st.caps);
                        true
                    })
                } else {
                    let lowest = max.map_or(0, |m| pos.saturating_sub(m));
                    let highest = pos.checked_sub(*min);
                    highest.is_some_and(|hi| {
                        (lowest..=hi).rev().any(|from| {
                            self.m(node, from, st, &mut |p, st| {
                                if p != pos {
                                    return false;
                                }
                                saved.clone_from(&st.caps);
                                true
                            })
                        })
                    })
                };
                if found == *negate {
                    st.caps = before;
                    return false;
                }
                if found {
                    st.caps = saved;
                } else {
                    st.caps.clone_from(&before);
                }
                if k(pos, st) {
                    return true;
                }
                st.caps = before;
                false
            }
            Node::Backref(n, ci) => {
                let (s, e) = (st.caps.get(2 * n).copied(), st.caps.get(2 * n + 1).copied());
                let (Some(s), Some(e)) = (s, e) else {
                    return false;
                };
                if s == usize::MAX || e == usize::MAX {
                    return false;
                }
                let len = e - s;
                if pos + len > hay.len() {
                    return false;
                }
                let ok = if *ci {
                    hay[s..e].eq_ignore_ascii_case(&hay[pos..pos + len])
                } else {
                    hay[s..e] == hay[pos..pos + len]
                };
                ok && k(pos + len, st)
            }
            Node::NamedBackref(..) => false,
            Node::Cond(n, yes, no) => {
                let set = st.caps.get(2 * n + 1).is_some_and(|&e| e != usize::MAX);
                self.m(if set { yes } else { no }, pos, st, k)
            }
            Node::Recurse(n) => {
                let group = &self.groups[*n];
                let Node::Group(_, inner) = group else {
                    return false;
                };
                // Captures set inside a recursion are undone when it returns.
                let before = st.caps.clone();
                let ok = self.m(inner, pos, st, &mut |p, st| {
                    let inside = std::mem::replace(&mut st.caps, before.clone());
                    if k(p, st) {
                        return true;
                    }
                    st.caps = inside;
                    false
                });
                if !ok {
                    st.caps = before;
                }
                ok
            }
        }
    }

    fn concat<'h>(
        &self,
        items: &[Node],
        i: usize,
        pos: usize,
        st: &mut State<'h>,
        k: &mut Cont<'_, 'h>,
    ) -> bool {
        if i == items.len() {
            return k(pos, st);
        }
        self.m(&items[i], pos, st, &mut |p, st| {
            self.concat(items, i + 1, p, st, k)
        })
    }

    /// Iteration `count` of a loop, at `pos`.
    fn repeat<'h>(
        &self,
        lp: &Loop,
        count: usize,
        pos: usize,
        greed: Greed,
        st: &mut State<'h>,
        k: &mut Cont<'_, 'h>,
    ) -> bool {
        let (min, max) = (lp.min, lp.max);
        // Perl's super-linear cache: once the rest of the match has failed
        // from an iteration at a position, it fails there again. Past the
        // minimum of an unbounded loop the count no longer matters.
        let key = (
            if count >= min && max.is_none() {
                min
            } else {
                count
            },
            pos,
        );
        if lp
            .memo
            .is_some_and(|m| !m.borrow().is_empty() && m.borrow().contains(&key))
        {
            return false;
        }
        let can_more = max.is_none_or(|m| count < m);
        if greed == Greed::Lazy && count >= min && k(pos, st) {
            return true;
        }
        if can_more
            && self.m(lp.inner, pos, st, &mut |p, st| {
                if p == pos && count >= min {
                    // An empty iteration ends the loop.
                    return greed != Greed::Lazy && k(p, st);
                }
                self.repeat(lp, count + 1, p, greed, st, k)
            })
        {
            return true;
        }
        if greed != Greed::Lazy && count >= min && k(pos, st) {
            return true;
        }
        // Like Perl, start remembering only once the loop has failed often
        // enough for backtracking to be the cost.
        let failures = lp.failures.get() + 1;
        lp.failures.set(failures);
        if failures > MEMO_AFTER
            && let Some(m) = lp.memo
        {
            m.borrow_mut().insert(key);
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn repeat_simple<'h>(
        &self,
        set: &Set,
        pos: usize,
        min: usize,
        max: Option<usize>,
        greed: Greed,
        st: &mut State<'h>,
        k: &mut Cont<'_, 'h>,
    ) -> bool {
        let hay = st.hay;
        let limit = max.map_or(hay.len() - pos, |m| m.min(hay.len() - pos));
        let mut n = 0;
        while n < limit && set.has(hay[pos + n]) {
            n += 1;
        }
        if n < min {
            return false;
        }
        match greed {
            Greed::Greedy => (min..=n).rev().any(|c| k(pos + c, st)),
            Greed::Lazy => (min..=n).any(|c| k(pos + c, st)),
            Greed::Possessive => k(pos + n, st),
        }
    }
}

/// The byte set a node matches when it always matches exactly one byte.
fn single_byte(node: &Node) -> Option<Set> {
    match node {
        Node::Set(s) => Some(s.clone()),
        Node::Lit(b, ci) if b.len() == 1 => {
            let mut s = Set::empty();
            s.add(b[0]);
            if *ci {
                s.fold();
            }
            Some(s)
        }
        Node::Group(None, inner) => single_byte(inner),
        _ => None,
    }
}

/// Perl's `quotemeta`: every byte but `[A-Za-z0-9_]` (and bytes from 0x80)
/// escaped.
pub fn quotemeta(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for &b in s {
        if !(is_word(b) || b >= 0x80) {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}
