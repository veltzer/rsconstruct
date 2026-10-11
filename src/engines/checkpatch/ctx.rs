//! checkpatch.pl's helper subs: line sanitising, the statement and block
//! walkers, the value annotator and `possible()`.

use super::check::{Checker, at, count, len, split_ws, substr, substr_s};
use super::re::SUBSEP;

pub const TABSIZE: i64 = 8;

/// `expand_tabs`.
pub fn expand_tabs(s: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(s.len());
    let mut n = 0;
    for &c in s {
        if c == b'\t' {
            res.push(b' ');
            n += 1;
            while n % TABSIZE != 0 {
                res.push(b' ');
                n += 1;
            }
            continue;
        }
        res.push(c);
        n += 1;
    }
    res
}

/// `copy_spacing`: every character but a tab becomes a space.
pub fn copy_spacing(s: &[u8]) -> Vec<u8> {
    s.iter()
        .map(|&c| if c == b'\t' { b'\t' } else { b' ' })
        .collect()
}

/// `get_raw_comment`: the raw characters where the sanitised line has a
/// comment.
pub fn get_raw_comment(line: &[u8], rawline: &[u8]) -> Vec<u8> {
    let mut comment = Vec::new();
    for (i, &c) in line.iter().enumerate() {
        if c == SUBSEP {
            comment.extend_from_slice(
                substr(rawline, i64::try_from(i).expect("offset"), Some(1)).unwrap_or(&[]),
            );
        }
    }
    comment
}

impl Checker {
    /// `line_stats`: the length and the indent of a line, its diff marker
    /// dropped and its tabs expanded.
    pub(super) fn line_stats(&self, line: &[u8]) -> (i64, i64) {
        let mut line = line.to_vec();
        self.s(r"^.", "", &mut line, false, &mut |_| Vec::new());
        let line = expand_tabs(&line);
        let white = self.m(r"^(\s*)", &line).map_or(0, |c| len(&c.s(1)));
        (len(&line), white)
    }

    /// `trim`.
    pub(super) fn trim(&self, s: &[u8]) -> Vec<u8> {
        let mut s = s.to_vec();
        self.s(r"^\s+|\s+$", "", &mut s, true, &mut |_| Vec::new());
        s
    }

    /// `rtrim`.
    pub(super) fn rtrim(&self, s: &[u8]) -> Vec<u8> {
        let mut s = s.to_vec();
        self.s(r"\s+$", "", &mut s, false, &mut |_| Vec::new());
        s
    }

    /// `cat_vet`: control characters shown as `^X`, `$` at the end.
    pub(super) fn cat_vet(&self, vet: &[u8]) -> Vec<u8> {
        let mut res = Vec::new();
        for c in self.all(r"([^[:cntrl:]]*)([[:cntrl:]]|$)", "", vet) {
            res.extend(c.s(1));
            let two = c.s(2);
            if let Some(&b) = two.first() {
                res.push(b'^');
                res.push(b.wrapping_add(64));
            }
        }
        self.s(r"$", "", &mut res, false, &mut |_| b"$".to_vec());
        res
    }

    /// `deparenthesize`.
    pub(super) fn deparenthesize(&self, s: Option<&[u8]>) -> Vec<u8> {
        let Some(s) = s else {
            return Vec::new();
        };
        let mut s = s.to_vec();
        while self.t(r"^\s*\(.*\)\s*$", &s) {
            self.s(r"^\s*\(\s*", "", &mut s, false, &mut |_| Vec::new());
            self.s(r"\s*\)\s*$", "", &mut s, false, &mut |_| Vec::new());
        }
        self.s(r"\s+", "", &mut s, true, &mut |_| b" ".to_vec());
        s
    }

    /// `get_quoted_string`: the raw text of the first string on the line.
    pub(super) fn get_quoted_string(&self, line: Option<&[u8]>, rawline: Option<&[u8]>) -> Vec<u8> {
        let (Some(line), Some(rawline)) = (line, rawline) else {
            return Vec::new();
        };
        let Some(c) = self.m(r"($String)", line) else {
            return Vec::new();
        };
        let (s, e) = (c.start(0).expect("match"), c.end(0).expect("match"));
        substr_s(
            rawline,
            i64::try_from(s).expect("start"),
            Some(i64::try_from(e - s).expect("length")),
        )
    }

