//! Lua 5.1 patterns, as `lstrlib.c` matches them.
//!
//! luacheck runs on Lua 5.1 and uses patterns everywhere a user can write
//! one (`ignore`, `enable` and `only` filters, globs turned into patterns),
//! so the matcher is a port of Lua 5.1.5's `match` machinery: the same
//! backtracking, the same character classes (C locale), and the same errors,
//! raised as lazily as Lua raises them (only when matching reaches the
//! malformed part).

use std::fmt;

const L_ESC: u8 = b'%';
const SPECIALS: &[u8] = b"^$*+?.([%-";
const MAX_CAPTURES: usize = 32;
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;

/// A malformed pattern, with Lua's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError(pub String);

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PatternError {}

/// A capture: a substring (byte range of the subject) or a position
/// capture `()` (1-based, as Lua returns it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture {
    Str(usize, usize),
    Pos(usize),
}

impl Capture {
    /// The captured bytes; a position capture has none.
    pub fn bytes<'a>(&self, subject: &'a [u8]) -> &'a [u8] {
        match self {
            Self::Str(start, end) => &subject[*start..*end],
            Self::Pos(_) => &[],
        }
    }

    pub const fn position(&self) -> Option<usize> {
        match self {
            Self::Pos(p) => Some(*p),
            Self::Str(..) => None,
        }
    }
}

/// A successful match: the 1-based inclusive byte range `start..=end` as
/// `string.find` returns it, and the captures.
#[derive(Debug, Clone)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    pub captures: Vec<Capture>,
}

struct MatchState<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    capture: [(usize, isize); MAX_CAPTURES],
}

fn err(msg: &str) -> PatternError {
    PatternError(msg.to_string())
}

const fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

const fn is_cntrl(c: u8) -> bool {
    c < 32 || c == 127
}

const fn is_punct(c: u8) -> bool {
    c.is_ascii_punctuation()
}

const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

const fn match_class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => is_alpha(c),
        b'c' => is_cntrl(c),
        b'd' => c.is_ascii_digit(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => is_punct(c),
        b's' => is_space(c),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        b'z' => c == 0,
        _ => return cl == c,
    };
    if cl.is_ascii_lowercase() { res } else { !res }
}