    /// `sanitise_line_reset`.
    pub(super) const fn sanitise_line_reset(&mut self, in_comment: bool) {
        self.sanitise_quote = if in_comment { b"*/" } else { b"" };
    }

    /// `sanitise_line`: comments become `$;`, the insides of strings and
    /// characters `X`.
    pub(super) fn sanitise_line(&mut self, line: &[u8]) -> Vec<u8> {
        let mut res: Vec<u8> = line.get(..1).unwrap_or(&[]).to_vec();
        let mut off = 1;
        while off < line.len() {
            let c = line[off];
            let two = &line[off..(off + 2).min(line.len())];
            let q = self.sanitise_quote;
            if q.is_empty() && two == b"/*" {
                self.sanitise_quote = b"*/";
                res.extend_from_slice(&[SUBSEP, SUBSEP]);
                off += 2;
                continue;
            }
            if q == b"*/" && two == b"*/" {
                self.sanitise_quote = b"";
                res.extend_from_slice(&[SUBSEP, SUBSEP]);
                off += 2;
                continue;
            }
            if q.is_empty() && two == b"//" {
                self.sanitise_quote = b"//";
                res.extend_from_slice(b"//");
                off += 2;
                continue;
            }
            if (q == b"'" || q == b"\"") && c == b'\\' {
                res.extend_from_slice(b"XX");
                off += 2;
                continue;
            }
            if c == b'\'' || c == b'"' {
                if q.is_empty() {
                    self.sanitise_quote = if c == b'\'' { b"'" } else { b"\"" };
                    res.push(c);
                    off += 1;
                    continue;
                } else if q == [c] {
                    self.sanitise_quote = b"";
                }
            }
            let q = self.sanitise_quote;
            if (q == b"*/" || q == b"//") && c != b'\t' {
                res.push(SUBSEP);
            } else if !q.is_empty() && c != b'\t' {
                res.push(b'X');
            } else {
                res.push(c);
            }
            off += 1;
        }
        if self.sanitise_quote == b"//" {
            self.sanitise_quote = b"";
        }

        if let Some(c) = self.m(r"^.\s*\#\s*include\s+\<(.*)\>", &res) {
            let clean = vec![b'X'; c.s(1).len()];
            self.s(r"\<.*\>", "", &mut res, false, &mut |_| {
                [b"<", &clean[..], b">"].concat()
            });
        } else if let Some(c) = self.m(r"^.\s*\#\s*(?:error|warning)\s+(.*)\b", &res) {
            let clean = vec![b'X'; c.s(1).len()];
            self.s(
                r"(\#\s*(?:error|warning)\s+).*",
                "",
                &mut res,
                false,
                &mut |c| [&c.s(1)[..], &clean[..]].concat(),
            );
        }

        if let Some(c) = self.m(r"(//.*$)", &res) {
            let matched = c.s(1);
            let re = self.rel(r"\Q$match\E", "", &[("match", &matched)]);
            super::check::subst(&re, &mut res, false, &mut |_| vec![SUBSEP; matched.len()]);
        }
        res
    }

    /// `$lines[$i]`.
    pub(super) fn line_at(&self, i: i64) -> Option<&[u8]> {
        at(&self.lines, i).map(Vec::as_slice)
    }

    /// `$rawlines[$i]`.
    pub(super) fn rawline_at(&self, i: i64) -> Option<&[u8]> {
        at(&self.rawlines, i).map(Vec::as_slice)
    }

    /// `ctx_statement_block`: the statement starting at offset `off` of
    /// line `linenr`, and its condition.
    pub(super) fn ctx_statement_block(
        &self,
        linenr: i64,
        mut remain: i64,
        mut off: i64,
    ) -> StatementBlock {
        let mut line = linenr - 1;
        let mut blk: Vec<u8> = Vec::new();
        let soff = off;
        let mut coff = off - 1;
        let mut coff_set = false;
        let mut loff = 0;
        let mut ty: u8 = 0; // '' as 0
        let mut level: i64 = 0;
        let mut stack: Vec<(u8, i64)> = Vec::new();
        let mut p: Option<u8>;
        let mut c: Option<u8> = None;
        let mut blen: i64 = 0;
        loop {
            if stack.is_empty() {
                stack.push((0, 0));
            }
            if off >= blen {
                while remain > 0 {
                    let Some(l) = self.line_at(line) else {
                        break;
                    };
                    if self.t(r"^-", l) {
                        line += 1;
                        continue;
                    }
                    remain -= 1;
                    loff = blen;
                    blk.extend_from_slice(l);
                    blk.push(b'\n');
                    blen = len(&blk);
                    line += 1;
                    break;
                }
                if off >= blen {
                    break;
                }
                if level == 0 && self.t(r"^.\s*#\s*define", substr(&blk, off, None).unwrap_or(&[]))
                {
                    level += 1;
                    ty = b'#';
                }
            }
            p = c;
            let o = usize::try_from(off).expect("offset");
            c = Some(blk[o]);
            let remainder = &blk[o..];

            if self.t(r"^#\s*(?:ifndef|ifdef|if)\s", remainder) {
                stack.push((ty, level));
            } else if self.t(r"^#\s*(?:else|elif)\b", remainder) {
                let top = *at(&stack, len_of(&stack) - 2).expect("a stack entry");
                (ty, level) = top;
            } else if self.t(r"^#\s*endif\b", remainder) {
                (ty, level) = stack.pop().expect("a stack entry");
            }

            if level == 0 && c == Some(b';') {
                break;
            }

            if level == 0
                && !coff_set
                && p.is_none_or(|p| self.t(r"(?:\s|\}|\+)", &[p]))
                && let Some(m) = self.m(r"^(else)(?:\s|{)", remainder)
                && !self.t(r"^else\s+if\b", remainder)
            {
                coff = off + len(&m.s(1)) - 1;
                coff_set = true;
            }

            if (ty == 0 || ty == b'(') && c == Some(b'(') {
                level += 1;
                ty = b'(';
            }
            if ty == b'(' && c == Some(b')') {
                level -= 1;
                ty = if level != 0 { b'(' } else { 0 };
                if level == 0 && coff < soff {
                    coff = off;
                    coff_set = true;
                }
            }
            if (ty == 0 || ty == b'{') && c == Some(b'{') {
                level += 1;
                ty = b'{';
            }
            if ty == b'{' && c == Some(b'}') {
                level -= 1;
                ty = if level != 0 { b'{' } else { 0 };
                if level == 0 {
                    if substr(&blk, off + 1, Some(1)) == Some(b";") {
                        off += 1;
                    }
                    break;
                }
            }
            if ty == b'#' && c == Some(b'\n') && p != Some(b'\\') {
                level -= 1;
                off += 1;
                break;
            }
            off += 1;
        }
        if off == blen {
            loff = blen + 1;
            line += 1;
            remain -= 1;
        }
        let statement = substr_s(&blk, soff, Some(off - soff + 1));
        let condition = substr_s(&blk, soff, Some(coff - soff + 1));
        StatementBlock {
            statement,
            condition,
            line,
            remain: remain + 1,
            off: off - loff + 1,
            level,
        }
    }

    /// `ctx_statement_full`: the statement and the `else`/`do` arms that
    /// continue it, each as `(condition, statement)`.
    pub(super) fn ctx_statement_full(
        &self,
        linenr: i64,
        remain: i64,
        off: i64,
    ) -> (i64, i64, Vec<Chunk>) {
        let mut chunks = Vec::new();
        let b = self.ctx_statement_block(linenr, remain, off);
        let (mut linenr, mut remain, mut off, mut level) = (b.line, b.remain, b.off, b.level);
        let first =
            !(remain > 0 && self.tf(r"^\s*(?:\n[+-])?\s*(?:if|else|do)\b", "s", &b.condition));
        chunks.push((b.condition, b.statement));
        if first {
            return (level, linenr, chunks);
        }
        loop {
            let b = self.ctx_statement_block(linenr, remain, off);
            (linenr, remain, off, level) = (b.line, b.remain, b.off, b.level);
            if !(remain > 0 && self.tf(r"^(?:\s*\n[+-])*\s*(?:else|do)\b", "s", &b.condition)) {
                break;
            }
            chunks.push((b.condition, b.statement));
        }
        (level, linenr, chunks)
    }