impl MatchState<'_> {
    /// The pattern byte at `p`, or 0 past the end (Lua patterns are
    /// NUL-terminated C strings).
    fn p(&self, p: usize) -> u8 {
        self.pat.get(p).copied().unwrap_or(0)
    }

    /// The subject byte at `s`, or 0 past the end (Lua strings carry a
    /// terminating NUL that the matcher can read).
    fn s(&self, s: usize) -> u8 {
        self.src.get(s).copied().unwrap_or(0)
    }

    fn check_capture(&self, l: u8) -> Result<usize, PatternError> {
        let l = isize::from(l) - isize::from(b'1');
        if l < 0 {
            return Err(err("invalid capture index"));
        }
        let l = l as usize;
        if l >= self.level || self.capture[l].1 == CAP_UNFINISHED {
            return Err(err("invalid capture index"));
        }
        Ok(l)
    }

    fn capture_to_close(&self) -> Result<usize, PatternError> {
        (0..self.level)
            .rev()
            .find(|&level| self.capture[level].1 == CAP_UNFINISHED)
            .ok_or_else(|| err("invalid pattern capture"))
    }

    fn class_end(&self, mut p: usize) -> Result<usize, PatternError> {
        let c = self.p(p);
        p += 1;
        match c {
            L_ESC => {
                if self.p(p) == 0 {
                    return Err(err("malformed pattern (ends with '%')"));
                }
                Ok(p + 1)
            }
            b'[' => {
                if self.p(p) == b'^' {
                    p += 1;
                }
                loop {
                    if self.p(p) == 0 {
                        return Err(err("malformed pattern (missing ']')"));
                    }
                    let c = self.p(p);
                    p += 1;
                    if c == L_ESC && self.p(p) != 0 {
                        p += 1;
                    }
                    if self.p(p) == b']' {
                        break;
                    }
                }
                Ok(p + 1)
            }
            _ => Ok(p),
        }
    }

    /// `p` points at `[`, `ec` at the closing `]`.
    fn match_bracket_class(&self, c: u8, mut p: usize, ec: usize) -> bool {
        let mut sig = true;
        if self.p(p + 1) == b'^' {
            sig = false;
            p += 1;
        }
        loop {
            p += 1;
            if p >= ec {
                break;
            }
            if self.p(p) == L_ESC {
                p += 1;
                if match_class(c, self.p(p)) {
                    return sig;
                }
            } else if self.p(p + 1) == b'-' && p + 2 < ec {
                p += 2;
                if self.p(p - 2) <= c && c <= self.p(p) {
                    return sig;
                }
            } else if self.p(p) == c {
                return sig;
            }
        }
        !sig
    }

    fn single_match(&self, c: u8, p: usize, ep: usize) -> bool {
        match self.p(p) {
            b'.' => true,
            L_ESC => match_class(c, self.p(p + 1)),
            b'[' => self.match_bracket_class(c, p, ep - 1),
            pc => pc == c,
        }
    }

    fn match_balance(&self, s: usize, p: usize) -> Result<Option<usize>, PatternError> {
        if self.p(p) == 0 || self.p(p + 1) == 0 {
            return Err(err("unbalanced pattern"));
        }
        if self.s(s) != self.p(p) {
            return Ok(None);
        }
        let open = self.p(p);
        let close = self.p(p + 1);
        let mut depth = 1;
        let mut pos = s;
        loop {
            pos += 1;
            if pos >= self.src.len() {
                break;
            }
            let byte = self.src[pos];
            if byte == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(pos + 1));
                }
            } else if byte == open {
                depth += 1;
            }
        }
        Ok(None)
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> Result<Option<usize>, PatternError> {
        let mut i = 0usize;
        while s + i < self.src.len() && self.single_match(self.src[s + i], p, ep) {
            i += 1;
        }
        loop {
            if let Some(res) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(res));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(
        &mut self,
        mut s: usize,
        p: usize,
        ep: usize,
    ) -> Result<Option<usize>, PatternError> {
        loop {
            if let Some(res) = self.do_match(s, ep + 1)? {
                return Ok(Some(res));
            }
            if s < self.src.len() && self.single_match(self.src[s], p, ep) {
                s += 1;
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(
        &mut self,
        s: usize,
        p: usize,
        what: isize,
    ) -> Result<Option<usize>, PatternError> {
        let level = self.level;
        if level >= MAX_CAPTURES {
            return Err(err("too many captures"));
        }
        self.capture[level] = (s, what);
        self.level = level + 1;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.level -= 1;
        }
        Ok(res)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> Result<Option<usize>, PatternError> {
        let l = self.capture_to_close()?;
        self.capture[l].1 = (s - self.capture[l].0) as isize;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.capture[l].1 = CAP_UNFINISHED;
        }
        Ok(res)
    }

    fn match_capture(&self, s: usize, l: u8) -> Result<Option<usize>, PatternError> {
        let l = self.check_capture(l)?;
        let (init, len) = self.capture[l];
        let len = len as usize;
        if self.src.len() - s >= len && self.src[init..init + len] == self.src[s..s + len] {
            Ok(Some(s + len))
        } else {
            Ok(None)
        }
    }

    fn do_match(&mut self, mut s: usize, mut p: usize) -> Result<Option<usize>, PatternError> {
        loop {
            match self.p(p) {
                b'(' => {
                    return if self.p(p + 1) == b')' {
                        self.start_capture(s, p + 2, CAP_POSITION)
                    } else {
                        self.start_capture(s, p + 1, CAP_UNFINISHED)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                L_ESC if self.p(p + 1) == b'b' => match self.match_balance(s, p + 2)? {
                    Some(next) => {
                        s = next;
                        p += 4;
                    }
                    None => return Ok(None),
                },
                L_ESC if self.p(p + 1) == b'f' => {
                    p += 2;
                    if self.p(p) != b'[' {
                        return Err(err("missing '[' after '%f' in pattern"));
                    }
                    let ep = self.class_end(p)?;
                    let previous = if s == 0 { 0 } else { self.s(s - 1) };
                    if self.match_bracket_class(previous, p, ep - 1)
                        || !self.match_bracket_class(self.s(s), p, ep - 1)
                    {
                        return Ok(None);
                    }
                    p = ep;
                }
                L_ESC if self.p(p + 1).is_ascii_digit() => {
                    match self.match_capture(s, self.p(p + 1))? {
                        Some(next) => {
                            s = next;
                            p += 2;
                        }
                        None => return Ok(None),
                    }
                }
                0 => return Ok(Some(s)),
                b'$' if self.p(p + 1) == 0 => {
                    return Ok(if s == self.src.len() { Some(s) } else { None });
                }
                _ => {
                    let ep = self.class_end(p)?;
                    let m = s < self.src.len() && self.single_match(self.src[s], p, ep);
                    match self.p(ep) {
                        b'?' => {
                            if m && let Some(res) = self.do_match(s + 1, ep + 1)? {
                                return Ok(Some(res));
                            }
                            p = ep + 1;
                        }
                        b'*' => return self.max_expand(s, p, ep),
                        b'+' => {
                            return if m {
                                self.max_expand(s + 1, p, ep)
                            } else {
                                Ok(None)
                            };
                        }
                        b'-' => return self.min_expand(s, p, ep),
                        _ => {
                            if !m {
                                return Ok(None);
                            }
                            s += 1;
                            p = ep;
                        }
                    }
                }
            }
        }
    }

    fn captures(
        &self,
        s: usize,
        e: usize,
        whole_if_none: bool,
    ) -> Result<Vec<Capture>, PatternError> {
        let n = if self.level == 0 && whole_if_none {
            1
        } else {
            self.level
        };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if i >= self.level {
                // Only reachable for i == 0: the whole match.
                out.push(Capture::Str(s, e));
                continue;
            }
            let (init, len) = self.capture[i];
            if len == CAP_UNFINISHED {
                return Err(err("unfinished capture"));
            }
            if len == CAP_POSITION {
                out.push(Capture::Pos(init + 1));
            } else {
                out.push(Capture::Str(init, init + len as usize));
            }
        }
        Ok(out)
    }
}

fn mem_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The start offset (0-based) of a 1-based, possibly negative `init`, as
/// `str_find_aux` clamps it.
const fn start_offset(init: i64, len: usize) -> usize {
    let len_i = len as i64;
    let mut init = if init < 0 { len_i + init + 1 } else { init };
    init -= 1;
    if init < 0 {
        0
    } else if init > len_i {
        len
    } else {
        init as usize
    }
}

fn search(
    subject: &[u8],
    pattern: &[u8],
    init: i64,
    find: bool,
    plain: bool,
) -> Result<Option<Match>, PatternError> {
    let init = start_offset(init, subject.len());
    if find && (plain || !pattern.iter().any(|c| SPECIALS.contains(c))) {
        return Ok(mem_find(&subject[init..], pattern).map(|at| Match {
            start: init + at + 1,
            end: init + at + pattern.len(),
            captures: Vec::new(),
        }));
    }
    let (anchor, pat) = match pattern.first() {
        Some(b'^') => (true, &pattern[1..]),
        _ => (false, pattern),
    };
    let mut ms = MatchState {
        src: subject,
        pat,
        level: 0,
        capture: [(0, 0); MAX_CAPTURES],
    };
    let mut s1 = init;
    loop {
        ms.level = 0;
        if let Some(e) = ms.do_match(s1, 0)? {
            let captures = ms.captures(s1, e, !find)?;
            return Ok(Some(Match {
                start: s1 + 1,
                end: e,
                captures,
            }));
        }
        if anchor || s1 >= subject.len() {
            return Ok(None);
        }
        s1 += 1;
    }
}

/// `string.find(subject, pattern, init)`.
pub fn find(subject: &[u8], pattern: &[u8], init: i64) -> Result<Option<Match>, PatternError> {
    search(subject, pattern, init, true, false)
}

/// `string.match(subject, pattern, init)`: the captures, or the whole match
/// when the pattern has none.
pub fn lua_match(
    subject: &[u8],
    pattern: &[u8],
    init: i64,
) -> Result<Option<Vec<Capture>>, PatternError> {
    Ok(search(subject, pattern, init, false, false)?.map(|m| m.captures))
}

/// `not not string.match(subject, pattern)`: luacheck's `utils.pmatch`.
pub fn pmatch(subject: &[u8], pattern: &[u8]) -> Result<bool, PatternError> {
    Ok(search(subject, pattern, 1, false, false)?.is_some())
}

/// `string.gsub(subject, pattern, f)` with a function replacement that
/// receives the first capture (or the whole match) and returns the
/// replacement, or `None` to keep the match. Returns the result and the
/// number of matches.
pub fn gsub(
    subject: &[u8],
    pattern: &[u8],
    mut f: impl FnMut(&[Capture]) -> Option<Vec<u8>>,
) -> Result<(Vec<u8>, usize), PatternError> {
    let (anchor, pat) = match pattern.first() {
        Some(b'^') => (true, &pattern[1..]),
        _ => (false, pattern),
    };
    let mut ms = MatchState {
        src: subject,
        pat,
        level: 0,
        capture: [(0, 0); MAX_CAPTURES],
    };
    let mut out = Vec::with_capacity(subject.len());
    let mut src = 0usize;
    let mut n = 0usize;
    loop {
        ms.level = 0;
        let e = ms.do_match(src, 0)?;
        if let Some(e) = e {
            n += 1;
            let caps = ms.captures(src, e, true)?;
            match f(&caps) {
                Some(rep) => out.extend_from_slice(&rep),
                None => out.extend_from_slice(&subject[src..e]),
            }
        }
        match e {
            Some(e) if e > src => src = e,
            _ => {
                if src < subject.len() {
                    out.push(subject[src]);
                    src += 1;
                } else {
                    break;
                }
            }
        }
        if anchor {
            break;
        }
    }
    if src < subject.len() {
        out.extend_from_slice(&subject[src..]);
    }
    Ok((out, n))
}