    /// `ctx_block_get`: the raw lines of the block opened on line `linenr`.
    pub(super) fn ctx_block_get(
        &self,
        linenr: i64,
        mut remain: i64,
        outer: bool,
        open: u8,
        close: u8,
        mut off: i64,
    ) -> (i64, Vec<Vec<u8>>) {
        let mut res = Vec::new();
        let mut level: i64 = 0;
        let mut stack: Vec<i64> = vec![level];
        let mut line = linenr - 1;
        while remain > 0 {
            let raw = self.rawline_at(line);
            if raw.is_some_and(|r| self.t(r"^-", r)) {
                line += 1;
                continue;
            }
            remain -= 1;
            let l = self.line_at(line).unwrap_or(&[]);
            if self.t(r"^.\s*#\s*(?:ifndef|ifdef|if)\s", l) {
                stack.push(level);
            } else if self.t(r"^.\s*#\s*(?:else|elif)\b", l) {
                level = at(&stack, len_of(&stack) - 2).copied().unwrap_or(0);
            } else if self.t(r"^.\s*#\s*endif\b", l) {
                level = stack.pop().unwrap_or(0);
            }
            for &c in l {
                if off > 0 {
                    off -= 1;
                    continue;
                }
                if c == close && level > 0 {
                    level -= 1;
                    if level == 0 {
                        break;
                    }
                } else if c == open {
                    level += 1;
                }
            }
            if !outer || level <= 1 {
                res.push(raw.map(<[u8]>::to_vec).unwrap_or_default());
            }
            if level == 0 {
                break;
            }
            line += 1;
        }
        (level, res)
    }

    /// `ctx_locate_comment`.
    fn ctx_locate_comment(&self, first_line: i64, end_line: i64) -> Vec<u8> {
        for (i, tpl) in [
            (end_line - 1, r"^\+.*(//.*$)"),
            (end_line - 2, r"^[\+ ].*(//.*$)"),
            (end_line, r"^[\+ ].*(//.*$)"),
            (end_line - 1, r".*(/\*.*\*/)\s*(?:\\\s*)?$"),
        ] {
            if let Some(c) = self.rawline_at(i).and_then(|l| self.m(tpl, l))
                && let Some(g) = c.get(1)
            {
                return g.to_vec();
            }
        }
        let mut in_comment = false;
        let mut current: Vec<u8> = Vec::new();
        let mut linenr = first_line;
        while linenr < end_line {
            let line = self.rawline_at(linenr - 1).unwrap_or(&[]);
            if linenr == first_line && self.t(r"^.\s*\*", line) {
                in_comment = true;
            }
            if self.t(r"/\*", line) {
                in_comment = true;
            }
            if !in_comment && !current.is_empty() {
                current.clear();
            }
            if in_comment {
                current.extend_from_slice(line);
                current.push(b'\n');
            }
            if self.t(r"\*/", line) {
                in_comment = false;
            }
            linenr += 1;
        }
        if current.last() == Some(&b'\n') {
            current.pop();
        }
        current
    }

    /// `ctx_has_comment`.
    pub(super) fn ctx_has_comment(&self, first_line: i64, end_line: i64) -> bool {
        !self.ctx_locate_comment(first_line, end_line).is_empty()
    }

    /// `raw_line`: the `cnt`th raw line from `linenr` on, removed lines
    /// skipped.
    pub(super) fn raw_line(&self, linenr: i64, cnt: i64) -> Option<Vec<u8>> {
        let mut offset = linenr - 1;
        let mut cnt = cnt + 1;
        let mut line = None;
        while cnt > 0 {
            line = self.rawline_at(offset);
            offset += 1;
            if line.is_some_and(|l| self.t(r"^-", l)) {
                continue;
            }
            cnt -= 1;
        }
        line.map(<[u8]>::to_vec)
    }

    /// `get_stat_real`.
    pub(super) fn get_stat_real(&self, linenr: i64, lc: i64) -> Vec<u8> {
        let mut stat_real = self.raw_line(linenr, 0).unwrap_or_default();
        let mut count = linenr + 1;
        while count <= lc {
            stat_real.push(b'\n');
            stat_real.extend(self.raw_line(count, 0).unwrap_or_default());
            count += 1;
        }
        stat_real
    }

    /// `get_stat_here`.
    pub(super) fn get_stat_here(&self, linenr: i64, cnt: i64, here: &[u8]) -> Vec<u8> {
        let mut herectx = here.to_vec();
        herectx.push(b'\n');
        for n in 0..cnt {
            herectx.extend(self.raw_line(linenr, n).unwrap_or_default());
            herectx.push(b'\n');
        }
        herectx
    }

    /// `statement_lines`.
    pub(super) fn statement_lines(&self, stmt: &[u8]) -> i64 {
        let mut stmt = stmt.to_vec();
        self.s(r"(^|\n).", "", &mut stmt, true, &mut |c| c.s(1));
        self.s(r"^\s*", "", &mut stmt, false, &mut |_| Vec::new());
        self.s(r"\s*$", "", &mut stmt, false, &mut |_| Vec::new());
        count(&stmt, b'\n') + 1
    }

    /// `statement_block_size`.
    pub(super) fn statement_block_size(&self, stmt: &[u8]) -> i64 {
        let mut stmt = stmt.to_vec();
        self.s(r"(^|\n).", "", &mut stmt, true, &mut |c| c.s(1));
        self.s(r"^\s*{", "", &mut stmt, false, &mut |_| Vec::new());
        self.s(r"}\s*$", "", &mut stmt, false, &mut |_| Vec::new());
        self.s(r"^\s*", "", &mut stmt, false, &mut |_| Vec::new());
        self.s(r"\s*$", "", &mut stmt, false, &mut |_| Vec::new());
        let lines = count(&stmt, b'\n') + 1;
        let statements = count(&stmt, b';');
        if lines > statements {
            lines
        } else {
            statements
        }
    }

    /// `annotate_reset`.
    pub(super) fn annotate_reset(&mut self) {
        self.av_preprocessor = false;
        self.av_pending = b'_';
        self.av_paren_type = vec![b'E'];
        self.av_pend_colon = b'O';
    }

    /// `annotate_values`: the value type at each character of `stream`
    /// (`res`), and the variant of each operator (`var`).
    pub(super) fn annotate_values(&mut self, stream: &[u8], mut ty: u8) -> (Vec<u8>, Vec<u8>) {
        let mut res: Vec<u8> = Vec::new();
        let mut var = vec![b'_'; stream.len()];
        let mut cur: &[u8] = stream;
        while !cur.is_empty() {
            if self.av_paren_type.is_empty() {
                self.av_paren_type.push(b'E');
            }
            let taken: usize;
            if let Some(c) = self.m(r"^(\s+)", cur) {
                taken = c.s(1).len();
                if c.s(1).contains(&b'\n') && self.av_preprocessor {
                    ty = self.av_paren_type.pop().unwrap_or(0);
                    self.av_preprocessor = false;
                }
            } else if let Some(c) = self
                .m(r"^(\(\s*$Type\s*)\)", cur)
                .filter(|_| self.av_pending == b'_')
            {
                taken = c.s(1).len();
                self.av_paren_type.push(ty);
                ty = b'c';
            } else if let Some(c) = self.m(r"^($Type)\s*(?:$Ident|,|\)|\(|\s*$)", cur) {
                taken = c.s(1).len();
                ty = b'T';
            } else if let Some(c) = self.m(r"^($Modifier)\s*", cur) {
                taken = c.s(1).len();
                ty = b'T';
            } else if let Some(c) = self.m(r"^(\#\s*define\s*$Ident)(\(?)", cur) {
                taken = c.s(1).len();
                self.av_preprocessor = true;
                self.av_paren_type.push(ty);
                if !c.s(2).is_empty() {
                    self.av_pending = b'N';
                }
                ty = b'E';
            } else if let Some(c) = self.m(r"^(\#\s*(?:undef\s*$Ident|include\b))", cur) {
                taken = c.s(1).len();
                self.av_preprocessor = true;
                self.av_paren_type.push(ty);
            } else if let Some(c) = self.m(r"^(\#\s*(?:ifdef|ifndef|if))", cur) {
                taken = c.s(1).len();
                self.av_preprocessor = true;
                self.av_paren_type.push(ty);
                self.av_paren_type.push(ty);
                ty = b'E';
            } else if let Some(c) = self.m(r"^(\#\s*(?:else|elif))", cur) {
                taken = c.s(1).len();
                self.av_preprocessor = true;
                let top = *self.av_paren_type.last().expect("a paren type");
                self.av_paren_type.push(top);
                ty = b'E';
            } else if let Some(c) = self.m(r"^(\#\s*(?:endif))", cur) {
                taken = c.s(1).len();
                self.av_preprocessor = true;
                self.av_paren_type.pop();
                self.av_paren_type.push(ty);
                ty = b'E';
            } else if let Some(c) = self.m(r"^(\\\n)", cur) {
                taken = c.s(1).len();
            } else if let Some(c) = self.m(r"^(__attribute__)\s*\(?", cur) {
                taken = c.s(1).len();
                self.av_pending = ty;
                ty = b'N';
            } else if let Some(c) = self.m(r"^(sizeof)\s*(\()?", cur) {
                taken = c.s(1).len();
                if c.defined(2) {
                    self.av_pending = b'V';
                }
                ty = b'N';
            } else if let Some(c) = self.m(r"^(if|while|for)\b", cur) {
                taken = c.s(1).len();
                self.av_pending = b'E';
                ty = b'N';
            } else if let Some(c) = self.m(r"^(case)", cur) {
                taken = c.s(1).len();
                self.av_pend_colon = b'C';
                ty = b'N';
            } else if let Some(c) = self.m(r"^(return|else|goto|typeof|__typeof__)\b", cur) {
                taken = c.s(1).len();
                ty = b'N';
            } else if let Some(c) = self.m(r"^(\()", cur) {
                taken = c.s(1).len();
                self.av_paren_type.push(self.av_pending);
                self.av_pending = b'_';
                ty = b'N';
            } else if let Some(c) = self.m(r"^(\))", cur) {
                taken = c.s(1).len();
                let new_type = self.av_paren_type.pop().unwrap_or(0);
                if new_type != b'_' {
                    ty = new_type;
                }
            } else if let Some(c) = self.m(r"^($Ident)\s*\(", cur) {
                taken = c.s(1).len();
                ty = b'V';
                self.av_pending = b'V';
            } else if let Some(c) = self.m(r"^($Ident\s*):(?:\s*\d+\s*(,|=|;))?", cur) {
                taken = c.s(1).len();
                if c.defined(2) && ty == b'C' || ty == b'T' {
                    self.av_pend_colon = b'B';
                } else if ty == b'E' {
                    self.av_pend_colon = b'L';
                }
                ty = b'V';
            } else if let Some(c) = self.m(r"^($Ident|$Constant)", cur) {
                taken = c.s(1).len();
                ty = b'V';
            } else if let Some(c) = self.m(r"^($Assignment)", cur) {
                taken = c.s(1).len();
                ty = b'N';
            } else if let Some(c) = self.m(r"^(;|{|})", cur) {
                taken = c.s(1).len();
                ty = b'E';
                self.av_pend_colon = b'O';
            } else if let Some(c) = self.m(r"^(,)", cur) {
                taken = c.s(1).len();
                ty = b'C';
            } else if let Some(c) = self.m(r"^(\?)", cur) {
                taken = c.s(1).len();
                ty = b'N';
            } else if let Some(c) = self.m(r"^(:)", cur) {
                taken = c.s(1).len();
                var[res.len()] = self.av_pend_colon;
                ty = if self.av_pend_colon == b'C' || self.av_pend_colon == b'L' {
                    b'E'
                } else {
                    b'N'
                };
                self.av_pend_colon = b'O';
            } else if let Some(c) = self.m(r"^(\[)", cur) {
                taken = c.s(1).len();
                ty = b'N';
            } else if let Some(c) = self.m(r"^(-(?![->])|\+(?!\+)|\*|\&\&|\&)", cur) {
                taken = c.s(1).len();
                var[res.len()] = if ty == b'V' { b'B' } else { b'U' };
                ty = b'N';
            } else if let Some(c) = self.m(r"^($Operators)", cur) {
                taken = c.s(1).len();
                let op = c.s(1);
                if op != b"++" && op != b"--" {
                    ty = b'N';
                }
            } else if let Some(c) = self.m(r"(^.)", cur) {
                taken = c.s(1).len();
            } else {
                unreachable!("annotate_values: every character is matched");
            }
            cur = &cur[taken..];
            if ty != 0 {
                res.extend(std::iter::repeat_n(ty, taken));
            }
        }
        (res, var)
    }

    /// `possible`: a word used like a type or a modifier joins the type
    /// patterns.
    pub(super) fn possible(&mut self, possible: &[u8]) {
        let not_permitted = self.re(
            r"(?:
		^(?:
			$Modifier|
			$Storage|
			$Type|
			DEFINE_\S+
		)$|
		^(?:
			goto|
			return|
			case|
			else|
			asm|__asm__|
			do|
			\#|
			\#\#|
		)(?:\s|$)|
		^(?:typedef|struct|enum)\b
	    )",
            "x",
        );
        if not_permitted.find(possible).is_some() {
            return;
        }
        let mut possible = possible.to_vec();
        self.s(r"\s*$Storage\s*", "", &mut possible, true, &mut |_| {
            Vec::new()
        });
        self.s(r"\s*$Sparse\s*", "", &mut possible, true, &mut |_| {
            Vec::new()
        });
        let mut changed = false;
        if self.t(r"^\s*$", &possible) {
        } else if self.t(r"\s", &possible) {
            self.s(r"\s*$Type\s*", "", &mut possible, true, &mut |_| Vec::new());
            for modifier in split_ws(&possible) {
                if not_permitted.find(&modifier).is_none() {
                    self.v.modifier_list_file.push(modifier);
                    changed = true;
                }
            }
        } else {
            self.v.type_list_file.push(possible);
            changed = true;
        }
        if changed {
            self.v.build_types();
            self.types_changed();
        }
    }

    /// `pos_last_openparen`: the column after the last unclosed `(`.
    pub(super) fn pos_last_openparen(&self, line: &[u8]) -> i64 {
        let opens = count(line, b'(');
        let closes = count(line, b')');
        let mut last_openparen = 0;
        if opens == 0 || closes >= opens {
            return -1;
        }
        let l = len(line);
        let mut pos = 0;
        while pos < l {
            let string = substr(line, pos, None).unwrap_or(&[]);
            if let Some(c) = self.m(r"^($FuncArg|$balanced_parens)", string) {
                pos += len(&c.s(1)) - 1;
            } else if substr(line, pos, Some(1)) == Some(b"(") {
                last_openparen = pos;
            } else if !string.contains(&b'(') {
                break;
            }
            pos += 1;
        }
        len(&expand_tabs(
            substr(line, 0, Some(last_openparen)).unwrap_or(&[]),
        )) + 1
    }

    /// `perms_to_octal`.
    pub(super) fn perms_to_octal(&self, string: &[u8]) -> Vec<u8> {
        if self.t(r"^\s*0[0-7]{3,3}\s*$", string) {
            return self.trim(string);
        }
        let mut to = 0u32;
        let mut lastpos = 0;
        for c in self.all(
            r"\b(($single_mode_perms_string_search)\b(?:\s*\|\s*)?\s*)",
            "",
            string,
        ) {
            let curpos = c.end(0).expect("match");
            let omatch = c.s(1);
            if lastpos > 0 && curpos - omatch.len() != lastpos {
                break;
            }
            lastpos = curpos;
            let name = c.s(2);
            to |= super::globals::MODE_PERMISSION_STRING_TYPES
                .iter()
                .find(|(n, _)| n.as_bytes() == name.as_slice())
                .map_or(0, |(_, v)| *v);
        }
        format!("{to:04o}").into_bytes()
    }
}

fn len_of<T>(v: &[T]) -> i64 {
    i64::try_from(v.len()).expect("length")
}

/// One arm of `ctx_statement_full`: its condition and its statement.
pub type Chunk = (Vec<u8>, Vec<u8>);

/// What `ctx_statement_block` returns.
pub struct StatementBlock {
    pub statement: Vec<u8>,
    pub condition: Vec<u8>,
    pub line: i64,
    pub remain: i64,
    pub off: i64,
    pub level: i64,
}
