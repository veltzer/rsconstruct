//! checkpatch.pl's `process`: the checks, line by line, in its order.
//!
//! Only `--no-tree -f` mode is ported: the input is a single new file, so
//! the commit-message, sign-off and kernel-tree checks, which that mode can
//! never reach, are left out. `--strict` is off, so a CHECK only reports
//! for files under the paths checkpatch turns it on for (`net/`,
//! `drivers/net/`, `drivers/staging/`); work that only feeds a CHECK is
//! skipped when it cannot report.
//!
//! Each check is written as checkpatch.pl writes it, `if`s and all, so the
//! two can be read side by side; the mutation and single-letter names
//! (`p` for `process`'s variables, `l` for the line's) follow it too.

// `${name}` in these templates is Perl interpolation, not a format argument.
#![allow(clippy::literal_string_with_formatting_args)]
// Written as the Perl is: mutated variables, `if` chains with equal arms.
#![allow(
    clippy::useless_let_if_seq,
    clippy::if_same_then_else,
    clippy::many_single_char_names
)]

use std::collections::{HashMap, HashSet};

use super::check::{Checker, cat, count, len, split, subst, substr, substr_s};
use super::ctx::{TABSIZE, copy_spacing, expand_tabs, get_raw_comment};
use super::globals::{ALLOW_REPEATED_WORDS, DEPRECATED_APIS, MODE_PERMISSION_FUNCS};
use super::re::{SUBSEP, qr, quotemeta_bytes};

const MAX_LINE_LENGTH: i64 = 100;
const MIN_CONF_DESC_LENGTH: i64 = 4;

const DO_WHILE_0_ADVICE: &str = "
   do {} while (0) advice is over-stated in a few situations:

   The more obvious case is macros, like MODULE_PARM_DESC, invoked at
   file-scope, where C disallows code (it must be in functions).  See
   $exceptions if you have one to add by name.

   More troublesome is declarative macros used at top of new scope,
   like DECLARE_PER_CPU.  These might just compile with a do-while-0
   wrapper, but would be incorrect.  Most of these are handled by
   detecting struct,union,etc declaration primitives in $exceptions.

   Theres also macros called inside an if (block), which \"return\" an
   expression.  These cannot do-while, and need a ({}) wrapper.

   Enjoy this qualification while we work to improve our heuristics.
";

/// `process`'s variables that live across lines.
#[derive(Default)]
struct P {
    realfile: Vec<u8>,
    realline: i64,
    realcnt: i64,
    prevline: Vec<u8>,
    prevrawline: Vec<u8>,
    stashline: Vec<u8>,
    stashrawline: Vec<u8>,
    length: i64,
    indent: i64,
    previndent: i64,
    stashindent: i64,
    first_line: i64,
    context_function: Option<Vec<u8>>,
    suppress_ifbraces: HashSet<i64>,
    suppress_whiletrailers: HashSet<i64>,
    suppress_export: HashMap<i64, i64>,
    suppress_statement: i64,
    prev_values: u8,
    last_blank_line: i64,
    checklicenseline: i64,
    is_patch: bool,
    reported_maintainer_file: bool,
    is_binding_patch: i64,
}

/// One line's variables.
#[derive(Default)]
struct L {
    linenr: i64,
    line: Vec<u8>,
    sline: Vec<u8>,
    rawline: Vec<u8>,
    raw_comment: Vec<u8>,
    here: Vec<u8>,
    hereline: Vec<u8>,
    herecurr: Vec<u8>,
    hereprev: Vec<u8>,
    hunk_line: bool,
    stat: Option<Vec<u8>>,
    cond: Option<Vec<u8>>,
    line_nr_next: Option<i64>,
    remain_next: Option<i64>,
    off_next: Option<i64>,
    realline_next: Option<i64>,
    opline: Vec<u8>,
    curr_values: Vec<u8>,
    curr_vars: Vec<u8>,
    /// `pos($line)`.
    line_pos: Option<usize>,
}

/// A number as Perl reads one from the start of a string.
fn num(s: &[u8]) -> i64 {
    let digits: Vec<u8> = s.iter().copied().take_while(u8::is_ascii_digit).collect();
    std::str::from_utf8(&digits)
        .ok()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0)
}

/// A string of digits as a Perl number, for comparisons.
fn fnum(s: &[u8]) -> f64 {
    let digits: Vec<u8> = s.iter().copied().take_while(u8::is_ascii_digit).collect();
    std::str::from_utf8(&digits)
        .ok()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0)
}

/// Perl truth of a string.
fn truthy(s: &[u8]) -> bool {
    !s.is_empty() && s != b"0"
}

fn n(i: i64) -> Vec<u8> {
    i.to_string().into_bytes()
}

/// How many bytes at the start of `s` are `$UTF8` units: tab, newline,
/// carriage return, printable ASCII or a well-formed UTF-8 sequence.
fn utf8_prefix(s: &[u8]) -> usize {
    let cont = |b: Option<&u8>, lo: u8, hi: u8| b.is_some_and(|&b| (lo..=hi).contains(&b));
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        let unit = match b {
            b'\t' | b'\n' | b'\r' | 0x20..=0x7e => 1,
            0xc2..=0xdf if cont(s.get(i + 1), 0x80, 0xbf) => 2,
            0xe0 if cont(s.get(i + 1), 0xa0, 0xbf) && cont(s.get(i + 2), 0x80, 0xbf) => 3,
            0xe1..=0xec | 0xee | 0xef
                if cont(s.get(i + 1), 0x80, 0xbf) && cont(s.get(i + 2), 0x80, 0xbf) =>
            {
                3
            }
            0xed if cont(s.get(i + 1), 0x80, 0x9f) && cont(s.get(i + 2), 0x80, 0xbf) => 3,
            0xf0 if cont(s.get(i + 1), 0x90, 0xbf)
                && cont(s.get(i + 2), 0x80, 0xbf)
                && cont(s.get(i + 3), 0x80, 0xbf) =>
            {
                4
            }
            0xf1..=0xf3
                if cont(s.get(i + 1), 0x80, 0xbf)
                    && cont(s.get(i + 2), 0x80, 0xbf)
                    && cont(s.get(i + 3), 0x80, 0xbf) =>
            {
                4
            }
            0xf4 if cont(s.get(i + 1), 0x80, 0x8f)
                && cont(s.get(i + 2), 0x80, 0xbf)
                && cont(s.get(i + 3), 0x80, 0xbf) =>
            {
                4
            }
            _ => return i,
        };
        i += unit;
    }
    i
}

impl Checker {
    pub(super) fn process(&mut self, filename: &[u8]) -> bool {
        self.prescan();

        let mut p = P {
            prev_values: b'E',
            checklicenseline: 1,
            is_binding_patch: -1,
            ..P::default()
        };
        for idx in 0..self.lines.len() {
            let line = self.lines[idx].clone();
            let mut sline = line.clone();
            for b in &mut sline {
                if *b == SUBSEP {
                    *b = b' ';
                }
            }
            let rawline = self.rawlines[idx].clone();
            let raw_comment = get_raw_comment(&line, &rawline);
            let mut l = L {
                linenr: i64::try_from(idx).expect("line number") + 1,
                line,
                sline,
                rawline,
                raw_comment,
                ..L::default()
            };
            if !self.header_checks(&mut p, &mut l) {
                continue;
            }
            if !self.hunk_checks(&mut p, &l) {
                continue;
            }
            if !self.c_checks(&mut p, &mut l) {
                continue;
            }
            self.added_line_checks(&mut p, &mut l);
            self.spacing_checks(&p, &l);
            self.statement_checks(&mut p, &mut l);
            self.macro_checks(&mut p, &mut l);
            self.string_checks(&p, &l);
            self.call_checks(&p, &l);
            self.declaration_checks(&mut p, &l);
        }

        if !p.is_patch && !self.t(r"cover-letter\.patch$", filename) {
            self.error(
                "NOT_UNIFIED_DIFF",
                b"Does not appear to be a unified-diff format patch\n",
            );
        }
        self.clean
    }

    /// The pre-scan: every line sanitised into `lines`.
    fn prescan(&mut self) {
        self.sanitise_line_reset(false);
        let mut realcnt: i64 = 0;
        for idx in 0..self.rawlines.len() {
            let linenr = i64::try_from(idx).expect("line number") + 1;
            let rawline = self.rawlines[idx].clone();
            let mut line = rawline.clone();
            if let Some(c) = self.m(r"^\@\@ -\d+(?:,\d+)? \+(\d+)(,(\d+))? \@\@", &rawline) {
                realcnt = if c.defined(2) { num(&c.s(3)) + 1 } else { 2 };
                let mut in_comment = false;
                let mut edge: Option<Vec<u8>> = None;
                let mut cnt = realcnt;
                let mut ln = linenr + 1;
                while cnt > 0 {
                    let raw = self.rawline_at(ln - 1);
                    if raw.is_some_and(|r| self.t(r"^-", r)) {
                        ln += 1;
                        continue;
                    }
                    cnt -= 1;
                    let Some(raw) = raw else {
                        break;
                    };
                    if let Some(e) = self.m(r"(/\*|\*/)", raw)
                        && !self.t(r#""[^"]*(?:/\*|\*/)[^"]*""#, raw)
                    {
                        edge = Some(e.s(1));
                        break;
                    }
                    ln += 1;
                }
                if edge.as_deref() == Some(b"*/") {
                    in_comment = true;
                }
                if edge.is_none()
                    && self
                        .rawline_at(linenr)
                        .is_some_and(|r| self.t(r"^.\s*(?:\*\*+| \*)(?:\s|$)", r))
                {
                    in_comment = true;
                }
                self.sanitise_line_reset(in_comment);
            } else if realcnt != 0 && self.t(r"^(?:\+| |$)", &rawline) {
                line = self.sanitise_line(&rawline);
            }
            if realcnt > 1 {
                if self.t(r"^(?:\+| |$)", &line) {
                    realcnt -= 1;
                }
            } else {
                realcnt = 0;
            }
            self.lines.push(line);
        }
    }

    /// From the top of the loop to `next if (!$hunk_line ...)`.
    fn header_checks(&mut self, p: &mut P, l: &mut L) -> bool {
        if self.t(r"^ mode change [0-7]+ => [0-7]+ \S+\s*$", &l.line)
            || self.t(r"^rename (?:from|to) \S+\s*$", &l.line)
            || self.t(r"^diff --git a\/[\w\/\.\_\-]+ b\/\S+\s*$", &l.line)
        {
            p.is_patch = true;
        }

        if let Some(c) = self.m(r"^\@\@ -\d+(?:,\d+)? \+(\d+)(,(\d+))? \@\@(.*)", &l.line) {
            let context = c.s(4);
            p.is_patch = true;
            p.first_line = l.linenr + 1;
            p.realline = num(&c.s(1)) - 1;
            p.realcnt = if c.defined(2) { num(&c.s(3)) + 1 } else { 2 };
            self.annotate_reset();
            p.prev_values = b'E';
            p.suppress_ifbraces.clear();
            p.suppress_whiletrailers.clear();
            p.suppress_export.clear();
            p.suppress_statement = 0;
            p.context_function = self.m(r"\b(\w+)\s*\(", &context).map(|c| c.s(1));
            return false;
        } else if self.t(r"^( |\+|$)", &l.line) {
            p.realline += 1;
            if p.realcnt != 0 {
                p.realcnt -= 1;
            }
            (p.length, p.indent) = self.line_stats(&l.rawline);
            p.prevline = std::mem::replace(&mut p.stashline, l.line.clone());
            p.previndent = std::mem::replace(&mut p.stashindent, p.indent);
            p.prevrawline = std::mem::replace(&mut p.stashrawline, l.rawline.clone());
        } else if p.realcnt == 1 {
            p.realcnt -= 1;
        }

        l.hunk_line = p.realcnt != 0;
        l.here = format!("#{}: ", p.realline).into_bytes();

        let mut found_file = false;
        if let Some(c) = self.m(r"^diff --git.*?(\S+)$", &l.line) {
            p.realfile = c.s(1);
            found_file = true;
        } else if let Some(c) = self.m(r"^\+\+\+\s+(\S+)", &l.line) {
            p.realfile = c.s(1);
            if self.t(r"^include/asm/", &p.realfile) {
                let msg = cat(&[
                    b"do not modify files in include/asm, change architecture specific files in arch/<architecture>/include/asm\n",
                    &l.here,
                    &l.rawline,
                    b"\n",
                ]);
                self.error("MODIFIED_INCLUDE_ASM", &msg);
            }
            found_file = true;
        }

        if found_file {
            self.check = self.t(r"^(?:drivers/net/|net/|drivers/staging/)", &p.realfile);
            p.checklicenseline = 1;
            if !self.t(r"^(MAINTAINERS|dev\/null)", &p.realfile) {
                let last_binding_patch = p.is_binding_patch;
                p.is_binding_patch = i64::from(self.t(
                    r"^(?:Documentation/devicetree/|include/dt-bindings/)",
                    &p.realfile,
                ));
                if last_binding_patch != -1 && (last_binding_patch ^ p.is_binding_patch) != 0 {
                    self.warn(
                        "DT_SPLIT_BINDING_PATCH",
                        b"DT binding docs and includes should be a separate patch. See: Documentation/devicetree/bindings/submitting-patches.rst\n",
                    );
                }
            }
            return false;
        }

        if p.realcnt != 0 {
            l.here
                .extend(cat(&[b"FILE: ", &p.realfile, b":", &n(p.realline), b":"]));
        }
        l.hereline = cat(&[&l.here, b"\n", &l.rawline, b"\n"]);
        l.herecurr = l.hereline.clone();
        l.hereprev = cat(&[&l.here, b"\n", &p.prevrawline, b"\n", &l.rawline, b"\n"]);

        if p.realcnt != 0 {
            self.cnt_lines += 1;
        }

        // Check for mailing list archives other than lore.kernel.org
        if self.t(r"http.*\b$obsolete_archives", &l.rawline) {
            let msg = cat(&[
                b"Use lore.kernel.org archive links when possible - see https://lore.kernel.org/lists.html\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_LORE_ARCHIVE", &msg);
        }

        // Check for added, moved or deleted files
        if !p.reported_maintainer_file
            && (self.t(r"^(?:new|deleted) file mode\s*\d+\s*$", &l.line)
                || self.t(r"^rename (?:from|to) [\w\/\.\-]+\s*$", &l.line)
                || self.t(r"\{\s*([\w\/\.\-]*)\s*\=\>\s*([\w\/\.\-]*)\s*\}", &l.line))
        {
            p.is_patch = true;
            p.reported_maintainer_file = true;
            let msg = cat(&[
                b"added, moved or deleted file(s), does MAINTAINERS need updating?\n",
                &l.herecurr,
            ]);
            self.warn("FILE_PATH_CHANGES", &msg);
        }

        // Check for wrappage within a valid hunk of the file
        if p.realcnt != 0 && !self.t(r"^(?:\+|-| |\\ No newline|$)", &l.line) {
            if self.emitted_corrupt == 0 {
                let msg = cat(&[b"patch seems to be corrupt (line wrapped?)\n", &l.herecurr]);
                self.error("CORRUPTED_PATCH", &msg);
            }
            self.emitted_corrupt += 1;
        }

        // UTF-8 regex found at http://www.w3.org/International/questions/qa-forms-utf-8.en.php
        if self.check && (p.realfile.is_empty() || l.line.first() == Some(&b'+')) {
            let prefix = utf8_prefix(&l.rawline);
            let whole = prefix == l.rawline.len()
                || (prefix + 1 == l.rawline.len() && l.rawline[prefix] == b'\n');
            if !whole {
                let blank = copy_spacing(&l.rawline);
                let ptr = cat(&[&blank[..prefix], b"^"]);
                let msg = cat(&[
                    b"Invalid UTF-8, patch and commit message should be encoded in UTF-8\n",
                    &l.hereline,
                    &ptr,
                    b"\n",
                ]);
                self.chk("INVALID_UTF8", &msg);
            }
        }

        // check for repeated words separated by a single space
        if l.rawline.first() == Some(&b'+') && !self.t(r"[bcCdDlMnpPs\?-][rwxsStT-]{9}", &l.rawline)
        {
            self.repeated_words(p, l);
        }

        // ignore non-hunk lines and lines being removed
        l.hunk_line && l.line.first() != Some(&b'-')
    }

    fn repeated_words(&mut self, p: &P, l: &L) {
        let rawline = l.rawline.clone();
        let re = self.re(r"\b($word_pattern) (?=($word_pattern))", "");
        let mut pos = 1;
        let mut last_empty_at = None;
        while pos <= rawline.len() {
            let Some(c) = re.find_g(&rawline, pos, last_empty_at) else {
                break;
            };
            let (start, end) = (c.start(), c.end());
            last_empty_at = (start == end).then_some(end);
            pos = end;
            let first = c.str(&rawline, 1);
            let second = c.str(&rawline, 2);
            let start_pos = i64::try_from(c.get(1).expect("group").0).expect("offset");
            let end_pos = i64::try_from(c.get(2).expect("group").1).expect("offset");
            if self.t(r"(?:struct|union|enum)", &first) {
                pos += first.len() + second.len() + 1;
                last_empty_at = None;
                continue;
            }
            if !first.eq_ignore_ascii_case(&second) {
                continue;
            }
            if first == b"long" {
                continue;
            }
            let mut start_char: Vec<u8> = Vec::new();
            let mut end_char: Vec<u8> = Vec::new();
            if start_pos > 1 {
                start_char = substr_s(&rawline, start_pos - 1, Some(1));
            }
            if end_pos < len(&rawline) {
                end_char = substr_s(&rawline, end_pos, Some(1));
            }
            if self.t(r"^\S$", &start_char) {
                continue;
            }
            if !end_char.is_empty() && !b" \t.,;?!".contains(&end_char[0]) {
                continue;
            }
            if self.mf(r"\b[0-9a-f]{2,}\b", "i", &first).is_some() {
                let lc = first.to_ascii_lowercase();
                if !ALLOW_REPEATED_WORDS
                    .iter()
                    .any(|w| w.as_bytes() == lc.as_slice())
                {
                    continue;
                }
            }
            let msg = cat(&[b"Possible repeated word: '", &first, b"'\n", &l.herecurr]);
            self.warn("REPEATED_WORD", &msg);
        }

        // if it's a repeated word on consecutive lines in a comment block
        if self.t(r"$;+\s*$", &p.prevline)
            && let Some(c) = self.m(r"($word_pattern)\s*$", &p.prevrawline)
        {
            let last_word = c.s(1);
            if self.tl(
                r"^\+\s*\*\s*$last_word ",
                &[("last_word", &last_word)],
                &l.rawline,
            ) {
                let msg = cat(&[
                    b"Possible repeated word: '",
                    &last_word,
                    b"'\n",
                    &l.hereprev,
                ]);
                self.warn("REPEATED_WORD", &msg);
            }
        }
    }

    /// The checks on every hunk line, to the C-only ones.
    fn hunk_checks(&mut self, p: &mut P, l: &L) -> bool {
        // trailing whitespace
        if self.t(r"^\+.*\015", &l.line) {
            let herevet = cat(&[&l.here, b"\n", &self.cat_vet(&l.rawline), b"\n"]);
            self.error("DOS_LINE_ENDINGS", &cat(&[b"DOS line endings\n", &herevet]));
        } else if self.t(r"^\+.*\S\s+$", &l.rawline) || self.t(r"^\+\s+$", &l.rawline) {
            let herevet = cat(&[&l.here, b"\n", &self.cat_vet(&l.rawline), b"\n"]);
            self.error(
                "TRAILING_WHITESPACE",
                &cat(&[b"trailing whitespace\n", &herevet]),
            );
            self.rpt_cleaners = true;
        }

        // Check for FSF mailing addresses.
        if self.check
            && (self.tf(r"\bwrite to the Free", "i", &l.rawline)
                || self.tf(r"\b675\s+Mass\s+Ave", "i", &l.rawline)
                || self.tf(r"\b59\s+Temple\s+Pl", "i", &l.rawline)
                || self.tf(r"\b51\s+Franklin\s+St", "i", &l.rawline))
        {
            let herevet = cat(&[&l.here, b"\n", &self.cat_vet(&l.rawline), b"\n"]);
            self.chk(
                "FSF_MAILING_ADDRESS",
                &cat(&[
                    b"Do not include the paragraph about writing to the Free Software Foundation's mailing address from the sample GPL notice. The FSF has changed addresses in the past, and may do so again. Linux already includes a copy of the GPL.\n",
                    &herevet,
                ]),
            );
        }

        // check for Kconfig help text having a real description
        if self.t(r"Kconfig", &p.realfile)
            && self.t(r"^\+\s*(?:config|menuconfig|choice)\b", &l.line)
        {
            self.kconfig_help(l);
        }

        // check MAINTAINERS entries
        if self.t(r"^MAINTAINERS$", &p.realfile) {
            self.maintainers_entry(p, l);
        }

        // Check for RGMII phy-mode with delay on PCB
        if self.t(r"\.(dts|dtsi|dtso)$", &p.realfile)
            && let Some(c) = self.m(r#"^\+\s*(phy-mode|phy-connection-type)\s*=\s*""#, &l.line)
            && !self.ctx_has_comment(p.first_line, l.linenr)
        {
            let prop = c.s(1);
            let mode = self.get_quoted_string(Some(&l.line), Some(&l.rawline));
            if self.t(r#"^"rgmii(?:|-rxid|-txid)"$"#, &mode) {
                let msg = cat(&[
                    &prop,
                    b" ",
                    &mode,
                    b" without comment -- delays on the PCB should be described, otherwise use \"rgmii-id\"\n",
                    &l.herecurr,
                ]);
                self.warn("UNCOMMENTED_RGMII_MODE", &msg);
            }
        }

        // check for using SPDX license tag at beginning of files
        if p.realline == p.checklicenseline {
            self.spdx_first_line(p, l);
        }

        // check for embedded filenames
        if self.tl(
            r"^\+.*\b\Q$realfile\E\b",
            &[("realfile", &p.realfile)],
            &l.rawline,
        ) {
            let msg = cat(&[
                b"It's generally not useful to have the filename in the file\n",
                &l.herecurr,
            ]);
            self.warn("EMBEDDED_FILENAME", &msg);
        }

        // check we are in a valid source file if not then ignore this hunk
        if !self.t(r"\.(h|c|rs|s|S|sh|dtsi|dts)$", &p.realfile) {
            return false;
        }

        // check for using SPDX-License-Identifier on the wrong line number
        if p.realline != p.checklicenseline && self.t(r"\bSPDX-License-Identifier:", &l.rawline) {
            let msg = cat(&[
                b"Misplaced SPDX-License-Identifier tag - use line ",
                &n(p.checklicenseline),
                b" instead\n",
                &l.herecurr,
            ]);
            self.warn("SPDX_LICENSE_TAG", &msg);
        }

        // check for disallowed SPDX file tags
        if self.t(r"\bSPDX-.*:", &l.rawline)
            && !self.t(r"\bSPDX-License-Identifier:", &l.rawline)
            && !self.t(r"\bSPDX-FileCopyrightText:", &l.rawline)
        {
            let msg = cat(&[b"Disallowed SPDX tag\n", &l.herecurr]);
            self.warn("SPDX_LICENSE_TAG", &msg);
        }

        // line length limit (with some exclusions)
        if self.check && l.line.first() == Some(&b'+') && p.length > MAX_LINE_LENGTH {
            self.long_line(p, l);
        }

        // check for adding lines without a newline.
        if l.line.first() == Some(&b'+')
            && self
                .line_at(l.linenr)
                .is_some_and(|next| self.t(r"^\\ No newline at end of file", next))
        {
            let msg = cat(&[
                b"adding a line without newline at end of file\n",
                &l.herecurr,
            ]);
            self.warn("MISSING_EOF_NEWLINE", &msg);
        }

        // check for .L prefix local symbols in .S files
        if self.t(r"\.S$", &p.realfile)
            && self.t(
                r"^\+\s*(?:[A-Z]+_)?SYM_[A-Z]+_(?:START|END)(?:_[A-Z_]+)?\s*\(\s*\.L",
                &l.line,
            )
        {
            let msg = cat(&[
                b"Avoid using '.L' prefixed local symbol names for denoting a range of code via 'SYM_*_START/END' annotations; see Documentation/core-api/asm-annotations.rst\n",
                &l.herecurr,
            ]);
            self.warn("AVOID_L_PREFIX", &msg);
        }

        // check we are in a valid source file C or perl if not then ignore this hunk
        if !self.t(r"\.(h|c|pl|dtsi|dts)$", &p.realfile) {
            return false;
        }

        // at the beginning of a line any tabs must come first and anything
        // more than $tabsize must use tabs.
        if self.t(r"^\+\s* \t\s*\S", &l.rawline) || self.t(r"^\+\s*        \s*", &l.rawline) {
            let herevet = cat(&[&l.here, b"\n", &self.cat_vet(&l.rawline), b"\n"]);
            self.rpt_cleaners = true;
            self.error(
                "CODE_INDENT",
                &cat(&[b"code indent should use tabs where possible\n", &herevet]),
            );
        }

        // check for space before tabs.
        if l.rawline.first() == Some(&b'+') && self.t(r" \t", &l.rawline) {
            let herevet = cat(&[&l.here, b"\n", &self.cat_vet(&l.rawline), b"\n"]);
            self.warn(
                "SPACE_BEFORE_TAB",
                &cat(&[b"please, no space before tabs\n", &herevet]),
            );
        }

        // check for assignments on the start of a line
        if self.check
            && let Some(c) = self.m(r"^\+\s+($Assignment)[^=]", &l.sline)
        {
            let msg = cat(&[
                b"Assignment operator '",
                &c.s(1),
                b"' should be on the previous line\n",
                &l.hereprev,
            ]);
            self.chk("ASSIGNMENT_CONTINUATIONS", &msg);
        }

        // check for && or || at the start of a line
        if self.check && self.t(r"^\+\s*(&&|\|\|)", &l.rawline) {
            let msg = cat(&[
                b"Logical continuations should be on the previous line\n",
                &l.hereprev,
            ]);
            self.chk("LOGICAL_CONTINUATIONS", &msg);
        }

        // check indentation starts on a tab stop
        if let Some(c) = self.m(
            r"^\+\t+( +)(?:$c90_Keywords\b|\{\s*$|\}\s*(?:else\b|while\b|\s*$)|$Declare\s*$Ident\s*[;=])",
            &l.sline,
        ) {
            let indent = len(&c.s(1));
            if indent % TABSIZE != 0 {
                let msg = cat(&[b"Statements should start on a tabstop\n", &l.herecurr]);
                self.warn("TABSTOP", &msg);
            }
        }

        // check multi-line statement indentation matches previous line
        if self.check {
            self.parenthesis_alignment(p, l);
        }

        // check for space after cast like "(int) foo" or "(struct foo) bar"
        if self.check
            && let Some(c) = self.m(
                r"^\+(.*)\(\s*$Type\s*\)([ \t]++)((?![={]|\\$|$Attribute|__attribute__))",
                &l.line,
            )
            && (!c.defined(1) || !self.t(r"\b(?:sizeof|__alignof__)\s*$", &c.s(1)))
        {
            let msg = cat(&[b"No space is necessary after a cast\n", &l.herecurr]);
            self.chk("SPACING", &msg);
        }

        // Block comments use * on subsequent lines
        if self.t(r"$;[ \t]*$", &p.prevline)
            && self.t(r"^\+.*?\/\*", &p.prevrawline)
            && !self.t(r"\*\/[ \t]*$", &p.prevrawline)
            && l.rawline.first() == Some(&b'+')
            && !self.t(r"^\+[ \t]*\*", &l.rawline)
        {
            let msg = cat(&[b"Block comments use * on subsequent lines\n", &l.hereprev]);
            self.warn("BLOCK_COMMENT_STYLE", &msg);
        }

        // Block comments use */ on trailing lines
        if !self.t(r"^\+[ \t]*\*/[ \t]*$", &l.rawline)
            && !self.t(r"^\+.*/\*.*\*/[ \t]*$", &l.rawline)
            && !self.t(r"^\+.*\*{2,}/[ \t]*$", &l.rawline)
            && self.t(r"^\+[ \t]*.+\*\/[ \t]*$", &l.rawline)
        {
            let msg = cat(&[
                b"Block comments use a trailing */ on a separate line\n",
                &l.herecurr,
            ]);
            self.warn("BLOCK_COMMENT_STYLE", &msg);
        }

        // Block comment * alignment
        if self.t(r"$;[ \t]*$", &p.prevline)
            && self.t(r"^\+[ \t]*$;", &l.line)
            && self.t(r"^\+[ \t]*\*", &l.rawline)
            && ((self.t(r"^\+.*?\/\*", &p.prevrawline) && !self.t(r"\*\/[ \t]*$", &p.prevrawline))
                || self.t(r"^\+[ \t]*\*", &p.prevrawline))
        {
            let oldindent = if let Some(c) = self.m(r"^\+([ \t]*/?)\*", &p.prevrawline) {
                expand_tabs(&c.s(1))
            } else {
                expand_tabs(
                    &self
                        .m(r"^\+(.*/?)\*", &p.prevrawline)
                        .map(|c| c.s(1))
                        .unwrap_or_default(),
                )
            };
            let newindent = expand_tabs(
                &self
                    .m(r"^\+([ \t]*)\*", &l.rawline)
                    .map(|c| c.s(1))
                    .unwrap_or_default(),
            );
            if oldindent.len() != newindent.len() {
                let msg = cat(&[
                    b"Block comments should align the * on each line\n",
                    &l.hereprev,
                ]);
                self.warn("BLOCK_COMMENT_STYLE", &msg);
            }
        }

        // check for missing blank lines after struct/union declarations
        if self.check
            && self.t(r"^[\+ ]};?\s*$", &p.prevline)
            && l.line.first() == Some(&b'+')
            && !(self.t(r"^\+\s*$", &l.line)
                || self.t(
                    r"^\+\s*(?:EXPORT_SYMBOL|early_param|ALLOW_ERROR_INJECTION)",
                    &l.line,
                )
                || self.tf(r"^\+\s*MODULE_", "i", &l.line)
                || self.t(r"^\+\s*\#\s*(?:end|elif|else)", &l.line)
                || self.t(r"^\+[a-z_]*init", &l.line)
                || self.t(r"^\+\s*(?:static\s+)?[A-Z_]*ATTR", &l.line)
                || self.t(r"^\+\s*DECLARE", &l.line)
                || self.t(r"^\+\s*NOKPROBE_SYMBOL", &l.line)
                || self.t(r"^\+\s*builtin_[\w_]*driver", &l.line)
                || self.t(r"^\+\s*__setup", &l.line))
        {
            let msg = cat(&[
                b"Please use a blank line after function/struct/union/enum declarations\n",
                &l.hereprev,
            ]);
            self.chk("LINE_SPACING", &msg);
        }

        // check for multiple consecutive blank lines
        if self.t(r"^[\+ ]\s*$", &p.prevline)
            && self.t(r"^\+\s*$", &l.line)
            && p.last_blank_line != l.linenr - 1
        {
            let msg = cat(&[b"Please don't use multiple blank lines\n", &l.hereprev]);
            self.chk("LINE_SPACING", &msg);
            p.last_blank_line = l.linenr;
        }

        // check for missing blank lines after declarations
        if let Some(c) = self.m(r"\+(\s+)\S", &p.prevline)
            && self.tl(r"^\+$1\S", &[("1", &c.s(1))], &l.sline)
        {
            self.declaration_blank_line(p, l);
        }

        // check for spaces at the beginning of a line.
        if self.t(r"^\+ ", &l.rawline) && !self.t(r"^\+ *(?:$;|#|$Ident:)", &l.line) {
            let herevet = cat(&[&l.here, b"\n", &self.cat_vet(&l.rawline), b"\n"]);
            self.warn(
                "LEADING_SPACE",
                &cat(&[b"please, no spaces at the start of a line\n", &herevet]),
            );
        }

        // check we are in a valid C source file if not then ignore this hunk
        self.t(r"\.(h|c)$", &p.realfile)
    }

    fn kconfig_help(&mut self, l: &L) {
        let mut ln = l.linenr;
        let mut needs_help = false;
        let mut has_help = false;
        let mut help_length = 0;
        while let Some(f) = self.line_at(ln).map(<[u8]>::to_vec) {
            ln += 1;
            if self.t(r"^-", &f) {
                continue;
            }
            if !self.t(r"^[\+ ]", &f) {
                break;
            }
            if self.t(r#"^\+\s*(?:bool|tristate|prompt)\s*["']"#, &f) {
                needs_help = true;
                continue;
            }
            if self.t(r"^\+\s*help\s*$", &f) {
                has_help = true;
                continue;
            }
            let mut f = f;
            self.s(r"^.", "", &mut f, false, &mut |_| Vec::new());
            self.s(r"#.*", "", &mut f, false, &mut |_| Vec::new());
            self.s(r"^\s+", "", &mut f, false, &mut |_| Vec::new());
            if self.t(r"^$", &f) {
                continue;
            }
            if self.tf(
                "^(?:config|menuconfig|choice|endchoice|\n\t\t\t\t\t       if|endif|menu|endmenu|source)\\b",
                "x",
                &f,
            ) {
                break;
            }
            if has_help {
                help_length += 1;
            }
        }
        if needs_help && help_length < MIN_CONF_DESC_LENGTH {
            let stat_real = self.get_stat_real(l.linenr, ln - 1);
            let msg = cat(&[
                b"please write a help paragraph that fully describes the config symbol with at least ",
                &n(MIN_CONF_DESC_LENGTH),
                b" lines\n",
                &l.here,
                b"\n",
                &stat_real,
                b"\n",
            ]);
            self.warn("CONFIG_DESCRIPTION", &msg);
        }
    }

    fn maintainers_entry(&mut self, p: &P, l: &L) {
        if self.t(r"^\+[A-Z]:", &l.rawline) && !self.t(r"^\+[A-Z]:\t\S", &l.rawline) {
            let msg = cat(&[
                b"MAINTAINERS entries use one tab after TYPE:\n",
                &l.herecurr,
            ]);
            self.warn("MAINTAINERS_STYLE", &msg);
        }
        let preferred_order = b"MRLSWQBCPTFXNK";
        if self.t(r"^\+[A-Z]:", &l.rawline) && self.t(r"^[\+ ][A-Z]:", &p.prevrawline) {
            let c = self.m(r"^\+([A-Z]):\s*(.*)", &l.rawline).expect("an entry");
            let (cur, curval) = (c.s(1), c.s(2));
            let c = self
                .m(r"^[\+ ]([A-Z]):\s*(.*)", &p.prevrawline)
                .expect("an entry");
            let (prev, prevval) = (c.s(1), c.s(2));
            let curindex = preferred_order.iter().position(|&b| b == cur[0]);
            let previndex = preferred_order.iter().position(|&b| b == prev[0]);
            match curindex {
                None => {
                    let msg = cat(&[
                        b"Unknown MAINTAINERS entry type: '",
                        &cur,
                        b"'\n",
                        &l.herecurr,
                    ]);
                    self.warn("MAINTAINERS_STYLE", &msg);
                }
                Some(curindex) => {
                    if previndex.is_some_and(|previndex| curindex < previndex) {
                        let msg = cat(&[
                            b"Misordered MAINTAINERS entry - list '",
                            &cur,
                            b":' before '",
                            &prev,
                            b":'\n",
                            &l.hereprev,
                        ]);
                        self.warn("MAINTAINERS_STYLE", &msg);
                    } else if ((prev == b"F" && cur == b"F") || (prev == b"X" && cur == b"X"))
                        && prevval > curval
                    {
                        let msg = cat(&[
                            b"Misordered MAINTAINERS entry - list file patterns in alphabetic order\n",
                            &l.hereprev,
                        ]);
                        self.warn("MAINTAINERS_STYLE", &msg);
                    }
                }
            }
        }
    }

    fn spdx_first_line(&mut self, p: &mut P, l: &L) {
        if self.t(r"^[ \+]\s*\#\!\s*\/", &l.rawline) {
            p.checklicenseline = 2;
            return;
        }
        if l.rawline.first() != Some(&b'+') {
            return;
        }
        let mut comment: &[u8] = b"";
        if self.t(r"\.(c|rs|dts|dtsi)$", &p.realfile) {
            comment = b"//";
        } else if p.checklicenseline == 2 || self.t(r"\.(sh|pl|py|awk|tc|yaml)$", &p.realfile) {
            comment = b"#";
        } else if self.t(r"\.rst$", &p.realfile) {
            comment = b"..";
        }
        let mut pattern = qr(&quotemeta_bytes(comment), "");
        if self.t(r"\.(h|s|S)$", &p.realfile) {
            comment = b"/*";
            pattern = qr(br"/\*", "");
        }
        if self.t(r"\.[chsS]$", &p.realfile)
            && self.t(r"SPDX-License-Identifier:", &l.rawline)
            && !self.tl(r"^\+\s*$pattern\s*", &[("pattern", &pattern)], &l.rawline)
        {
            let msg = cat(&[
                b"Improper SPDX comment style for '",
                &p.realfile,
                b"', please use '",
                comment,
                b"' instead\n",
                &l.herecurr,
            ]);
            self.warn("SPDX_LICENSE_TAG", &msg);
        }
        if !comment.is_empty()
            && !self.tl(
                r"^\+$pattern SPDX-License-Identifier: ",
                &[("pattern", &pattern)],
                &l.rawline,
            )
        {
            let msg = cat(&[
                b"Missing or malformed SPDX-License-Identifier tag in line ",
                &n(p.checklicenseline),
                b"\n",
                &l.herecurr,
            ]);
            self.warn("SPDX_LICENSE_TAG", &msg);
        } else if let Some(c) = self.m(r"(SPDX-License-Identifier: .*)", &l.rawline) {
            let spdx_license = c.s(1);
            if self.t(r"^Documentation/devicetree/bindings/", &p.realfile)
                && !self.t(r"GPL-2\.0(?:-only)? OR BSD-2-Clause", &spdx_license)
            {
                let msg = cat(&[
                    b"DT binding documents should be licensed (GPL-2.0-only OR BSD-2-Clause)\n",
                    &l.herecurr,
                ]);
                self.chk("SPDX_LICENSE_TAG", &msg);
            }
            if self.t(r"^include/dt-bindings/", &p.realfile)
                && !self.t(r"GPL-2\.0(?:-only)? OR \S+", &spdx_license)
            {
                let msg = cat(&[
                    b"DT binding headers should be licensed (GPL-2.0-only OR .*)\n",
                    &l.herecurr,
                ]);
                self.warn("SPDX_LICENSE_TAG", &msg);
            }
        }
    }

    fn long_line(&mut self, p: &P, l: &L) {
        let mut msg_type: &str = "LONG_LINE";
        let rest_fits = |tail: &[u8]| {
            len(&expand_tabs(
                substr(&l.line, 1, Some(len(&l.line) - len(tail) - 1)).unwrap_or(&[]),
            )) <= MAX_LINE_LENGTH
        };
        if let Some(c) = self.m(
            r#"^\+\s*$logFunctions\s*\(\s*(?:(?:KERN_\S+\s*|[^"]*))?($String\s*(?:|,|\)\s*;)\s*)$"#,
            &l.line,
        ) && rest_fits(&c.s(1))
        {
            msg_type = "";
        } else if self.t(r"^\+\s*$String\s*(?:\s*|,|\)\s*;)\s*$", &l.line)
            || self.t(r"^\+\s*#\s*define\s+\w+\s+$String$", &l.line)
        {
            msg_type = "";
        } else if self.t(r"^\+.*\bEFI_GUID\s*\(", &l.line)
            || self.t(r"^\+\s*(?:\w+)?\s*DEFINE_PER_CPU", &l.line)
        {
            msg_type = "";
        } else if self
            .mf(r"^\+.*\b[a-z][\w\.\+\-]*:\/\/\S+", "i", &l.rawline)
            .is_some()
        {
            msg_type = "";
        } else if let Some(c) = self.m(r"($;[\s$;]*)$", &l.line)
            && rest_fits(&c.s(1))
        {
            msg_type = "LONG_LINE_COMMENT";
        } else if let Some(c) = self.m(r"\s*($String(?:\s*(?:\\|,\s*|\)\s*;\s*))?)$", &l.sline)
            && rest_fits(&c.s(1))
        {
            msg_type = "LONG_LINE_STRING";
        }
        if !msg_type.is_empty() {
            let msg = cat(&[
                b"line length of ",
                &n(p.length),
                b" exceeds ",
                &n(MAX_LINE_LENGTH),
                b" columns\n",
                &l.herecurr,
            ]);
            self.chk(msg_type, &msg);
        }
    }

    fn parenthesis_alignment(&mut self, p: &P, l: &L) {
        if !self.t(
            r"^\+([ \t]*)((?:$c90_Keywords(?:\s+if)\s*)|(?:$Declare\s*)?(?:$Ident|\(\s*\*\s*$Ident\s*\))\s*|(?:\*\s*)*$Lval\s*=\s*$Ident\s*)\(.*(\&\&|\|\||,)\s*$",
            &p.prevline,
        ) {
            return;
        }
        let Some(c) = self.m(r"^\+(\t*)(.*)$", &p.prevline) else {
            return;
        };
        let (oldindent, rest) = (c.s(1), c.s(2));
        let pos = self.pos_last_openparen(&rest);
        if pos >= 0 {
            let newindent = self
                .m(r"^(\+| )([ \t]*)", &l.line)
                .map(|c| c.s(2))
                .unwrap_or_default();
            let mut goodtabindent = oldindent.clone();
            goodtabindent.extend(std::iter::repeat_n(
                b'\t',
                usize::try_from(pos / TABSIZE).expect("tabs"),
            ));
            goodtabindent.extend(std::iter::repeat_n(
                b' ',
                usize::try_from(pos % TABSIZE).expect("spaces"),
            ));
            let mut goodspaceindent = oldindent;
            goodspaceindent.extend(std::iter::repeat_n(
                b' ',
                usize::try_from(pos).expect("spaces"),
            ));
            if newindent != goodtabindent && newindent != goodspaceindent {
                let msg = cat(&[b"Alignment should match open parenthesis\n", &l.hereprev]);
                self.chk("PARENTHESIS_ALIGNMENT", &msg);
            }
        }
    }

    fn declaration_blank_line(&mut self, p: &P, l: &L) {
        let mut sl = l.sline.clone();
        let mut pl = p.prevline.clone();
        self.s(
            r"\b(?:$Attribute|$Sparse)\b",
            "",
            &mut sl,
            true,
            &mut |_| Vec::new(),
        );
        self.s(
            r"\b(?:$Attribute|$Sparse)\b",
            "",
            &mut pl,
            true,
            &mut |_| Vec::new(),
        );
        let looks_declaration = |this: &Self, s: &[u8]| {
            this.t(r"^\+\s+$Declare\s*$Ident\s*[=,;:\[]", s)
                || this.t(
                    r"^\+\s+$Declare\s*\(\s*\*\s*$Ident(?:\s*\[\s*(?:$Ident|$Constant)?\s*\])?\s*\)\s*[=,;:\[\(]",
                    s,
                )
                || this.t(r"^\+\s+$Ident(?:\s+|\s*\*\s*)$Ident\s*[=,;\[]", s)
                || this.t(r"^\+\s+$declaration_macros", s)
        };
        if looks_declaration(self, &pl)
            && !(self.t(r"^\+\s+$c90_Keywords\b", &pl)
                || self.t(r"(?:$Compare|$Assignment|$Operators)\s*$", &pl)
                || self.t(r"(?:\{\s*|\\)$", &pl))
            && !(looks_declaration(self, &sl)
                || self.t(
                    r"^\+\s+(?:static\s+)?(?:const\s+)?(?:union|struct|enum|typedef)\b",
                    &sl,
                )
                || self.t(r#"^\+\s+(?:$|[\{\}\.\#\"\?\:\(\[])"#, &sl)
                || self.t(r"^\+\s+$Ident\s*:\s*\d+\s*[,;]", &sl)
                || self.t(r"^\+\s+\(?\s*(?:$Compare|$Assignment|$Operators)", &sl))
        {
            let msg = cat(&[b"Missing a blank line after declarations\n", &l.hereprev]);
            self.warn("LINE_SPACING", &msg);
        }
    }

    /// The C-only checks up to `next if ($line =~ /^[^\+]/)`.
    fn c_checks(&mut self, p: &mut P, l: &mut L) -> bool {
        // check for unusual line ending [ or (
        if self.check
            && let Some(c) = self.m(r"^\+.*([\[\(])\s*$", &l.line)
        {
            let msg = cat(&[
                b"Lines should not end with a '",
                &c.s(1),
                b"'\n",
                &l.herecurr,
            ]);
            self.chk("OPEN_ENDED_LINE", &msg);
        }

        // check if this appears to be the start function declaration, save the name
        if self.t(r"^\+\{\s*$", &l.sline)
            && let Some(c) = self.m(
                r"^\+(?:(?:(?:$Storage|$Inline)\s*)*\s*$Type\s*)?($Ident)\(",
                &p.prevline,
            )
        {
            p.context_function = Some(c.s(1));
        }

        // check if this appears to be the end of function declaration
        if self.t(r"^\+\}\s*$", &l.sline) {
            p.context_function = None;
        }

        // check indentation of any line with a bare else
        if let Some(c) = self.m(r"^\+([\t]+)(?:}[ \t]*)?else(?:[ \t]*{)?\s*$", &l.sline) {
            let tabs = n(len(&c.s(1)) + 1);
            let tabs = [("tabs", &tabs[..])];
            if self.tl(r"^\+\t{$tabs,$tabs}break\b", &tabs, &p.prevline)
                || (self.tl(r"^\+\t{$tabs,$tabs}return\b", &tabs, &p.prevline)
                    && self
                        .line_at(l.linenr)
                        .is_some_and(|next| !self.tl(r"^[ \+]\t{$tabs,$tabs}return", &tabs, next)))
            {
                let msg = cat(&[
                    b"else is not generally useful after a break or return\n",
                    &l.hereprev,
                ]);
                self.warn("UNNECESSARY_ELSE", &msg);
            }
        }

        // check indentation of a line with a break;
        if let Some(c) = self.m(r"^\+([\t]+)break\s*;\s*$", &l.sline) {
            let tabs = c.s(1);
            if let Some(c) = self.ml(
                r"^\+$tabs(goto|return|break)\b",
                "",
                &[("tabs", &tabs)],
                &p.prevline,
            ) {
                let msg = cat(&[b"break is not useful after a ", &c.s(1), b"\n", &l.hereprev]);
                self.warn("UNNECESSARY_BREAK", &msg);
            }
        }

        // check for RCS/CVS revision markers
        if self.t(r"^\+.*\$(Revision|Log|Id)(?:\$|)", &l.rawline) {
            let msg = cat(&[
                b"CVS style keyword markers, these will _not_ be updated\n",
                &l.herecurr,
            ]);
            self.warn("CVS_KEYWORD", &msg);
        }

        // check for old HOTPLUG __dev<foo> section markings
        if let Some(c) = self.m(r"\b(__dev(init|exit)(data|const|))\b", &l.line) {
            let msg = cat(&[b"Using ", &c.s(1), b" is unnecessary\n", &l.herecurr]);
            self.warn("HOTPLUG_SECTION", &msg);
        }

        // Check for potential 'bare' types
        if l.linenr > p.suppress_statement && p.realcnt != 0 && self.t(r".\s*\S", &l.sline) {
            self.bare_types(p, l);
        }

        // Check for switch () and associated case and default
        // statements should be at the same indent.
        if self.t(r"\bswitch\s*\(.*\)", &l.line) {
            let mut err: Vec<u8> = Vec::new();
            let mut sep: &[u8] = b"";
            let (_, ctx) = self.ctx_block_get(l.linenr, p.realcnt, true, b'{', b'}', 0);
            for c in ctx.iter().skip(1) {
                let (_, cindent) = self.line_stats(c);
                if self.t(r"^\+\s*(case\s+|default:)", c) && p.indent != cindent {
                    err.extend(cat(&[sep, c, b"\n"]));
                    sep = b"";
                } else {
                    sep = b"[...]\n";
                }
            }
            if !err.is_empty() {
                let msg = cat(&[
                    b"switch and case should be at the same indent\n",
                    &l.hereline,
                    &err,
                ]);
                self.error("SWITCH_CASE_INDENT_LEVEL", &msg);
            }
        }

        // if/while/etc brace do not go on next line, unless defining a do while loop,
        // or if that brace on the next line is for something else
        if let Some(c) = self.m(
            r"(.*)\b((?:if|while|for|switch|(?:[a-z_]+|)for_each[a-z_]+)\s*\(|do\b|else\b)",
            &l.line,
        ) && !self.t(r"^.\s*\#", &l.line)
        {
            let pre_ctx = cat(&[&c.s(1), &c.s(2)]);
            self.brace_next_line(p, l, &pre_ctx);
        }

        // Check relative indent for conditionals and blocks.
        if self.t(
            r"\b(?:(?:if|while|for|(?:[a-z_]+|)for_each[a-z_]+)\s*\(|(?:do|else)\b)",
            &l.line,
        ) && !self.t(r"^.\s*#", &l.line)
            && !self.t(r"\}\s*while\s*", &l.line)
        {
            self.suspect_code_indent(p, l);
        }

        // Track the 'values' across context and added lines.
        let mut opline = l.line.clone();
        self.s(r"^.", "", &mut opline, false, &mut |_| b" ".to_vec());
        let (curr_values, curr_vars) = self.annotate_values(&cat(&[&opline, b"\n"]), p.prev_values);
        let mut values = vec![p.prev_values];
        values.extend(curr_values);
        p.prev_values = *values.last().expect("a value");
        l.opline = opline;
        l.curr_values = values;
        l.curr_vars = curr_vars;

        // ignore lines not being added
        !self.t(r"^[^\+]", &l.line)
    }

    fn statement_block(&self, l: &mut L, realcnt: i64) {
        if l.stat.is_none() {
            let b = self.ctx_statement_block(l.linenr, realcnt, 0);
            l.stat = Some(b.statement);
            l.cond = Some(b.condition);
            l.line_nr_next = Some(b.line);
            l.remain_next = Some(b.remain);
            l.off_next = Some(b.off);
        }
    }

    fn bare_types(&mut self, p: &mut P, l: &mut L) {
        let b = self.ctx_statement_block(l.linenr, p.realcnt, 0);
        let mut stat = b.statement;
        let mut cond = b.condition;
        self.s(r"\n.", "", &mut stat, true, &mut |_| b"\n ".to_vec());
        self.s(r"\n.", "", &mut cond, true, &mut |_| b"\n ".to_vec());
        l.line_nr_next = Some(b.line);
        l.remain_next = Some(b.remain);
        l.off_next = Some(b.off);

        // If this statement has no statement boundaries within
        // it there is no point in retrying a statement scan
        // until we hit end of it.
        let mut frag = stat.clone();
        self.s(r";+\s*$", "", &mut frag, false, &mut |_| Vec::new());
        if !self.t(r"(?:{|;)", &frag) {
            p.suppress_statement = b.line;
        }

        // Find the real next line.
        let mut realline_next = b.line;
        if self
            .line_at(realline_next - 1)
            .is_none_or(|next| self.t(r"^\s*$", substr(next, b.off, None).unwrap_or(&[])))
        {
            realline_next += 1;
        }
        l.realline_next = Some(realline_next);

        let mut s = stat.clone();
        self.s(r"{.*$", "s", &mut s, false, &mut |_| Vec::new());
        l.stat = Some(stat);
        l.cond = Some(cond);

        if self.tf(r"$Ident:\*$", "s", &s) || self.tf(r"^.\s*$Ident\s*\(", "s", &s) || self.tf(r"^.\s*else\b", "s", &s) {
            // Ignore goto labels, functions being called and else.
        } else if let Some(c) = self
            .mf(
                r"^.\s*(?:$Storage\s+)?(?:$Inline\s+)?(?:const\s+)?((?:\s*$Ident)+?)\b(?:\s+$Sparse)?\s*\**\s*(?:$Ident|\(\*[^\)]*\))(?:\s*$Modifier)?\s*(?:;|=|,|\()",
                "s",
                &s,
            )
            .filter(|_| p.prev_values == b'E')
        {
            let mut ty = c.s(1);
            self.s(r"\s+", "", &mut ty, true, &mut |_| b" ".to_vec());
            self.possible(&ty);
        } else if let Some(c) = self.mf(r"^.(?:$Storage\s+)?(?:$Inline\s+)?(?:const\s+)?($Ident)\b\s*(?!:)", "s", &s) {
            self.possible(&c.s(1));
        }

        // any (foo ... *) is a pointer cast, and foo is a type
        for c in self.all(r"\(($Ident)(?:\s+$Sparse)*[\s\*]+\s*\)", "s", &s) {
            self.possible(&c.s(1));
        }

        // Check for any sort of function declaration.
        if p.prev_values == b'E'
            && let Some(c) = self.mf(
                r"^(.(?:typedef\s*)?(?:(?:$Storage|$Inline)\s*)*\s*$Type\s*(?:\b$Ident|\(\*\s*$Ident\))\s*)\(",
                "s",
                &s,
            )
        {
            let name_len = c.s(1).len();
            let mut ctx = s.get(name_len + 1..).unwrap_or(&[]).to_vec();
            self.s(r"\)[^\)]*$", "", &mut ctx, false, &mut |_| Vec::new());
            for arg in split(&self.re(r"\s*,\s*", ""), &ctx) {
                if let Some(c) = self
                    .mf(r"^(?:const\s+)?($Ident)(?:\s+$Sparse)*\s*\**\s*(:?\b$Ident)?$", "s", &arg)
                    .or_else(|| self.mf(r"^($Ident)$", "s", &arg))
                {
                    self.possible(&c.s(1));
                }
            }
        }
    }

    fn brace_next_line(&mut self, p: &P, l: &L, pre_ctx: &[u8]) {
        let (level, ctx) = self.ctx_block_get(l.linenr, p.realcnt, false, b'(', b')', 0);
        if self.t(r"^\+\t{6,}", &l.line) {
            let msg = cat(&[
                b"Too many leading tabs - consider code refactoring\n",
                &l.herecurr,
            ]);
            self.warn("DEEP_INDENTATION", &msg);
        }
        let ctx_cnt = p.realcnt - len_of(&ctx);
        let ctx = ctx.join(&b'\n');
        let mut ctx_ln = l.linenr;
        let mut ctx_skip = p.realcnt;
        while ctx_skip > ctx_cnt
            || (ctx_skip == ctx_cnt && self.line_at(ctx_ln - 1).is_some_and(|x| self.t(r"^-", x)))
        {
            if self.line_at(ctx_ln - 1).is_none_or(|x| !self.t(r"^-", x)) {
                ctx_skip -= 1;
            }
            ctx_ln += 1;
        }
        let next_raw = self.rawline_at(ctx_ln - 1).unwrap_or(&[]).to_vec();
        if !self.t(r"{\s*", &ctx)
            && self
                .line_at(ctx_ln - 1)
                .is_some_and(|x| self.t(r"^\+\s*{", x))
        {
            let msg = cat(&[
                b"that open brace { should be on the previous line\n",
                &l.here,
                b"\n",
                &ctx,
                b"\n",
                &next_raw,
                b"\n",
            ]);
            self.error("OPEN_BRACE", &msg);
        }
        if level == 0
            && !self.t(r"}\s*while\s*\($", pre_ctx)
            && self.t(r"\)\s*\;\s*$", &ctx)
            && let Some(next) = self.line_at(ctx_ln - 1).map(<[u8]>::to_vec)
        {
            let (_, nindent) = self.line_stats(&next);
            if nindent > p.indent {
                let msg = cat(&[
                    b"trailing semicolon indicates no statements, indent implies otherwise\n",
                    &l.here,
                    b"\n",
                    &ctx,
                    b"\n",
                    &next_raw,
                    b"\n",
                ]);
                self.warn("TRAILING_SEMICOLON", &msg);
            }
        }
    }

    fn suspect_code_indent(&mut self, p: &P, l: &mut L) {
        self.statement_block(l, p.realcnt);
        let stat = l.stat.clone().expect("a statement");
        let mut s = stat.clone();
        let mut c = l.cond.clone().expect("a condition");
        s.drain(..c.len().min(s.len()));

        // remove inline comments
        for b in s.iter_mut().chain(c.iter_mut()) {
            if *b == SUBSEP {
                *b = b' ';
            }
        }

        // Find out how long the conditional actually is.
        let mut cond_lines = count(&c, b'\n');

        // Make sure we remove the line prefixes as we have
        // none on the first line, and are going to readd them
        // where necessary.
        self.s(r"\n.", "s", &mut s, true, &mut |_| b"\n".to_vec());
        while self.t(r"\n\s+\\\n", &s) {
            cond_lines +=
                i64::try_from(self.s(r"\n\s+\\\n", "", &mut s, true, &mut |_| b"\n".to_vec()))
                    .expect("count");
        }

        // We want to check the first line inside the block
        // starting at the end of the conditional, so remove:
        //  1) any blank line termination
        //  2) any opening brace { on end of the line
        //  3) any do (...) {
        let mut continuation = false;
        let mut check = false;
        self.s(r"^.*\bdo\b", "", &mut s, false, &mut |_| Vec::new());
        self.s(r"^\s*{", "", &mut s, false, &mut |_| Vec::new());
        if self.s(r"^\s*\\", "", &mut s, false, &mut |_| Vec::new()) > 0 {
            continuation = true;
        }
        if self.s(r"^\s*?\n", "", &mut s, false, &mut |_| Vec::new()) > 0 {
            check = true;
            cond_lines += 1;
        }

        // Also ignore a loop construct at the end of a
        // preprocessor statement.
        if (self.t(r"^.\s*#\s*define\s", &p.prevline) || self.t(r"\\\s*$", &p.prevline))
            && !continuation
        {
            check = false;
        }

        let mut cond_ptr = -1;
        continuation = false;
        while cond_ptr != cond_lines {
            cond_ptr = cond_lines;

            // If we see an #else/#elif then the code
            // is not linear.
            if self.t(r"^\s*\#\s*(?:else|elif)", &s) {
                check = false;
            }

            // Ignore:
            //  1) blank lines, they should be at 0,
            //  2) preprocessor lines, and
            //  3) labels.
            if continuation
                || self.t(r"^\s*?\n", &s)
                || self.t(r"^\s*#\s*?", &s)
                || self.t(r"^\s*$Ident\s*:", &s)
            {
                continuation = self.t(r"^.*?\\\n", &s);
                if self.s(r"^.*?\n", "", &mut s, false, &mut |_| Vec::new()) > 0 {
                    cond_lines += 1;
                }
            }
        }

        let (_, sindent) = self.line_stats(&cat(&[b"+", &s]));
        let mut stat_real = self.raw_line(l.linenr, cond_lines);

        // Check if either of these lines are modified, else
        // this is not this patch's fault.
        if stat_real.is_none()
            || (!self.t(r"^\+", &stat) && !stat_real.as_deref().is_some_and(|r| self.t(r"^\+", r)))
        {
            check = false;
        }
        if let Some(r) = &stat_real
            && cond_lines > 1
        {
            stat_real = Some(cat(&[b"[...]\n", r]));
        }

        if check
            && !s.is_empty()
            && (sindent % TABSIZE != 0
                || sindent < p.indent
                || (sindent == p.indent && !self.t(r"^\s*(?:\}|\{|else\b)", &s))
                || sindent > p.indent + TABSIZE)
        {
            let msg = cat(&[
                b"suspect code indent for conditional statements (",
                &n(p.indent),
                b", ",
                &n(sindent),
                b")\n",
                &l.herecurr,
                &stat_real.unwrap_or_default(),
                b"\n",
            ]);
            self.warn("SUSPECT_CODE_INDENT", &msg);
        }
    }

    /// From the self-assignment check to `C99_COMMENTS`, and the
    /// declarations anchored on the added line.
    fn added_line_checks(&mut self, p: &mut P, l: &mut L) {
        // check for self assignments used to avoid compiler warnings
        if let Some(c) = self.m(r"^\+\s*(?:$Declare)?([A-Za-z_][A-Za-z\d_]*)\s*=", &l.line) {
            let var = c.s(1);
            if self.tl(
                r"^\+\s*(?:$Declare)?$var\s*=\s*(?:$var|\*\s*\(?\s*&\s*\(?\s*$var\s*\)?\s*\)?)\s*[;,]",
                &[("var", &var)],
                &l.line,
            ) {
                let msg = cat(&[b"Do not use self-assignments to avoid compiler warnings\n", &l.herecurr]);
                self.warn("SELF_ASSIGNMENT", &msg);
            }
        }

        // check for dereferences that span multiple lines
        if self.t(r"^\+.*$Lval\s*(?:\.|->)\s*$", &p.prevline)
            && self.t(r"^\+\s*(?!\#\s*(?!define\s+|if))\s*$Lval", &l.line)
        {
            let mut r = self
                .m(r"($Lval\s*(?:\.|->))\s*$", &p.prevline)
                .map(|c| c.s(1))
                .unwrap_or_default();
            let second = self
                .m(r"^.\s*($Lval)", &l.line)
                .map(|c| c.s(1))
                .unwrap_or_default();
            r.extend(second);
            r.retain(|b| !super::check::perl_space(*b));
            let msg = cat(&[
                b"Avoid multiple line dereference - prefer '",
                &r,
                b"'\n",
                &l.hereprev,
            ]);
            self.warn("MULTILINE_DEREFERENCE", &msg);
        }

        // check for declarations of signed or unsigned without int
        for c in self.all(
            r"\b($Declare)\s*(?!char\b|short\b|int\b|long\b)\s*($Ident)?\s*[=,;\[\)\(]",
            "",
            &l.line,
        ) {
            let ty = c.s(1);
            if let Some(m) = self.m(
                r"^(?:(?:$Storage|$Inline|$Attribute)\s+)*((?:un)?signed)((?:\s*\*)*)\s*$",
                &ty,
            ) {
                let sign = m.s(1);
                let pointer = m.s(2);
                let msg = cat(&[
                    b"Prefer '",
                    &self.trim(&sign),
                    b" int",
                    &self.rtrim(&pointer),
                    b"' to bare use of '",
                    &sign,
                    &self.rtrim(&pointer),
                    b"'\n",
                    &l.herecurr,
                ]);
                self.warn("UNSPECIFIED_INT", &msg);
            }
        }

        // check for initialisation to aggregates open brace on the next line
        if self.t(r"^.\s*{", &l.line) && self.t(r"(?:^|[^=])=\s*$", &p.prevline) {
            let msg = cat(&[
                b"that open brace { should be on the previous line\n",
                &l.hereprev,
            ]);
            self.error("OPEN_BRACE", &msg);
        }

        // check for malformed paths in #include statements (uses RAW line)
        if let Some(c) = self.m(r#"^.\s*\#\s*include\s+[<"](.*)[">]"#, &l.rawline) {
            let path = c.s(1);
            if self.t(r"//", &path) {
                let msg = cat(&[b"malformed #include filename\n", &l.herecurr]);
                self.error("MALFORMED_INCLUDE", &msg);
            }
            if self.t(r"^uapi/", &path) && self.t(r"\binclude/uapi/", &p.realfile) {
                let msg = cat(&[
                    b"No #include in ...include/uapi/... should use a uapi/ path prefix\n",
                    &l.herecurr,
                ]);
                self.error("UAPI_INCLUDE", &msg);
            }
        }

        // no C99 // comments
        if self.t(r"//", &l.line) {
            let msg = cat(&[b"do not use C99 // comments\n", &l.herecurr]);
            self.error("C99_COMMENTS", &msg);
        }
        // Remove C99 comments.
        if self.s(r"//.*", "", &mut l.line, false, &mut |_| Vec::new()) > 0 {
            self.lines[usize::try_from(l.linenr - 1).expect("line")].clone_from(&l.line);
        }
        self.s(r"//.*", "", &mut l.opline, false, &mut |_| Vec::new());

        // EXPORT_SYMBOL should immediately follow the thing it is exporting, consider
        // the whole statement.
        self.export_symbol(p, l);

        // check for global initialisers.
        if let Some(c) = self.m(
            r"^\+$Type\s*$Ident(?:\s+$Modifier)*\s*=\s*($zero_initializer)\s*;",
            &l.line,
        ) && !(self.t(r"^tools/testing/selftests/bpf/progs/.*\.c$", &p.realfile)
            || self.t(r"^samples/bpf/.*_kern\.c$", &p.realfile)
            || self.t(r"/bpf/.*\.bpf\.c$", &p.realfile))
        {
            let msg = cat(&[
                b"do not initialise globals to ",
                &c.s(1),
                b"\n",
                &l.herecurr,
            ]);
            self.error("GLOBAL_INITIALISERS", &msg);
        }
        // check for static initialisers.
        if let Some(c) = self.m(r"^\+.*\bstatic\s.*=\s*($zero_initializer)\s*;", &l.line) {
            let msg = cat(&[
                b"do not initialise statics to ",
                &c.s(1),
                b"\n",
                &l.herecurr,
            ]);
            self.error("INITIALISED_STATIC", &msg);
        }

        // check for misordered declarations of char/short/int/long with signed/unsigned
        for c in self.all(r"(\b$TypeMisordered\b)", "", &l.sline) {
            let tmp = self.trim(&c.s(1));
            let msg = cat(&[
                b"type '",
                &tmp,
                b"' should be specified in [[un]signed] [short|int|long|long long] order\n",
                &l.herecurr,
            ]);
            self.warn("MISORDERED_TYPE", &msg);
        }

        // check for unnecessary <signed> int declarations of short/long/long long
        for c in self.all(
            r"\b($TypeMisordered(\s*\*)*|$C90_int_types)\b",
            "",
            &l.sline,
        ) {
            let ty = self.trim(&c.s(1));
            if !self.t(r"\bint\b", &ty) {
                continue;
            }
            if !self.t(r"\b(?:short|long\s+long|long)\b", &ty) {
                continue;
            }
            let mut new_type = ty.clone();
            self.s(r"\b\s*int\s*\b", "", &mut new_type, false, &mut |_| {
                b" ".to_vec()
            });
            self.s(
                r"\b\s*(?:un)?signed\b\s*",
                "",
                &mut new_type,
                false,
                &mut |_| b" ".to_vec(),
            );
            self.s(r"^const\s+", "", &mut new_type, false, &mut |_| Vec::new());
            if self.t(r"\bunsigned\b", &ty) {
                new_type = cat(&[b"unsigned ", &new_type]);
            }
            if self.t(r"^const\b", &ty) {
                new_type = cat(&[b"const ", &new_type]);
            }
            self.s(r"\s+", "", &mut new_type, true, &mut |_| b" ".to_vec());
            let new_type = self.trim(&new_type);
            let msg = cat(&[
                b"Prefer '",
                &new_type,
                b"' over '",
                &ty,
                b"' as the int is unnecessary\n",
                &l.herecurr,
            ]);
            self.warn("UNNECESSARY_INT", &msg);
        }

        // check for static const char * arrays.
        if self.t(
            r"\bstatic\s+const\s+char\s*\*\s*(\w+)\s*\[\s*\]\s*=\s*",
            &l.line,
        ) {
            let msg = cat(&[
                b"static const char * array should probably be static const char * const\n",
                &l.herecurr,
            ]);
            self.warn("STATIC_CONST_CHAR_ARRAY", &msg);
        }

        // check for initialized const char arrays that should be static const
        if self.t(
            r#"^\+\s*const\s+(char|unsigned\s+char|_*u8|(?:[us]_)?int8_t)\s+\w+\s*\[\s*(?:\w+\s*)?\]\s*=\s*""#,
            &l.line,
        ) {
            let msg = cat(&[b"const array should probably be static const\n", &l.herecurr]);
            self.warn("STATIC_CONST_CHAR_ARRAY", &msg);
        }

        // check for static char foo[] = "bar" declarations.
        if self.t(r#"\bstatic\s+char\s+(\w+)\s*\[\s*\]\s*=\s*""#, &l.line) {
            let msg = cat(&[
                b"static char array declaration should probably be static const char\n",
                &l.herecurr,
            ]);
            self.warn("STATIC_CONST_CHAR_ARRAY", &msg);
        }

        // check for const <foo> const where <foo> is not a pointer or array type
        if let Some(c) = self.m(r"\bconst\s+($BasicType)\s+const\b", &l.sline) {
            let found = c.s(1);
            let f = [("found", &found[..])];
            if self.tl(r"\bconst\s+\Q$found\E\s+const\b\s*\*", &f, &l.sline) {
                let msg = cat(&[
                    b"'const ",
                    &found,
                    b" const *' should probably be 'const ",
                    &found,
                    b" * const'\n",
                    &l.herecurr,
                ]);
                self.warn("CONST_CONST", &msg);
            } else if !self.tl(r"\bconst\s+\Q$found\E\s+const\s+\w+\s*\[", &f, &l.sline) {
                let msg = cat(&[
                    b"'const ",
                    &found,
                    b" const' should probably be 'const ",
                    &found,
                    b"'\n",
                    &l.herecurr,
                ]);
                self.warn("CONST_CONST", &msg);
            }
        }

        // check for const static or static <non ptr type> const declarations
        if let Some(c) = self
            .m(r"^\+\s*const\s+static\s+($Type)\b", &l.sline)
            .or_else(|| self.m(r"^\+\s*static\s+($BasicType)\s+const\b", &l.sline))
        {
            let msg = cat(&[
                b"Move const after static - use 'static const ",
                &c.s(1),
                b"'\n",
                &l.herecurr,
            ]);
            self.warn("STATIC_CONST", &msg);
        }

        // check for non-global char *foo[] = {"bar", ...} declarations.
        if self.t(
            r"^.\s+(?:static\s+|const\s+)?char\s+\*\s*\w+\s*\[\s*\]\s*=\s*\{",
            &l.line,
        ) {
            let msg = cat(&[
                b"char * array declaration might be better as static const\n",
                &l.herecurr,
            ]);
            self.warn("STATIC_CONST_CHAR_ARRAY", &msg);
        }

        // check for sizeof(foo)/sizeof(foo[0]) that could be ARRAY_SIZE(foo)
        if let Some(c) = self.m(r"\bsizeof\s*\(\s*($Lval)\s*\)", &l.line) {
            let array = c.s(1);
            if self.tl(
                r"\b(sizeof\s*\(\s*\Q$array\E\s*\)\s*/\s*sizeof\s*\(\s*\Q$array\E\s*\[\s*0\s*\]\s*\))",
                &[("array", &array)],
                &l.line,
            ) {
                let msg = cat(&[b"Prefer ARRAY_SIZE(", &array, b")\n", &l.herecurr]);
                self.warn("ARRAY_SIZE", &msg);
            }
        }

        // check for function declarations without arguments like "int foo()"
        if let Some(c) = self.m(r"(\b$Type\s*$Ident)\s*\(\s*\)", &l.line) {
            let f = c.s(1);
            let msg = cat(&[
                b"Bad function definition - ",
                &f,
                b"() should probably be ",
                &f,
                b"(void)\n",
                &l.herecurr,
            ]);
            self.error("FUNCTION_WITHOUT_ARGS", &msg);
        }

        // check for new typedefs, only function parameters and sparse annotations
        // make sense.
        if self.t(r"\btypedef\s", &l.line)
            && !self.t(r"\btypedef\s+$Type\s*\(\s*\*?$Ident\s*\)\s*\(", &l.line)
            && !self.t(r"\btypedef\s+$Type\s+$Ident\s*\(", &l.line)
            && !self.t(r"\b$typeTypedefs\b", &l.line)
            && !self.t(r"\b__bitwise\b", &l.line)
        {
            let msg = cat(&[b"do not add new typedefs\n", &l.herecurr]);
            self.warn("NEW_TYPEDEFS", &msg);
        }

        // * goes on variable not on type
        for c in self.all(
            r"(\($NonptrType(\s*(?:$Modifier\b\s*|\*\s*)+)\))",
            "",
            &l.line,
        ) {
            let from = c.s(2);
            let to = self.pointer_spacing(&from);
            if from != to {
                let msg = cat(&[
                    b"\"(foo",
                    &from,
                    b")\" should be \"(foo",
                    &to,
                    b")\"\n",
                    &l.herecurr,
                ]);
                self.error("POINTER_LOCATION", &msg);
            }
        }
        for c in self.all(
            r"(\b$NonptrType(\s*(?:$Modifier\b\s*|\*\s*)+)($Ident))",
            "",
            &l.line,
        ) {
            let from = c.s(2);
            let ident = c.s(3);
            let mut to = self.pointer_spacing(&from);
            // Modifiers should have spaces.
            self.s(r"(\b$Modifier$)", "", &mut to, false, &mut |c| {
                cat(&[&c.s(1), b" "])
            });
            if from != to && !self.t(r"^$Modifier$", &ident) {
                let msg = cat(&[
                    b"\"foo",
                    &from,
                    b"bar\" should be \"foo",
                    &to,
                    b"bar\"\n",
                    &l.herecurr,
                ]);
                self.error("POINTER_LOCATION", &msg);
            }
        }

        // do not use BUG() or variants
        if self.check
            && self.t(
                r"\b(?!AA_|BUILD_|IDA_|KVM_|RWLOCK_|snd_|SPIN_)(?:[a-zA-Z_]*_)?BUG(?:_ON)?(?:_[A-Z_]+)?\s*\(",
                &l.line,
            )
        {
            let msg = cat(&[
                b"Do not crash the kernel unless it is absolutely unavoidable--use WARN_ON_ONCE() plus recovery code (if feasible) instead of BUG() or variants\n",
                &l.herecurr,
            ]);
            self.chk("AVOID_BUG", &msg);
        }

        // avoid LINUX_VERSION_CODE
        if self.t(r"\bLINUX_VERSION_CODE\b", &l.line) {
            let msg = cat(&[
                b"LINUX_VERSION_CODE should be avoided, code should be for the version to which it is merged\n",
                &l.herecurr,
            ]);
            self.warn("LINUX_VERSION_CODE", &msg);
        }

        // check for uses of printk_ratelimit
        if self.t(r"\bprintk_ratelimit\s*\(", &l.line) {
            let msg = cat(&[
                b"Prefer printk_ratelimited or pr_<level>_ratelimited to printk_ratelimit\n",
                &l.herecurr,
            ]);
            self.warn("PRINTK_RATELIMITED", &msg);
        }

        // printk should use KERN_* levels
        if self.t(r"\bprintk\s*\(\s*(?!KERN_[A-Z]+\b)", &l.line) {
            let msg = cat(&[
                b"printk() should include KERN_<LEVEL> facility level\n",
                &l.herecurr,
            ]);
            self.warn("PRINTK_WITHOUT_KERN_LEVEL", &msg);
        }

        // prefer variants of (subsystem|netdev|dev|pr)_<level> to printk(KERN_<LEVEL>
        if let Some(c) = self.m(
            r"\b(printk(_once|_ratelimited)?)\s*\(\s*KERN_([A-Z]+)",
            &l.line,
        ) {
            let printk = c.s(1);
            let modifier = c.s(2);
            let orig = c.s(3);
            let mut level = orig.to_ascii_lowercase();
            if level == b"warning" {
                level = b"warn".to_vec();
            }
            let mut level2 = level.clone();
            if level == b"debug" {
                level2 = b"dbg".to_vec();
            }
            level.extend_from_slice(&modifier);
            level2.extend_from_slice(&modifier);
            let msg = cat(&[
                b"Prefer [subsystem eg: netdev]_",
                &level2,
                b"([subsystem]dev, ... then dev_",
                &level2,
                b"(dev, ... then pr_",
                &level,
                b"(...  to ",
                &printk,
                b"(KERN_",
                &orig,
                b" ...\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_PR_LEVEL", &msg);
        }

        // prefer dev_<level> to dev_printk(KERN_<LEVEL>
        if let Some(c) = self.m(r"\bdev_printk\s*\(\s*KERN_([A-Z]+)", &l.line) {
            let orig = c.s(1);
            let mut level = orig.to_ascii_lowercase();
            if level == b"warning" {
                level = b"warn".to_vec();
            }
            if level == b"debug" {
                level = b"dbg".to_vec();
            }
            let msg = cat(&[
                b"Prefer dev_",
                &level,
                b"(... to dev_printk(KERN_",
                &orig,
                b", ...\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_DEV_LEVEL", &msg);
        }

        // trace_printk should not be used in production code.
        if let Some(c) = self.m(r"\b(trace_printk|trace_puts|ftrace_vprintk)\s*\(", &l.line) {
            let msg = cat(&[
                b"Do not use ",
                &c.s(1),
                b"() in production code (this can be ignored if built only with a debug config option)\n",
                &l.herecurr,
            ]);
            self.warn("TRACE_PRINTK", &msg);
        }

        // ENOSYS means "bad syscall nr" and nothing else.
        if self.t(r"\bENOSYS\b", &l.line) {
            let msg = cat(&[
                b"ENOSYS means 'invalid syscall nr' and nothing else\n",
                &l.herecurr,
            ]);
            self.warn("ENOSYS", &msg);
        }

        // function brace can't be on same line, except for #defines of do while,
        // or if closed on same line
        if self.t(r"$Type\s*$Ident\s*$balanced_parens\s*\{", &l.sline)
            && !self.t(r"\#\s*define\b.*do\s*\{", &l.sline)
            && !self.t(r"}", &l.sline)
        {
            let msg = cat(&[
                b"open brace '{' following function definitions go on the next line\n",
                &l.herecurr,
            ]);
            self.error("OPEN_BRACE", &msg);
        }

        // open braces for enum, union and struct go on the same line.
        if self.t(r"^.\s*{", &l.line)
            && let Some(c) = self.m(
                r"^.\s*(?:typedef\s+)?(enum|union|struct)(?:\s+$Ident)?\s*$",
                &p.prevline,
            )
        {
            let msg = cat(&[
                b"open brace '{' following ",
                &c.s(1),
                b" go on the same line\n",
                &l.hereprev,
            ]);
            self.error("OPEN_BRACE", &msg);
        }

        // missing space after union, struct or enum definition
        if let Some(c) = self.m(
            r"^.\s*(?:typedef\s+)?(enum|union|struct)(?:\s+$Ident){1,2}[=\{]",
            &l.line,
        ) {
            let msg = cat(&[
                b"missing space after ",
                &c.s(1),
                b" definition\n",
                &l.herecurr,
            ]);
            self.warn("SPACING", &msg);
        }

        // Function pointer declarations
        if let Some(c) = self.m(
            r"^.\s*($Declare)\((\s*)\*(\s*)($Ident)(\s*)\)(\s*)\(",
            &l.line,
        ) {
            self.function_pointer_spacing(l, &c);
        }
    }

    /// `$to` for `POINTER_LOCATION`: a space first, none last, `*`s
    /// together.
    fn pointer_spacing(&self, from: &[u8]) -> Vec<u8> {
        let mut to = from.to_vec();
        self.s(r"^(\S)", "", &mut to, false, &mut |c| cat(&[b" ", &c.s(1)]));
        self.s(r"\s+$", "", &mut to, false, &mut |_| Vec::new());
        while self.s(r"\*\s+\*", "", &mut to, false, &mut |_| b"**".to_vec()) > 0 {}
        to
    }

    fn function_pointer_spacing(&mut self, l: &L, c: &super::re::Caps) {
        let mut declare = c.s(1);
        let pre_pointer_space = c.s(2);
        let post_pointer_space = c.s(3);
        let post_funcname_space = c.s(5);
        let pre_args_space = c.s(6);

        let mut post_declare_space: Vec<u8> = Vec::new();
        if let Some(m) = self.m(r"(\s+)$", &declare) {
            post_declare_space = m.s(1);
            declare = self.rtrim(&declare);
        }
        if !self.t(r"\*$", &declare) && self.t(r"^$", &post_declare_space) {
            let msg = cat(&[b"missing space after return type\n", &l.herecurr]);
            self.warn("SPACING", &msg);
        }
        for (space, what) in [
            (
                &pre_pointer_space,
                &b"Unnecessary space after function pointer open parenthesis\n"[..],
            ),
            (
                &post_pointer_space,
                b"Unnecessary space before function pointer name\n",
            ),
            (
                &post_funcname_space,
                b"Unnecessary space after function pointer name\n",
            ),
            (
                &pre_args_space,
                b"Unnecessary space before function pointer arguments\n",
            ),
        ] {
            if self.t(r"^\s", space) {
                let msg = cat(&[what, &l.herecurr]);
                self.warn("SPACING", &msg);
            }
        }
    }

    fn export_symbol(&mut self, p: &mut P, l: &L) {
        if let Some(realline_next) = l.realline_next
            && usize::try_from(realline_next - 1).is_ok_and(|i| i < self.lines.len())
            && !p.suppress_export.contains_key(&realline_next)
            && let Some(c) = self.m(
                r"EXPORT_SYMBOL.*\((.*)\)",
                &self.lines[usize::try_from(realline_next - 1).expect("line")].clone(),
            )
        {
            let mut name = c.s(1);
            self.s(r"^\s*($Ident).*", "", &mut name, false, &mut |c| c.s(1));
            let stat = l.stat.clone().unwrap_or_default();
            let value = if let Some(m) =
                self.m(r"^(?:.\s*}\s*\n)?.([A-Z_]+)\s*\(\s*($Ident)", &stat)
                && self.tl(r"^${Ident}_$2", &[("2", &m.s(2))], &name)
            {
                1
            } else if self
                .ml(
                    r"(?:
				\n.}\s*$|
				^.DEFINE_$Ident\(\Q$name\E\)|
				^.DECLARE_$Ident\(\Q$name\E\)|
				^.LIST_HEAD\(\Q$name\E\)|
				^.(?:$Storage\s+)?$Type\s*\(\s*\*\s*\Q$name\E\s*\)\s*\(|
				\b\Q$name\E(?:\s+$Attribute)*\s*(?:;|=|\[|\()
			    )",
                    "x",
                    &[("name", &name)],
                    &stat,
                )
                .is_none()
            {
                2
            } else {
                1
            };
            p.suppress_export.insert(realline_next, value);
        }
        if !p.suppress_export.contains_key(&l.linenr)
            && self.t(r"^.\s*$", &p.prevline)
            && self.t(r"EXPORT_SYMBOL.*\((.*)\)", &l.line)
        {
            p.suppress_export.insert(l.linenr, 2);
        }
        if p.suppress_export.get(&l.linenr) == Some(&2) {
            let msg = cat(&[
                b"EXPORT_SYMBOL(foo); should immediately follow its function/variable\n",
                &l.herecurr,
            ]);
            self.warn("EXPORT_SYMBOL", &msg);
        }
    }

    /// From the bracket and function-name spacing to the operator checks.
    fn spacing_checks(&mut self, p: &P, l: &L) {
        // check for spacing round square brackets
        for c in self.all(r"(.*?\s)\[", "", &l.line) {
            let where_ = c.start(1).expect("group");
            let prefix = c.s(1);
            if !self.t(r"$Type\s+$", &prefix)
                && (where_ != 0 || !self.t(r"^.\s+$", &prefix))
                && !self.t(r"[{,:]\s+$", &prefix)
            {
                let msg = cat(&[
                    b"space prohibited before open square bracket '['\n",
                    &l.herecurr,
                ]);
                self.error("BRACKET_SPACE", &msg);
            }
        }

        // check for spaces between functions and their parentheses.
        for c in self.all(r"($Ident)\s+\(", "", &l.line) {
            let name = c.s(1);
            let ctx_before = l.line[..c.start(1).expect("group")].to_vec();
            let ctx = cat(&[&ctx_before, &name]);
            if self.tf(
                r"^(?:
				if|for|while|switch|return|case|
				volatile|__volatile__|
				__attribute__|format|__extension__|
				asm|__asm__|scoped_guard)$",
                "x",
                &name,
            ) || self.t(r"^.\s*\#\s*define\s*$", &ctx_before)
                || self.t(r"^.\s*\#\s*elif\s*$", &ctx)
                || self.t(r"$Type$", &ctx)
            {
            } else {
                let msg = cat(&[
                    b"space prohibited between function name and open parenthesis '('\n",
                    &l.herecurr,
                ]);
                self.warn("SPACING", &msg);
            }
        }

        // Check operator spacing.
        if !self.t(r"\#\s*include", &l.line) {
            self.operator_spacing(p, l);
        }

        // check for whitespace before a non-naked semicolon
        if self.t(r"^\+.*\S\s+;\s*$", &l.line) {
            let msg = cat(&[b"space prohibited before semicolon\n", &l.herecurr]);
            self.warn("SPACING", &msg);
        }

        // check for multiple assignments
        if self.check && self.t(r"^.\s*$Lval\s*=\s*$Lval\s*=(?!=)", &l.line) {
            let msg = cat(&[b"multiple assignments should be avoided\n", &l.herecurr]);
            self.chk("MULTIPLE_ASSIGNMENTS", &msg);
        }

        // need space before brace following if, while, etc
        if (self.t(r"\(.*\)\{", &l.line) && !self.t(r"\($Type\)\{", &l.line))
            || self.t(r"\b(?:else|do)\{", &l.line)
        {
            let msg = cat(&[b"space required before the open brace '{'\n", &l.herecurr]);
            self.error("SPACING", &msg);
        }

        // closing brace should have a space following it when it has anything
        // on the line
        if self.t(r"}(?!(?:,|;|\)|\}))\S", &l.line) {
            let msg = cat(&[b"space required after that close brace '}'\n", &l.herecurr]);
            self.error("SPACING", &msg);
        }

        // check spacing on square brackets
        if self.t(r"\[\s", &l.line) && !self.t(r"\[\s*$", &l.line) {
            let msg = cat(&[
                b"space prohibited after that open square bracket '['\n",
                &l.herecurr,
            ]);
            self.error("SPACING", &msg);
        }
        if self.t(r"\s\]", &l.line) {
            let msg = cat(&[
                b"space prohibited before that close square bracket ']'\n",
                &l.herecurr,
            ]);
            self.error("SPACING", &msg);
        }

        // check spacing on parentheses
        if self.t(r"\(\s", &l.line)
            && !self.t(r"\(\s*(?:\\)?$", &l.line)
            && !self.t(r"for\s*\(\s+;", &l.line)
        {
            let msg = cat(&[
                b"space prohibited after that open parenthesis '('\n",
                &l.herecurr,
            ]);
            self.error("SPACING", &msg);
        }
        if self.t(r"(\s+)\)", &l.line)
            && !self.t(r"^.\s*\)", &l.line)
            && !self.t(r"for\s*\(.*;\s+\)", &l.line)
            && !self.t(r":\s+\)", &l.line)
        {
            let msg = cat(&[
                b"space prohibited before that close parenthesis ')'\n",
                &l.herecurr,
            ]);
            self.error("SPACING", &msg);
        }

        if self.check {
            self.unnecessary_parentheses(p, l);
        }

        // check that goto labels aren't indented (allow a single space indentation)
        // and ignore bitfield definitions like foo:1
        if self.t(r"^.\s+[A-Za-z_][A-Za-z\d_]*:(?!\s*\d+)", &l.sline)
            && !self.t(r"^. [A-Za-z\d_][A-Za-z\d_]*:", &l.sline)
            && !self.t(r"^.\s+default:", &l.sline)
        {
            let msg = cat(&[b"labels should not be indented\n", &l.herecurr]);
            self.warn("INDENTED_LABEL", &msg);
        }

        // check if a statement with a comma should be two statements like:
        if let Some(stat) = l.stat.clone()
            && self.t(
                r"^\+\s*(?:$Lval\s*$Assignment\s*)?$FuncArg\s*,\s*(?:$Lval\s*$Assignment\s*)?$FuncArg\s*;\s*$",
                &stat,
            )
        {
            let cnt = count(&stat, b'\n') + 1;
            let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
            let msg = cat(&[b"Possible comma where semicolon could be used\n", &herectx]);
            self.warn("SUSPECT_COMMA_SEMICOLON", &msg);
        }

        // return is not a function
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.mf(r"^.\s*return(\s*)\(", "s", &stat)
        {
            let spacing = c.s(1);
            if let Some(m) = self.m(r"^.\s*return\s*($balanced_parens)\s*;\s*$", &stat) {
                let value = self.deparenthesize(Some(&m.s(1)));
                if self.t(r"^\s*$FuncArg\s*(?:\?|$)", &value) {
                    let msg = cat(&[
                        b"return is not a function, parentheses are not required\n",
                        &l.herecurr,
                    ]);
                    self.error("RETURN_PARENTHESES", &msg);
                }
            } else if !self.t(r"\s+", &spacing) {
                let msg = cat(&[
                    b"space required before the open parenthesis '('\n",
                    &l.herecurr,
                ]);
                self.error("SPACING", &msg);
            }
        }

        // unnecessary return in a void function
        if self.t(r"^[ \+]}\s*$", &l.sline)
            && self.t(r"^\+\treturn\s*;\s*$", &p.prevline)
            && l.linenr >= 3
            && self
                .line_at(l.linenr - 3)
                .is_some_and(|x| self.t(r"^[ +]", x) && !self.t(r"^[ +]\s*$Ident\s*:", x))
        {
            let msg = cat(&[
                b"void function return statements are not generally useful\n",
                &l.hereprev,
            ]);
            self.warn("RETURN_VOID", &msg);
        }

        // if statements using unnecessary parentheses - ie: if ((foo == bar))
        if let Some(c) = self.m(r"\bif\s*((?:\(\s*){2,})", &l.line) {
            let cnt = n(count(&c.s(1), b'('));
            if let Some(m) = self.ml(
                r"\bif\s*(?:\(\s*){$count,$count}$LvalOrFunc\s*($Compare)\s*$LvalOrFunc(?:\s*\)){$count,$count}",
                "",
                &[("count", &cnt)],
                &l.line,
            ) {
                let comp = m.s(4);
                let msg: &[u8] = if comp == b"==" { b" - maybe == should be = ?" } else { b"" };
                let msg = cat(&[b"Unnecessary parentheses", msg, b"\n", &l.herecurr]);
                self.warn("UNNECESSARY_PARENTHESES", &msg);
            }
        }

        // comparisons with a constant or upper case identifier on the left
        if let Some(c) = self.m(
            r"^\+(.*)\b($Constant|[A-Z_][A-Z0-9_]*)\s*($Compare)\s*($LvalOrFunc)",
            &l.line,
        ) {
            let lead = c.s(1);
            let to = c.s(4);
            if !self.t(r"(?:$Operators|\.)\s*$", &lead)
                && !self.t(r"^(?:Constant|[A-Z_][A-Z0-9_]*)$", &to)
            {
                let msg = cat(&[
                    b"Comparisons should place the constant on the right side of the test\n",
                    &l.herecurr,
                ]);
                self.warn("CONSTANT_COMPARISON", &msg);
            }
        }

        // Return of what appears to be an errno should normally be negative
        if let Some(c) = self.m(
            r"\breturn(?:\s*\(+\s*|\s+)(E[A-Z]+)(?:\s*\)+\s*|\s*)[;:,]",
            &l.sline,
        ) {
            let name = c.s(1);
            if name != b"EOF" && name != b"ERROR" && !self.t(r"^EPOLL", &name) {
                let msg = cat(&[
                    b"return of an errno should typically be negative (ie: return -",
                    &name,
                    b")\n",
                    &l.herecurr,
                ]);
                self.warn("USE_NEGATIVE_ERRNO", &msg);
            }
        }

        // Need a space before open parenthesis after if, while etc
        if self.t(r"\b(if|while|for|switch)\(", &l.line) {
            let msg = cat(&[
                b"space required before the open parenthesis '('\n",
                &l.herecurr,
            ]);
            self.error("SPACING", &msg);
        }
    }

    fn unnecessary_parentheses(&mut self, p: &P, l: &L) {
        // check unnecessary parentheses around addressof/dereference single $Lvals
        for c in self.all(
            r"(?:[^&]&\s*|\*)\(\s*($Ident\s*(?:$Member\s*)+)\s*\)",
            "",
            &l.line,
        ) {
            let msg = cat(&[
                b"Unnecessary parentheses around ",
                &c.s(1),
                b"\n",
                &l.herecurr,
            ]);
            self.chk("UNNECESSARY_PARENTHESES", &msg);
        }

        // check for unnecessary parentheses around function pointer uses
        if let Some(c) = self.m(
            r"(\bif\s*|)(\(\s*$Ident\s*(?:$Member\s*)+\))[ \t]*\(",
            &l.line,
        ) && !self.t(r"^if", &c.s(1))
        {
            let msg = cat(&[
                b"Unnecessary parentheses around function pointer ",
                &c.s(2),
                b"\n",
                &l.herecurr,
            ]);
            self.chk("UNNECESSARY_PARENTHESES", &msg);
        }

        // check for unnecessary parentheses around comparisons
        // except in drivers/staging
        if !self.t(r"^(?:drivers/staging/)", &p.realfile)
            && let Some(stat) = &l.stat
            && let Some(c) = self.m(r"(^.\s*if\s*($balanced_parens))", stat)
        {
            let if_stat = c.s(1);
            let test = substr_s(&c.s(2), 1, Some(-1));
            let mut herectx: Option<Vec<u8>> = None;
            for m in self.all(
                r"(?:^|[^\w\&\!\~])+\s*\(\s*([\&\!\~]?\s*$Lval\s*(?:$Compare\s*$FuncArg)?)\s*\)",
                "",
                &test,
            ) {
                let matched = m.s(1);
                // avoid parentheses around potential macro args
                if self.t(r"^\s*\w+\s*$", &matched) {
                    continue;
                }
                let herectx = herectx.get_or_insert_with(|| {
                    let mut h = cat(&[&l.here, b"\n"]);
                    let cnt = count(&if_stat, b'\n') + 1;
                    for k in 0..cnt {
                        let rl = self.raw_line(l.linenr, k).unwrap_or_default();
                        h.extend(cat(&[&rl, b"\n"]));
                        if self.t(r"^[ \+].*\{", &rl) {
                            break;
                        }
                    }
                    h
                });
                let msg = cat(&[
                    b"Unnecessary parentheses around '",
                    &matched,
                    b"'\n",
                    herectx,
                ]);
                self.chk("UNNECESSARY_PARENTHESES", &msg);
            }
        }
    }

    fn operator_spacing(&mut self, p: &P, l: &L) {
        let opline = &l.opline;
        let elements = split(&self.re(r"($ops|;)", ""), opline);
        let mut fix_elements: Vec<Option<Vec<u8>>> = Vec::with_capacity(elements.len());
        let mut off: i64 = 0;
        for el in &elements {
            fix_elements.push(substr(&l.rawline, off, Some(len(el))).map(<[u8]>::to_vec));
            off += len(el);
        }
        off = 0;
        let blank = copy_spacing(opline);
        let mut k = 0;
        while k + 1 < elements.len() {
            off += len(&elements[k]);

            // Pick up the preceding and succeeding characters.
            let ca = substr_s(opline, 0, Some(off));
            let mut cc: Vec<u8> = Vec::new();
            if len(opline) >= off + len(&elements[k + 1]) {
                cc = substr_s(opline, off + len(&elements[k + 1]), None);
            }

            let el = &elements[k];
            let mut a: u8 = 0;
            if !el.is_empty() {
                a = b'V';
            }
            if self.t(r"\s$", el) {
                a = b'W';
            }
            if self.t(r"$;$", el) {
                a = b'C';
            }
            if self.t(r"(\[|\()$", el) {
                a = b'B';
            }
            if el.is_empty() {
                a = b'O';
            }
            if self.t(r"^\s*$", &ca) {
                a = b'E';
            }

            let op = elements[k + 1].clone();

            let mut c: u8 = 0;
            if let Some(next) = elements.get(k + 2) {
                if !next.is_empty() {
                    c = b'V';
                }
                if self.t(r"^\s", next) {
                    c = b'W';
                }
                if self.t(r"^$;", next) {
                    c = b'C';
                }
                if self.t(r"^(\)|\]|;)", next) {
                    c = b'B';
                }
                if next.is_empty() {
                    c = b'O';
                }
                if self.t(r"^\s*\\$", next) {
                    c = b'E';
                }
            } else {
                c = b'E';
            }

            let mut ctx = Vec::new();
            if a != 0 {
                ctx.push(a);
            }
            ctx.push(b'x');
            if c != 0 {
                ctx.push(c);
            }
            let at_ = cat(&[b"(ctx:", &ctx, b")"]);
            let ptr = cat(&[&substr_s(&blank, 0, Some(off)), b"^"]);
            let hereptr = cat(&[&l.hereline, &ptr, b"\n"]);

            // Pull out the value of this operator.
            let op_type = substr_s(&l.curr_values, off + 1, Some(1));

            // Get the full operator variant.
            let opv = cat(&[&op, &substr_s(&l.curr_vars, off, Some(1))]);

            let ctx_m = |this: &Self, tpl: &'static str| this.t(tpl, &ctx);
            let report = |this: &mut Self, error: bool, what: &[u8]| {
                let msg = cat(&[what, b" '", &op, b"' ", &at_, b"\n", &hereptr]);
                if error {
                    this.error("SPACING", &msg);
                } else {
                    this.chk("SPACING", &msg);
                }
            };

            // Ignore operators passed as parameters.
            if op_type != b"V" && self.t(r"\s$", &ca) && self.t(r"^\s*[,\)]", &cc) {
            } else if op == b";" {
                // ; should have either the end of line or a space or \ after it
                if !ctx_m(self, r".x[WEBC]") && !self.t(r"^\\", &cc) && !self.t(r"^;", &cc) {
                    report(self, true, b"space required after that");
                }
            } else if op == b"//" || opv == b":B" {
                // a comment, or a bitfield's colon
            } else if op == b"->" {
                // No spaces for: ->
                if ctx_m(self, r"Wx.|.xW") {
                    report(self, true, b"spaces prohibited around that");
                }
            } else if op == b"," {
                // , must not have a space before and must have a space on the right.
                if ctx_m(self, r"Wx.") {
                    report(self, true, b"space prohibited before that");
                }
                if !ctx_m(self, r".x[WEC]") && !self.t(r"^}", &cc) {
                    report(self, true, b"space required after that");
                }
            } else if opv == b"*_" {
                // '*' as part of a type definition -- reported already.
            } else if op == b"!"
                || op == b"~"
                || opv == b"*U"
                || opv == b"-U"
                || opv == b"&U"
                || opv == b"&&U"
            {
                // unary operators should have a space before and
                // none after.  May be left adjacent to another
                // unary operator, or a cast
                if !ctx_m(self, r"[WEBC]x.") && !self.t(r"(?:\)|!|~|\*|-|\&|\||\+\+|\-\-|\{)$", &ca)
                {
                    report(self, true, b"space required before that");
                }
                if op == b"*" && self.t(r"\s*$Modifier\b", &cc) {
                    // A unary '*' may be const
                } else if ctx_m(self, r".xW") {
                    report(self, true, b"space prohibited after that");
                }
            } else if op == b"++" || op == b"--" {
                // unary ++ and unary -- are allowed no space on one side.
                if !ctx_m(self, r"[WEOBC]x[^W]") && !ctx_m(self, r"[^W]x[WOBEC]") {
                    report(self, true, b"space required one side of that");
                }
                if ctx_m(self, r"Wx[BE]") || (ctx_m(self, r"Wx.") && self.t(r"^;", &cc)) {
                    report(self, true, b"space prohibited before that");
                }
                if ctx_m(self, r"ExW") {
                    report(self, true, b"space prohibited after that");
                }
            } else if [
                &b"<<"[..],
                b">>",
                b"&",
                b"^",
                b"|",
                b"+",
                b"-",
                b"*",
                b"/",
                b"%",
            ]
            .contains(&op.as_slice())
            {
                // << and >> may either have or not have spaces both sides
                if self.check {
                    if fix_elements.get(k + 2).is_some_and(Option::is_some)
                        && !ctx_m(self, r"[EW]x[EW]")
                    {
                        report(self, false, b"spaces preferred around that");
                    } else if !fix_elements.get(k + 2).is_some_and(Option::is_some)
                        && !ctx_m(self, r"Wx[OE]")
                    {
                        report(self, false, b"space preferred before that");
                    }
                } else if ctx_m(self, r"Wx[^WCE]|[^WCE]xW") {
                    report(self, true, b"need consistent spacing around");
                }
            } else if opv == b":C" || opv == b":L" {
                // A colon needs no spaces before when it is
                // terminating a case value or a label.
                if ctx_m(self, r"Wx.") && !self.t(r".*\.lds\.h$", &p.realfile) {
                    report(self, true, b"space prohibited before that");
                }
            } else if !ctx_m(self, r"[EWC]x[CWE]") {
                // All the others need spaces both sides.
                let mut ok = false;

                // Ignore email addresses <foo@bar>
                if (op == b"<" && self.t(r"^\S+\@\S+>", &cc))
                    || (op == b">" && self.t(r"<\S+\@\S+$", &ca))
                {
                    ok = true;
                }

                // for asm volatile statements
                // ignore a colon with another
                // colon immediately before or after
                if op == b":" && (self.t(r":$", &ca) || self.t(r"^:", &cc)) {
                    ok = true;
                }

                // messages are ERROR, but ?: are CHK
                if !ok {
                    let chk = (op == b"?:" || op == b"?" || op == b":") && ctx_m(self, r"VxV");
                    report(self, !chk, b"spaces required around that");
                }
            }
            off += len(&elements[k + 1]);
            k += 2;
        }
    }

    /// From the open-brace checks to the line-continuation ones.
    fn statement_checks(&mut self, p: &mut P, l: &mut L) {
        // Check for illegal assignment in if conditional -- and check for trailing
        // statements after the conditional.
        if self.t(r"do\s*(?!{)", &l.line) {
            self.statement_block(l, p.realcnt);
            let b = self.ctx_statement_block(
                l.line_nr_next.expect("next line"),
                l.remain_next.expect("remain"),
                l.off_next.expect("offset"),
            );
            let mut stat_next = b.statement;
            self.s(r"\n.", "", &mut stat_next, true, &mut |_| b"\n ".to_vec());
            if self.t(r"^\s*while\b", &stat_next) {
                // If the statement carries leading newlines,
                // then count those as offsets.
                let whitespace = self
                    .mf(r"^((?:\s*\n[+-])*\s*)", "s", &stat_next)
                    .map(|c| c.s(1))
                    .unwrap_or_default();
                let offset = count(&whitespace, b'\n');
                p.suppress_whiletrailers
                    .insert(l.line_nr_next.expect("next line") + offset);
            }
        }
        if !p.suppress_whiletrailers.contains(&l.linenr)
            && let (Some(stat), Some(cond)) = (l.stat.clone(), l.cond.clone())
            && self.t(r"\b(?:if|while|for)\s*\(", &l.line)
            && !self.t(r"^.\s*#", &l.line)
        {
            let mut s = stat;
            let c = cond;
            if self.tf(r"\bif\s*\(.*[^<>!=]=[^=].*", "s", &c) {
                let msg = cat(&[b"do not use assignment in if condition\n", &l.herecurr]);
                self.error("ASSIGN_IN_IF", &msg);
            }

            // Find out what is on the end of the line after the
            // conditional.
            s.drain(..c.len().min(s.len()));
            self.s(r"\n.*", "", &mut s, true, &mut |_| Vec::new());
            s.retain(|&b| b != SUBSEP);
            if !c.is_empty() && !self.t(r"^\s*{?\s*\\*\s*$", &s) && !self.t(r"}\s*while\s*", &c) {
                // Find out how long the conditional actually is.
                let cond_lines = count(&c, b'\n');
                let mut stat_real: Vec<u8> = Vec::new();
                if cond_lines != 0 {
                    stat_real = cat(&[
                        &self.raw_line(l.linenr, cond_lines).unwrap_or_default(),
                        b"\n",
                    ]);
                }
                if cond_lines > 1 {
                    stat_real = cat(&[b"[...]\n", &stat_real]);
                }
                let msg = cat(&[
                    b"trailing statements should be on next line\n",
                    &l.herecurr,
                    &stat_real,
                ]);
                self.error("TRAILING_STATEMENTS", &msg);
            }
        }

        // Check for bitwise tests written as boolean
        if self.tf(
            r"
			(?:
				(?:\[|\(|\&\&|\|\|)
				\s*0[xX][0-9]+\s*
				(?:\&\&|\|\|)
			|
				(?:\&\&|\|\|)
				\s*0[xX][0-9]+\s*
				(?:\&\&|\|\||\)|\])
			)",
            "x",
            &l.line,
        ) {
            let msg = cat(&[
                b"boolean test with hexadecimal, perhaps just 1 & or |?\n",
                &l.herecurr,
            ]);
            self.warn("HEXADECIMAL_BOOLEAN_TEST", &msg);
        }

        // if and else should not have general statements after it
        if let Some(c) = self.m(r"^.\s*(?:}\s*)?else\b(.*)", &l.line) {
            let mut s = c.s(1);
            s.retain(|&b| b != SUBSEP);
            if !self.t(r"^\s*(?:\sif|(?:{|)\s*\\?\s*$)", &s) {
                let msg = cat(&[b"trailing statements should be on next line\n", &l.herecurr]);
                self.error("TRAILING_STATEMENTS", &msg);
            }
        }
        // if should not continue a brace
        if self.t(r"}\s*if\b", &l.line) {
            let msg = cat(&[
                b"trailing statements should be on next line (or did you mean 'else if'?)\n",
                &l.herecurr,
            ]);
            self.error("TRAILING_STATEMENTS", &msg);
        }
        // case and default should not have general statements after them
        l.line_pos = None;
        if let Some(c) = self.m(r"^.\s*(?:case\s*.*|default\s*):", &l.line) {
            let pos = c.end(0).expect("match");
            let re = self.re(
                r"\G(?:
			(?:\s*$;*)(?:\s*{)?(?:\s*$;*)(?:\s*\\)?\s*$|
			\s*return\s+
		    )",
                "x",
            );
            if let Some(m) = re.find_g(&l.line, pos, None) {
                l.line_pos = Some(m.end());
            } else {
                let msg = cat(&[b"trailing statements should be on next line\n", &l.herecurr]);
                self.error("TRAILING_STATEMENTS", &msg);
            }
        }

        // Check for }<nl>else {, these must be at the same
        // indent level to be relevant to each other.
        if self.t(r"}\s*$", &p.prevline)
            && self.t(r"^.\s*else\s*", &l.line)
            && p.previndent == p.indent
        {
            let msg = cat(&[b"else should follow close brace '}'\n", &l.hereprev]);
            self.error("ELSE_AFTER_BRACE", &msg);
        }

        if self.t(r"}\s*$", &p.prevline)
            && self.t(r"^.\s*while\s*", &l.line)
            && p.previndent == p.indent
        {
            let b = self.ctx_statement_block(l.linenr, p.realcnt, 0);
            let mut s = b.statement;
            s.drain(..b.condition.len().min(s.len()));
            self.s(r"\n.*", "", &mut s, true, &mut |_| Vec::new());
            if self.t(r"^\s*;", &s) {
                let msg = cat(&[b"while should follow close brace '}'\n", &l.hereprev]);
                self.error("WHILE_AFTER_BRACE", &msg);
            }
        }

        // Specific variable tests
        if self.check {
            self.camelcase(l);
        }

        // no spaces allowed after \ in define
        if self.t(r"\#\s*define.*\\\s+$", &l.line) {
            let msg = cat(&[
                b"Whitespace after \\ makes next lines useless\n",
                &l.herecurr,
            ]);
            self.warn("WHITESPACE_AFTER_LINE_CONTINUATION", &msg);
        }
    }

    fn camelcase(&mut self, l: &L) {
        let re = self.re(r"($Constant|$Lval)", "");
        let mut pos = l.line_pos.unwrap_or(0);
        let mut last_empty_at = None;
        while pos <= l.line.len() {
            let Some(c) = re.find_g(&l.line, pos, last_empty_at) else {
                break;
            };
            last_empty_at = (c.start() == c.end()).then_some(c.end());
            pos = c.end();
            let var = c.str(&l.line, 1);
            if !self.t(r"^$Constant$", &var)
                && self.t(r"[A-Z][a-z]|[a-z][A-Z]", &var)
                && !self.t(r"^_Generic$", &var)
                && !self.t(r"^(?:[A-Z]+_){1,5}[A-Z]{1,3}[a-z]", &var)
                && !self.t(r"^(?:Clear|Set|TestClear|TestSet|)Page[A-Z]", &var)
                && !self.t(r"^ETHTOOL_LINK_MODE_", &var)
                && !self.t(
                    r"^(?:[a-z0-9_]*|[A-Z0-9_]*)?_?[a-z][A-Z](?:_[a-z0-9_]+|_[A-Z0-9_]+)?$",
                    &var,
                )
                && !self.t(r"^(?:[a-z_]*?)_?(?:[KMGT]iB|[KMGT]?Hz)(?:_[a-z_]+)?$", &var)
            {
                for w in self.all(r"\b($Ident)", "", &var) {
                    let word = w.s(1);
                    if !self.t(r"[A-Z][a-z]|[a-z][A-Z]", &word) {
                        continue;
                    }
                    if self.camelcase.insert(word.clone()) {
                        let msg = cat(&[b"Avoid CamelCase: <", &word, b">\n", &l.herecurr]);
                        self.chk("CAMELCASE", &msg);
                    }
                }
            }
        }
    }

    /// The `#define` checks.
    fn macro_checks(&mut self, p: &mut P, l: &mut L) {
        // Usually multi-statement macros should be enclosed in a do {} while
        // (0) loop.  Grab the first statement and ensure its the whole macro
        // if its not enclosed in a known good container
        if !self.t(r"/vmlinux.lds.h$", &p.realfile)
            && let Some(guard) = self.m(r"^.\s*\#\s*define\s*$Ident(\()?", &l.line)
        {
            let guard_1 = guard.get(1).map(<[u8]>::to_vec);
            self.complex_macro(p, l, guard_1);
        } else if self.t(r"/vmlinux.lds.h$", &p.realfile) {
            let mut line = std::mem::take(&mut l.line);
            let re = self.re(r"(\w+)", "");
            let symbols = &mut self.maybe_linker_symbol;
            subst(&re, &mut line, true, &mut |c| {
                let counter = symbols.entry(c.s(1)).or_insert(0);
                let old = *counter;
                *counter += 1;
                n(i64::try_from(old).expect("count"))
            });
            l.line = line;
            self.lines[usize::try_from(l.linenr - 1).expect("line")].clone_from(&l.line);
        } else if !self.t(r"^..*\\$", &p.prevline)
            && !self.t(r"^\+\s*\#.*\\$", &l.line)
            && !self.t(r"^\+.*\b(__asm__|asm)\b.*\\$", &l.line)
            && self.t(r"^\+.*\\$", &l.line)
        {
            let msg = cat(&[b"Avoid unnecessary line continuations\n", &l.herecurr]);
            self.warn("LINE_CONTINUATIONS", &msg);
        }

        // do {} while (0) macro tests:
        // single-statement macros do not need to be enclosed in do while (0) loop,
        // macro should not end with a semicolon
        if !self.t(r"/vmlinux.lds.h$", &p.realfile)
            && self.t(r"^.\s*\#\s*define\s+$Ident(\()?", &l.line)
        {
            let b = self.ctx_statement_block(l.linenr, p.realcnt, 0);
            let mut ctx = b.statement.clone();
            let mut dstat = b.statement;
            self.s(r"\\\n.", "", &mut dstat, true, &mut |_| Vec::new());
            for x in &mut dstat {
                if *x == SUBSEP {
                    *x = b' ';
                }
            }
            if let Some(c) = self.m(
                r"^\+\s*#\s*define\s+$Ident\s*${balanced_parens}\s*do\s*{(.*)\s*}\s*while\s*\(\s*0\s*\)\s*([;\s]*)\s*$",
                &dstat,
            ) {
                let stmts = c.s(2);
                let semis = c.get(3).map(<[u8]>::to_vec);
                self.s(r"\n*$", "", &mut ctx, false, &mut |_| Vec::new());
                let cnt = count(&ctx, b'\n') + 1;
                let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
                if count(&stmts, b';') == 1 && !self.t(r"^\s*(if|while|for|switch)\b", &stmts) {
                    let msg = cat(&[b"Single statement macros should not use a do {} while (0) loop\n", &herectx]);
                    self.warn("SINGLE_STATEMENT_DO_WHILE_MACRO", &msg);
                }
                if semis.is_some_and(|s| !s.is_empty()) {
                    let msg = cat(&[b"do {} while (0) macros should not be semicolon terminated\n", &herectx]);
                    self.warn("DO_WHILE_MACRO_WITH_TRAILING_SEMICOLON", &msg);
                }
            } else if self.t(r"^\+\s*#\s*define\s+$Ident.*;\s*$", &dstat) {
                self.s(r"\n*$", "", &mut ctx, false, &mut |_| Vec::new());
                let cnt = count(&ctx, b'\n') + 1;
                let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
                let msg = cat(&[b"macros should not use a trailing semicolon\n", &herectx]);
                self.warn("TRAILING_SEMICOLON", &msg);
            }
        }

        // check for redundant bracing round if etc
        if let Some(c) = self.m(r"(^.*)\bif\b", &l.line)
            && !self.t(r"else\s*$", &c.s(1))
        {
            self.redundant_braces(p, l);
        }
        if !p.suppress_ifbraces.contains(&(l.linenr - 1))
            && let Some(c) = self.m(r"\b(if|while|for|else)\b", &l.line)
        {
            let start = c.start(0).expect("match");
            self.single_statement_braces(p, l, start);
        }

        if self.check {
            // check for single line unbalanced braces
            if self.t(r"^.\s*\}\s*else\s*$", &l.sline) || self.t(r"^.\s*else\s*\{\s*$", &l.sline) {
                let msg = cat(&[b"Unbalanced braces around else statement\n", &l.herecurr]);
                self.chk("BRACES", &msg);
            }

            // check for unnecessary blank lines around braces
            if self.t(r"^.\s*}\s*$", &l.line) && self.t(r"^.\s*$", &p.prevrawline) {
                let msg = cat(&[
                    b"Blank lines aren't necessary before a close brace '}'\n",
                    &l.hereprev,
                ]);
                self.chk("BRACES", &msg);
            }
            if self.t(r"^.\s*$", &l.rawline) && self.t(r"^..*{\s*$", &p.prevline) {
                let msg = cat(&[
                    b"Blank lines aren't necessary after an open brace '{'\n",
                    &l.hereprev,
                ]);
                self.chk("BRACES", &msg);
            }
        }

        // no volatiles please
        if self.t(r"\bvolatile\b", &l.line) && !self.t(r"$asm_volatile", &l.line) {
            let msg = cat(&[
                b"Use of volatile is usually wrong: see Documentation/process/volatile-considered-harmful.rst\n",
                &l.herecurr,
            ]);
            self.warn("VOLATILE", &msg);
        }
    }

    fn complex_macro(&mut self, p: &P, l: &L, guard_1: Option<Vec<u8>>) {
        let b = self.ctx_statement_block(l.linenr, p.realcnt, 0);
        let mut dstat = b.statement;
        let mut ctx = dstat.clone();

        // `$1` as the statements below leave it, should the define not
        // match: Perl's captures are those of the last match that did.
        let mut last_1 = guard_1;
        let has_flow_statement = if let Some(c) = self.m(r"\b(goto|return)\b", &ctx) {
            last_1 = Some(c.s(1));
            true
        } else {
            false
        };
        let mut has_arg_concat = false;
        if self.t(r"\#\#", &ctx) {
            last_1 = None;
            if self.t(r"\#\#\s*(?:__VA_ARGS__|args)\b", &ctx) {
                last_1 = None;
            } else {
                has_arg_concat = true;
            }
        }

        let mut matched = false;
        let mut define_args: Option<Vec<u8>> = None;
        self.s(
            r"^.\s*\#\s*define\s+$Ident(\([^\)]*\))?\s*",
            "",
            &mut dstat,
            false,
            &mut |c| {
                matched = true;
                define_args = c.get(1).map(<[u8]>::to_vec);
                Vec::new()
            },
        );
        if !matched {
            define_args = last_1;
        }
        let define_stmt = dstat.clone();
        let mut def_args: Vec<Vec<u8>> = Vec::new();
        if let Some(args) = define_args.filter(|a| !a.is_empty()) {
            let mut args = substr_s(&args, 1, Some(len(&args) - 2));
            args.retain(|b| !super::check::perl_space(*b));
            self.s(r"\\\+?", "", &mut args, true, &mut |_| Vec::new());
            def_args = split(&self.re(r",", ""), &args);
        }

        dstat.retain(|&b| b != SUBSEP);
        self.s(r"\\\n.", "", &mut dstat, true, &mut |_| Vec::new());
        self.s(r"^\s*", "s", &mut dstat, false, &mut |_| Vec::new());
        self.s(r"\s*$", "s", &mut dstat, false, &mut |_| Vec::new());

        // Flatten any parentheses and braces
        while self.s(r"\([^\(\)]*\)", "", &mut dstat, false, &mut |_| {
            b"1u".to_vec()
        }) > 0
            || self.s(r"\{[^\{\}]*\}", "", &mut dstat, false, &mut |_| {
                b"1u".to_vec()
            }) > 0
            || self.s(r".\[[^\[\]]*\]", "", &mut dstat, false, &mut |_| {
                b"1u".to_vec()
            }) > 0
        {}

        // Flatten any obvious string concatenation.
        while self.s(r"($String)\s*$Ident", "", &mut dstat, false, &mut |c| {
            c.s(1)
        }) > 0
            || self.s(r"$Ident\s*($String)", "", &mut dstat, false, &mut |c| {
                c.s(1)
            }) > 0
        {}

        // Make asm volatile uses seem like a generic function
        self.s(
            r"\b_*asm_*\s+_*volatile_*\b",
            "",
            &mut dstat,
            true,
            &mut |_| b"asm_volatile".to_vec(),
        );

        self.s(r"\n*$", "", &mut ctx, false, &mut |_| Vec::new());
        let stmt_cnt = count(&ctx, b'\n') + 1;
        let herectx = self.get_stat_here(l.linenr, stmt_cnt, &l.here);

        if !dstat.is_empty()
            && !self.t(r"^(?:$Ident|-?$Constant),$", &dstat)
            && !self.t(r"^(?:$Ident|-?$Constant);$", &dstat)
            && !self.t(r"^[!~-]?(?:$Lval|$Constant)$", &dstat)
            && !self.t(r"^'X'$", &dstat)
            && !self.t(r"^'XX'$", &dstat)
            && !self.tf(
                r#"
				$Declare|
				# named exceptions
				module_param_named|
				MODULE_PARM_DESC|
				DECLARE_PER_CPU|
				DEFINE_PER_CPU|
				static_assert|
				# declaration primitives
				__typeof__\(|
				union|
				struct|
				\.$Ident\s*=\s*|
				^\"|\"$|
				^\[
			"#,
                "x",
                &dstat,
            )
            && !self.t(r"^\.$Ident\s*=", &dstat)
            && !self.t(r"^(?:\#\s*$Ident|\#\s*$Constant)\s*$", &dstat)
            && !self.t(r"^case\b", &dstat)
            && !self.t(r"^do\s*$Constant\s*while\s*$Constant;?$", &dstat)
            && !self.t(r"^while\s*$Constant\s*$Constant\s*$", &dstat)
            && !self.t(r"^for\s*$Constant$", &dstat)
            && !self.t(r"^for\s*$Constant\s+(?:$Ident|-?$Constant)$", &dstat)
            && !self.t(r"^do\s*{", &dstat)
            && !self.t(r"^\(\{", &dstat)
            && !self.t(
                r"^.\s*#\s*define\s+TRACE_(?:SYSTEM|INCLUDE_FILE|INCLUDE_PATH)\b",
                &ctx,
            )
        {
            if self.t(r"^\s*if\b", &dstat) {
                let msg = cat(&[
                    b"Macros starting with if should be enclosed by a do - while loop to avoid possible if/else logic defects\n",
                    &herectx,
                ]);
                self.error("MULTISTATEMENT_MACRO_USE_DO_WHILE", &msg);
            } else if dstat.contains(&b';') {
                let msg = cat(&[
                    b"Non-declarative macros with multiple statements should be enclosed in a do - while loop\n",
                    &herectx,
                    b"\nBUT SEE:\n",
                    DO_WHILE_0_ADVICE.as_bytes(),
                ]);
                self.warn("MULTISTATEMENT_MACRO_USE_DO_WHILE", &msg);
            } else {
                let msg = cat(&[
                    b"Macros with complex values should be enclosed in parentheses\n",
                    &herectx,
                    b"\nBUT SEE:\n",
                    DO_WHILE_0_ADVICE.as_bytes(),
                ]);
                self.error("COMPLEX_MACRO", &msg);
            }
        }

        // Make $define_stmt single line, comment-free, etc
        let mut stmt: Vec<u8> = Vec::new();
        for (i, mut piece) in split(&self.re(r"\n", ""), &define_stmt)
            .into_iter()
            .enumerate()
        {
            self.s(r"\\$", "", &mut piece, false, &mut |_| Vec::new());
            if i == 0 {
                stmt = piece;
            } else if self.t(r"^[\+ ]", &piece) {
                stmt.extend_from_slice(&piece[1..]);
            }
        }
        stmt.retain(|&b| b != SUBSEP);
        self.s(r"\s+", "", &mut stmt, true, &mut |_| b" ".to_vec());
        let define_stmt = self.trim(&stmt);

        // check if any macro arguments are reused (ignore '...' and 'type')
        for arg in def_args {
            if self.t(r"\.\.\.", &arg) {
                continue;
            }
            if self.tf(r"^type$", "i", &arg) {
                continue;
            }
            let a = [("arg", &arg[..])];
            let mut tmp_stmt = define_stmt.clone();
            let re = self.rel(
                r"\b(__must_be_array|offsetof|sizeof|sizeof_field|__stringify|typeof|__typeof__|__builtin\w+|typecheck\s*\(\s*$Type\s*,|\#+)\s*\(*\s*$arg\s*\)*\b",
                "",
                &a,
            );
            subst(&re, &mut tmp_stmt, true, &mut |_| Vec::new());
            subst(
                &self.rel(r"\#+\s*$arg\b", "", &a),
                &mut tmp_stmt,
                true,
                &mut |_| Vec::new(),
            );
            subst(
                &self.rel(r"\b$arg\s*\#\#", "", &a),
                &mut tmp_stmt,
                true,
                &mut |_| Vec::new(),
            );
            let use_cnt = super::check::all(&self.rel(r"\b$arg\b", "", &a), &tmp_stmt).len();
            if use_cnt > 1 {
                let msg = cat(&[
                    b"Macro argument reuse '",
                    &arg,
                    b"' - possible side-effects?\n",
                    &herectx,
                ]);
                self.chk("MACRO_ARG_REUSE", &msg);
            }
            // check if any macro arguments may have other precedence issues
            if let Some(c) = self.ml(
                r"($Operators)?\s*\b$arg\b\s*($Operators)?",
                "m",
                &a,
                &tmp_stmt,
            ) && (c.get(1).is_some_and(|o| o != b",") || c.get(2).is_some_and(|o| o != b","))
            {
                let msg = cat(&[
                    b"Macro argument '",
                    &arg,
                    b"' may be better as '(",
                    &arg,
                    b")' to avoid precedence issues\n",
                    &herectx,
                ]);
                self.chk("MACRO_ARG_PRECEDENCE", &msg);
            }

            // check if this is an unused argument
            if !self.tl(r"\b$arg\b", &a, &define_stmt) && truthy(&define_stmt) {
                let msg = cat(&[
                    b"Argument '",
                    &arg,
                    b"' is not used in function-like macro\n",
                    &herectx,
                ]);
                self.warn("MACRO_ARG_UNUSED", &msg);
            }
        }

        // check for macros with flow control, but without ## concatenation
        // ## concatenation is commonly a macro that defines a function so ignore those
        if has_flow_statement && !has_arg_concat {
            let cnt = count(&ctx, b'\n') + 1;
            let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
            let msg = cat(&[
                b"Macros with flow control statements should be avoided\n",
                &herectx,
            ]);
            self.warn("MACRO_WITH_FLOW_CONTROL", &msg);
        }
    }

    fn redundant_braces(&mut self, p: &mut P, l: &L) {
        let (level, _, chunks) = self.ctx_statement_full(l.linenr, p.realcnt, 1);
        if chunks.len() <= 1 || level != 0 {
            return;
        }
        let mut allowed: Vec<i64> = Vec::new();
        let mut allow = 0usize;
        let mut seen = 0;
        let mut herectx = cat(&[&l.here, b"\n"]);
        let mut ln = l.linenr - 1;
        for (cond, block) in &chunks {
            // If the condition carries leading newlines, then count those as offsets.
            let whitespace = self
                .mf(r"^((?:\s*\n[+-])*\s*)", "s", cond)
                .map(|c| c.s(1))
                .unwrap_or_default();
            let offset = count(&whitespace, b'\n');
            if allowed.len() <= allow {
                allowed.resize(allow + 1, 0);
            }
            allowed[allow] = 0;

            // We have looked at and allowed this specific line.
            p.suppress_ifbraces.insert(ln + offset);

            herectx.extend(cat(&[
                self.rawline_at(ln + offset).unwrap_or(&[]),
                b"\n[...]\n",
            ]));
            ln += count(block, b'\n');

            let mut block = block.clone();
            block.drain(..cond.len().min(block.len()));

            if self.t(r"^\s*{", &block) {
                seen += 1;
            }
            if self.statement_lines(cond) > 1 {
                allowed[allow] = 1;
            }
            if self.t(r"\b(?:if|for|while)\b", &block) {
                allowed[allow] = 1;
            }
            if self.statement_block_size(&block) > 1 {
                allowed[allow] = 1;
            }
            allow += 1;
        }
        if seen != 0 {
            let sum_allowed: i64 = allowed.iter().sum();
            if sum_allowed == 0 {
                let msg = cat(&[
                    b"braces {} are not necessary for any arm of this statement\n",
                    &herectx,
                ]);
                self.warn("BRACES", &msg);
            } else if sum_allowed != i64::try_from(allow).expect("count") && seen != allow {
                let msg = cat(&[
                    b"braces {} should be used on all arms of this statement\n",
                    &herectx,
                ]);
                self.chk("BRACES", &msg);
            }
        }
    }

    fn single_statement_braces(&mut self, p: &P, l: &L, start: usize) {
        let mut allowed = false;
        let mut off = start;

        // Check the pre-context.
        if let Some(c) = self.m(r"(\}\s*)$", &l.line[..start]) {
            allowed = true;
            // `$-[0]` is now this match's.
            off = c.start(0).expect("match");
        }

        let (level, _, chunks) =
            self.ctx_statement_full(l.linenr, p.realcnt, i64::try_from(off).expect("offset"));

        // Check the condition.
        let (cond, block) = &chunks[0];
        let mut block = block.clone();
        block.drain(..cond.len().min(block.len()));
        if self.statement_lines(cond) > 1 {
            allowed = true;
        }
        if self.t(r"\b(?:if|for|while)\b", &block) {
            allowed = true;
        }
        if self.statement_block_size(&block) > 1 {
            allowed = true;
        }
        // Check the post-context.
        if let Some((cond, block)) = chunks.get(1) {
            let mut block = block.clone();
            block.drain(..cond.len().min(block.len()));
            if self.t(r"^\s*\{", &block) {
                allowed = true;
            }
        }
        if level == 0 && self.t(r"^\s*\{", &block) && !allowed {
            let cnt = count(&block, b'\n') + 1;
            let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
            let msg = cat(&[
                b"braces {} are not necessary for single statement blocks\n",
                &herectx,
            ]);
            self.warn("BRACES", &msg);
        }
    }

    /// The checks of strings, logging and attributes.
    fn string_checks(&mut self, p: &P, l: &L) {
        // Check for user-visible strings broken across lines
        if self.t(r"^\+\s*$String", &l.line)
            && self.t(r#""\s*$"#, &p.prevline)
            && !self.t(
                r#"(?:\\(?:[ntr]|[0-7]{1,3}|x[0-9a-fA-F]{1,2})|;\s*|\{\s*)"\s*$"#,
                &p.prevrawline,
            )
        {
            let msg = cat(&[b"quoted string split across lines\n", &l.hereprev]);
            self.warn("SPLIT_STRING", &msg);
        }

        // check for missing a space in a string concatenation
        if self.t(r#"[^\\]\w"$"#, &p.prevrawline) && self.t(r#"^\+[\t ]+"\w"#, &l.rawline) {
            let msg = cat(&[b"break quoted strings at a space character\n", &l.hereprev]);
            self.warn("MISSING_SPACE", &msg);
        }

        // check for an embedded function name in a string when the function is known
        if self.t(r"^\+.*$String", &l.line)
            && let Some(cf) = p.context_function.clone()
        {
            let quoted = self.get_quoted_string(Some(&l.line), Some(&l.rawline));
            if self.tl(
                r"\b$context_function\b",
                &[("context_function", &cf)],
                &quoted,
            ) && quoted.len() != cf.len() + 2
            {
                let msg = cat(&[
                    b"Prefer using '\"%s...\", __func__' to using '",
                    &cf,
                    b"', this function's name, in a string\n",
                    &l.herecurr,
                ]);
                self.warn("EMBEDDED_FUNCTION_NAME", &msg);
            }
        }

        // check for unnecessary function tracing like uses
        if self.t(
            r#"^\+.*\([^"]*"$tracing_logging_tags{0,3}%s(?:\s*\(\s*\)\s*)?$tracing_logging_tags{0,3}(?:\\n)?"\s*,\s*__func__\s*\)\s*;"#,
            &l.rawline,
        ) {
            let msg = cat(&[b"Unnecessary ftrace-like logging - prefer using ftrace\n", &l.herecurr]);
            self.warn("TRACING_LOGGING", &msg);
        }

        // check for spaces before a quoted newline
        if self.t(r#"^.*\".*\s\\n"#, &l.rawline) {
            let msg = cat(&[
                b"unnecessary whitespace before a quoted newline\n",
                &l.herecurr,
            ]);
            self.warn("QUOTED_WHITESPACE_BEFORE_NEWLINE", &msg);
        }

        // concatenated string without spaces between elements
        if self.check
            && (self.t(r"$String[A-Z_]", &l.line)
                || self
                    .m(r"([A-Za-z0-9_]+)$String", &l.line)
                    .is_some_and(|c| !self.t(r"^[Lu]$", &c.s(1))))
        {
            let msg = cat(&[
                b"Concatenated strings should use spaces between elements\n",
                &l.herecurr,
            ]);
            self.chk("CONCATENATED_STRING", &msg);
        }

        // uncoalesced string fragments
        if self.t(r#"$String\s*[Lu]?""#, &l.line) {
            let msg = cat(&[
                b"Consecutive strings are generally better as a single string\n",
                &l.herecurr,
            ]);
            self.warn("STRING_FRAGMENTS", &msg);
        }

        // check for non-standard and hex prefixed decimal printf formats
        let mut show_l = true;
        let mut show_z = true;
        for c in self.all(r#"(?:^|")([X\t]*)(?:"|$)"#, "", &l.line) {
            let (s, e) = c.c.get(1).expect("group");
            let mut string = substr_s(
                &l.rawline,
                i64::try_from(s).expect("start"),
                Some(i64::try_from(e - s).expect("length")),
            );
            self.s(r"%%", "", &mut string, true, &mut |_| b"__".to_vec());
            // check for %L
            if show_l && let Some(m) = self.m(r"%[\*\d\.\$]*L([diouxX])", &string) {
                let x = m.s(1);
                let msg = cat(&[
                    b"%L",
                    &x,
                    b" is non-standard C, use %ll",
                    &x,
                    b"\n",
                    &l.herecurr,
                ]);
                self.warn("PRINTF_L", &msg);
                show_l = false;
            }
            // check for %Z
            if show_z && let Some(m) = self.m(r"%[\*\d\.\$]*Z([diouxX])", &string) {
                let x = m.s(1);
                let msg = cat(&[
                    b"%Z",
                    &x,
                    b" is non-standard C, use %z",
                    &x,
                    b"\n",
                    &l.herecurr,
                ]);
                self.warn("PRINTF_Z", &msg);
                show_z = false;
            }
            // check for 0x<decimal>
            if self.t(r"0x%[\*\d\.\$\Llzth]*[diou]", &string) {
                let msg = cat(&[
                    b"Prefixing 0x with decimal output is defective\n",
                    &l.herecurr,
                ]);
                self.error("PRINTF_0XDECIMAL", &msg);
            }
        }

        // check for line continuations in quoted strings with odd counts of "
        if self.t(r"\\$", &l.rawline) && count(&l.sline, b'"') % 2 != 0 {
            let msg = cat(&[b"Avoid line continuations in quoted strings\n", &l.herecurr]);
            self.warn("LINE_CONTINUATIONS", &msg);
        }

        // warn about #if 0
        if self.t(r"^.\s*\#\s*if\s+0\b", &l.line) {
            let msg = cat(&[
                b"Consider removing the code enclosed by this #if 0 and its #endif\n",
                &l.herecurr,
            ]);
            self.warn("IF_0", &msg);
        }

        // warn about #if 1
        if self.t(r"^.\s*\#\s*if\s+1\b", &l.line) {
            let msg = cat(&[b"Consider removing the #if 1 and its #endif\n", &l.herecurr]);
            self.warn("IF_1", &msg);
        }

        // check for needless "if (<foo>) fn(<foo>)" uses
        if let Some(c) = self.m(r"\bif\s*\(\s*($Lval)\s*\)", &p.prevline) {
            let tested = quotemeta_bytes(&c.s(1));
            let expr = cat(&[br"\s*\(\s*", &tested, br"\s*\)\s*;"]);
            if let Some(m) = self.ml(
                r"\b(kfree|usb_free_urb|debugfs_remove(?:_recursive)?|(?:kmem_cache|mempool|dma_pool)_destroy)$expr",
                "",
                &[("expr", &expr)],
                &l.line,
            ) {
                let msg = cat(&[
                    &m.s(1),
                    b"(NULL) is safe and this check is probably not required\n",
                    &l.hereprev,
                ]);
                self.warn("NEEDLESS_IF", &msg);
            }
        }

        // check for unnecessary "Out of Memory" messages
        if self.t(r"^\+.*\b$logFunctions\s*\(", &l.line)
            && let Some(c) = self.m(
                r"^[ \+]\s*if\s*\(\s*(\!\s*|NULL\s*==\s*)?($Lval)(\s*==\s*NULL\s*)?\s*\)",
                &p.prevline,
            )
            && (c.defined(1) || c.defined(3))
            && l.linenr > 3
        {
            let testval = c.s(2);
            let b = self.ctx_statement_block(l.linenr - 3, p.realcnt, 0);
            if self.tl(
                r"(?:^|\n)[ \+]\s*(?:$Type\s*)?\Q$testval\E\s*=\s*(?:\([^\)]*\)\s*)?\s*$allocFunctions\s*\(",
                &[("testval", &testval)],
                &b.statement,
            ) && !self.t(r"\b__GFP_NOWARN\b", &b.statement)
            {
                let msg = cat(&[b"Possible unnecessary 'out of memory' message\n", &l.hereprev]);
                self.warn("OOM_MESSAGE", &msg);
            }
        }

        // check for logging functions with KERN_<LEVEL>
        if !self.t(r"printk(?:_ratelimited|_once)?\s*\(", &l.line)
            && let Some(c) = self.m(r"\b$logFunctions\s*\(.*\b(KERN_[A-Z]+)\b", &l.line)
        {
            let msg = cat(&[b"Possible unnecessary ", &c.s(1), b"\n", &l.herecurr]);
            self.warn("UNNECESSARY_KERN_LEVEL", &msg);
        }

        // check for logging continuations
        if self.t(r"\bprintk\s*\(\s*KERN_CONT\b|\bpr_cont\s*\(", &l.line) {
            let msg = cat(&[
                b"Avoid logging continuation uses where feasible\n",
                &l.herecurr,
            ]);
            self.warn("LOGGING_CONTINUATION", &msg);
        }

        // check for unnecessary use of %h[xudi] and %hh[xudi] in logging functions
        if let Some(stat) = l.stat.clone()
            && self.t(r"\b$logFunctions\s*\(", &l.line)
            && stat.contains(&b'"')
        {
            let lc = count(&stat, b'\n') + l.linenr;
            let stat_real = self.get_stat_real(l.linenr, lc);
            let start = match stat_real.iter().position(|&b| b == b'"') {
                Some(i) => i,
                None => stat_real.len().saturating_sub(1),
            };
            let re = self.re(r#"[^\"%]*(%[\#\d\.\*\-]*(h+)[idux])"#, "");
            let mut pos = start;
            let mut last_empty_at = None;
            while pos <= stat_real.len() {
                let Some(c) = re.find_g(&stat_real, pos, last_empty_at) else {
                    break;
                };
                last_empty_at = (c.start() == c.end()).then_some(c.end());
                pos = c.end();
                let pspec = c.str(&stat_real, 1);
                let h = c.str(&stat_real, 2);
                let msg = cat(&[
                    b"Integer promotion: Using '",
                    &h,
                    b"' in '",
                    &pspec,
                    b"' is unnecessary\n",
                    &l.here,
                    b"\n",
                    &stat_real,
                    b"\n",
                ]);
                self.warn("UNNECESSARY_MODIFIER", &msg);
            }
        }

        // check for mask then right shift without a parentheses
        if let Some(c) = self.m(r"$LvalOrFunc\s*\&\s*($LvalOrFunc)\s*>>", &l.line)
            && !self.t(r"^\&", &c.s(4))
        {
            let msg = cat(&[
                b"Possible precedence defect with mask then right shift - may need parentheses\n",
                &l.herecurr,
            ]);
            self.warn("MASK_THEN_SHIFT", &msg);
        }

        // check for pointer comparisons to NULL
        if self.check {
            for c in self.all(r"\b$LvalOrFunc\s*(==|\!=)\s*NULL\b", "", &l.line) {
                let val = c.s(1);
                let equal: &[u8] = if c.s(4) == b"!=" { b"" } else { b"!" };
                let msg = cat(&[
                    b"Comparison to NULL could be written \"",
                    equal,
                    &val,
                    b"\"\n",
                    &l.herecurr,
                ]);
                self.chk("COMPARISON_TO_NULL", &msg);
            }
        }

        // check for bad placement of section $InitAttribute (e.g.: __initdata)
        if let Some(c) = self.m(r"(\b$InitAttribute\b)", &l.line) {
            let attr = c.s(1);
            let a = [("attr", &attr[..])];
            if let Some(m) = self.ml(
                r"^\+\s*static\s+(?:const\s+)?(?:$attr\s+)?($NonptrTypeWithAttr)\s+(?:$attr\s+)?($Ident(?:\[[^]]*\])?)\s*[=;]",
                "",
                &a,
                &l.line,
            ) {
                let ptr = m.s(1);
                let var = m.s(2);
                let msg = cat(&[&attr, b" should be placed after ", &var, b"\n", &l.herecurr]);
                if self.tl(r"\b(union|struct)\s+$attr\b", &a, &ptr) {
                    self.error("MISPLACED_INIT", &msg);
                } else {
                    self.warn("MISPLACED_INIT", &msg);
                }
            }
        }

        // check for $InitAttributeData (ie: __initdata) with const
        if self.t(r"\bconst\b", &l.line)
            && let Some(c) = self.m(r"($InitAttributeData)", &l.line)
        {
            let attr = c.s(1);
            let attr_prefix = self
                .m(r"($InitAttributePrefix)(.*)", &attr)
                .map(|m| m.s(1))
                .unwrap_or_default();
            let msg = cat(&[
                b"Use of const init definition must use ",
                &attr_prefix,
                b"initconst\n",
                &l.herecurr,
            ]);
            self.error("INIT_ATTRIBUTE", &msg);
        }

        // check for $InitAttributeConst (ie: __initconst) without const
        if !self.t(r"\bconst\b", &l.line)
            && let Some(c) = self.m(r"($InitAttributeConst)", &l.line)
        {
            let msg = cat(&[
                b"Use of ",
                &c.s(1),
                b" requires a separate use of const\n",
                &l.herecurr,
            ]);
            self.error("INIT_ATTRIBUTE", &msg);
        }

        // check for __read_mostly with const non-pointer (should just be const)
        if self.t(r"\b__read_mostly\b", &l.line)
            && let Some(c) = self.m(r"($Type)\s*$Ident", &l.line)
            && !self.t(r"\*\s*$", &c.s(1))
            && self.t(r"\bconst\b", &c.s(1))
        {
            let msg = cat(&[
                b"Invalid use of __read_mostly with const type\n",
                &l.herecurr,
            ]);
            self.error("CONST_READ_MOSTLY", &msg);
        }

        // don't use __constant_<foo> functions outside of include/uapi/
        if !self.t(r"^include/uapi/", &p.realfile)
            && let Some(c) = self.m(
                r"(__constant_(?:htons|ntohs|[bl]e(?:16|32|64)_to_cpu|cpu_to_[bl]e(?:16|32|64)))\s*\(",
                &l.line,
            )
        {
            let constant_func = c.s(1);
            let func = constant_func[b"__constant_".len()..].to_vec();
            let msg = cat(&[&constant_func, b" should be ", &func, b"\n", &l.herecurr]);
            self.warn("CONSTANT_CONVERSION", &msg);
        }

        // prefer usleep_range over udelay
        if let Some(c) = self.m(r"\budelay\s*\(\s*(\d+)\s*\)", &l.line) {
            let delay = fnum(&c.s(1));
            if delay >= 10.0 {
                let msg = cat(&[
                    b"usleep_range is preferred over udelay; see function description of usleep_range() and udelay().\n",
                    &l.herecurr,
                ]);
                self.chk("USLEEP_RANGE", &msg);
            }
            if delay > 2000.0 {
                let msg = cat(&[
                    b"long udelay - prefer mdelay; see function description of mdelay().\n",
                    &l.herecurr,
                ]);
                self.warn("LONG_UDELAY", &msg);
            }
        }

        // warn about unexpectedly long msleep's
        if let Some(c) = self.m(r"\bmsleep\s*\((\d+)\);", &l.line)
            && fnum(&c.s(1)) < 20.0
        {
            let msg = cat(&[
                b"msleep < 20ms can sleep for up to 20ms; see function description of msleep().\n",
                &l.herecurr,
            ]);
            self.warn("MSLEEP", &msg);
        }

        // check for comparisons of jiffies
        if self.t(r"\bjiffies\s*$Compare|$Compare\s*jiffies\b", &l.line) {
            let msg = cat(&[
                b"Comparing jiffies is almost always wrong; prefer time_after, time_before and friends\n",
                &l.herecurr,
            ]);
            self.warn("JIFFIES_COMPARISON", &msg);
        }

        // check for comparisons of get_jiffies_64()
        if self.t(
            r"\bget_jiffies_64\s*\(\s*\)\s*$Compare|$Compare\s*get_jiffies_64\s*\(\s*\)",
            &l.line,
        ) {
            let msg = cat(&[
                b"Comparing get_jiffies_64() is almost always wrong; prefer time_after64, time_before64 and friends\n",
                &l.herecurr,
            ]);
            self.warn("JIFFIES_COMPARISON", &msg);
        }

        // warn about spacing in #ifdefs
        if let Some(c) = self.m(r"^.\s*\#\s*(ifdef|ifndef|elif)\s\s+", &l.line) {
            let msg = cat(&[
                b"exactly one space required after that #",
                &c.s(1),
                b"\n",
                &l.herecurr,
            ]);
            self.error("SPACING", &msg);
        }

        // check for spinlock_t definitions without a comment.
        if self.check
            && let Some(c) = self
                .m(r"^.\s*(struct\s+mutex|spinlock_t)\s+\S+;", &l.line)
                .or_else(|| self.m(r"^.\s*(DEFINE_MUTEX)\s*\(", &l.line))
            && !self.ctx_has_comment(p.first_line, l.linenr)
        {
            let msg = cat(&[&c.s(1), b" definition without comment\n", &l.herecurr]);
            self.chk("UNCOMMENTED_DEFINITION", &msg);
        }

        // check for memory barriers without a comment.
        if self.t(r"\b(?:$all_barriers)\s*\(", &l.line)
            && !self.ctx_has_comment(p.first_line, l.linenr)
        {
            let msg = cat(&[b"memory barrier without comment\n", &l.herecurr]);
            self.warn("MEMORY_BARRIER", &msg);
        }

        if !self.t(r"^include/asm-generic/", &p.realfile)
            && !self.t(r"/barrier\.h$", &p.realfile)
            && self.t(r"\b(?:$underscore_smp_barriers)\s*\(", &l.line)
            && !self.t(
                r"^.\s*\#\s*define\s+(?:$underscore_smp_barriers)\s*\(",
                &l.line,
            )
        {
            let msg = cat(&[
                b"__smp memory barriers shouldn't be used outside barrier.h and asm-generic\n",
                &l.herecurr,
            ]);
            self.warn("MEMORY_BARRIER", &msg);
        }

        // check for waitqueue_active without a comment.
        if self.t(r"\bwaitqueue_active\s*\(", &l.line)
            && !self.ctx_has_comment(p.first_line, l.linenr)
        {
            let msg = cat(&[b"waitqueue_active without comment\n", &l.herecurr]);
            self.warn("WAITQUEUE_ACTIVE", &msg);
        }

        // check for data_race without a comment.
        if self.t(r"\bdata_race\s*\(", &l.line) && !self.ctx_has_comment(p.first_line, l.linenr) {
            let msg = cat(&[b"data_race without comment\n", &l.herecurr]);
            self.warn("DATA_RACE", &msg);
        }

        // check for context_unsafe without a comment.
        if self.t(r"\bcontext_unsafe\b", &l.line) && !self.ctx_has_comment(p.first_line, l.linenr) {
            let msg = cat(&[b"context_unsafe without comment\n", &l.herecurr]);
            self.warn("CONTEXT_UNSAFE", &msg);
        }

        // check of hardware specific defines
        if self.check
            && self.t(
                r"^.\s*\#\s*if.*\b(__i386__|__powerpc64__|__sun__|__s390x__)\b",
                &l.line,
            )
            && !self.t(r"include/asm-", &p.realfile)
        {
            let msg = cat(&[
                b"architecture specific defines should be avoided\n",
                &l.herecurr,
            ]);
            self.chk("ARCH_DEFINES", &msg);
        }

        // check that the storage class is not after a type
        if let Some(c) = self.m(r"\b($Type)\s+($Storage)\b", &l.line) {
            let msg = cat(&[
                b"storage class '",
                &c.s(2),
                b"' should be located before type '",
                &c.s(1),
                b"'\n",
                &l.herecurr,
            ]);
            self.warn("STORAGE_CLASS", &msg);
        }
        // Check that the storage class is at the beginning of a declaration
        if self.t(r"\b$Storage\b", &l.line)
            && !self.t(r"^.\s*$Storage", &l.line)
            && let Some(c) = self.m(r"^.\s*(.+?)\$Storage\s", &l.line)
            && !self.t(r"[\,\)]\s*$", &c.s(1))
        {
            let msg = cat(&[
                b"storage class should be at the beginning of the declaration\n",
                &l.herecurr,
            ]);
            self.warn("STORAGE_CLASS", &msg);
        }

        // check the location of the inline attribute, that it is between
        // storage class and type.
        if self.t(r"\b$Type\s+$Inline\b", &l.line) || self.t(r"\b$Inline\s+$Storage\b", &l.line) {
            let msg = cat(&[
                b"inline keyword should sit between storage class and type\n",
                &l.herecurr,
            ]);
            self.error("INLINE_LOCATION", &msg);
        }

        // Check for __inline__ and __inline, prefer inline
        if !self.t(r"\binclude/uapi/", &p.realfile)
            && let Some(c) = self.m(r"\b(__inline__|__inline)\b", &l.line)
        {
            let msg = cat(&[
                b"plain inline is preferred over ",
                &c.s(1),
                b"\n",
                &l.herecurr,
            ]);
            self.warn("INLINE", &msg);
        }

        // Check for compiler attributes
        if !self.t(r"\binclude/uapi/", &p.realfile)
            && let Some(c) = self.m(
                r"\b__attribute__\s*\(\s*($balanced_parens)\s*\)",
                &l.rawline,
            )
        {
            self.compiler_attributes(l, c.s(1));
        }

        // Check for __attribute__ weak, or __weak declarations (may have link issues)
        if self.t(
            r"(?:$Declare|$DeclareMisordered)\s*$Ident\s*$balanced_parens\s*(?:$Attribute)?\s*;",
            &l.line,
        ) && (self.t(r"\b__attribute__\s*\(\s*\(.*\bweak\b", &l.line)
            || self.t(r"\b__weak\b", &l.line))
        {
            let msg = cat(&[
                b"Using weak declarations can have unintended link defects\n",
                &l.herecurr,
            ]);
            self.error("WEAK_DECLARATION", &msg);
        }
    }

    fn compiler_attributes(&mut self, l: &L, attr: Vec<u8>) {
        const ATTR_LIST: &[(&str, &str)] = &[
            ("alias", "__alias"),
            ("aligned", "__aligned"),
            ("always_inline", "__always_inline"),
            ("assume_aligned", "__assume_aligned"),
            ("cold", "__cold"),
            ("const", "__attribute_const__"),
            ("copy", "__copy"),
            ("designated_init", "__designated_init"),
            ("externally_visible", "__visible"),
            ("format", "printf|scanf"),
            ("gnu_inline", "__gnu_inline"),
            ("malloc", "__malloc"),
            ("mode", "__mode"),
            ("no_caller_saved_registers", "__no_caller_saved_registers"),
            ("noclone", "__noclone"),
            ("noinline", "noinline"),
            ("nonstring", "__nonstring"),
            ("noreturn", "__noreturn"),
            ("packed", "__packed"),
            ("pure", "__pure"),
            ("section", "__section"),
            ("used", "__used"),
            ("weak", "__weak"),
        ];
        let mut attr = attr;
        self.s(r"\s*\(\s*(.*)\)\s*", "", &mut attr, false, &mut |c| c.s(1));

        for c in self.all(r"\s*(\w+)\s*(${balanced_parens})?", "", &attr) {
            let orig_attr = c.s(1);
            let params = c.get(2).map(<[u8]>::to_vec).unwrap_or_default();
            let mut curr_attr = orig_attr.clone();
            // A successful substitution leaves no captures.
            let stripped = self.s(r"^[\s_]+|[\s_]+$", "", &mut curr_attr, true, &mut |_| {
                Vec::new()
            }) > 0;
            let Some((_, new)) = ATTR_LIST
                .iter()
                .find(|(k, _)| k.as_bytes() == curr_attr.as_slice())
            else {
                continue;
            };
            let new: Vec<u8> = if curr_attr == b"format" && truthy(&params) {
                if let Some(m) = self.m(r"^\s*\(\s*(\w+)\s*,\s*(.*)", &params) {
                    cat(&[b"__", &m.s(1), b"(", &m.s(2)])
                } else if stripped {
                    b"__(".to_vec()
                } else {
                    cat(&[b"__", &orig_attr, b"(", &params])
                }
            } else {
                cat(&[new.as_bytes(), &params])
            };
            let msg = cat(&[
                b"Prefer ",
                &new,
                b" over __attribute__((",
                &orig_attr,
                &params,
                b"))\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_DEFINED_ATTRIBUTE_MACRO", &msg);
        }

        // Check for __attribute__ unused, prefer __always_unused or __maybe_unused
        if self.t(r"^_*unused", &attr) {
            let msg = cat(&[
                b"__always_unused or __maybe_unused is preferred over __attribute__((__unused__))\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_DEFINED_ATTRIBUTE_MACRO", &msg);
        }
    }

    /// From the c99 types to the sscanf checks.
    fn call_checks(&mut self, p: &P, l: &L) {
        // check for c99 types like uint8_t used outside of uapi/ and tools/
        if self.check
            && !self.t(r"\binclude/uapi/", &p.realfile)
            && !self.t(r"\btools/", &p.realfile)
            && let Some(c) = self.m(r"\b($Declare)\s*$Ident\s*[=;,\[]", &l.line)
            && let Some(m) = self.m(r"\b($typeC99Typedefs)\b", &c.s(1))
        {
            let ty = m.s(1);
            let mut kernel_type = if self.t(r"^_*[si]", &ty) {
                b"s".to_vec()
            } else {
                b"u".to_vec()
            };
            kernel_type.extend(self.m(r"(\d+)", &ty).map(|d| d.s(1)).unwrap_or_default());
            let msg = cat(&[
                b"Prefer kernel type '",
                &kernel_type,
                b"' over '",
                &ty,
                b"'\n",
                &l.herecurr,
            ]);
            self.chk("PREFER_KERNEL_TYPES", &msg);
        }

        // check for cast of C90 native int or longer types constants
        if let Some(c) = self.m(r"(\(\s*$C90_int_types\s*\)\s*)($Constant)\b", &l.line) {
            let cast = c.s(1);
            let constant = c.s(2);
            let mut suffix: Vec<u8> = Vec::new();
            if self.t(r"\bunsigned\b", &cast) {
                suffix.push(b'U');
            }
            if self.t(r"\blong\s+long\b", &cast) {
                suffix.extend_from_slice(b"LL");
            } else if self.t(r"\blong\b", &cast) {
                suffix.push(b'L');
            }
            let msg = cat(&[
                b"Unnecessary typecast of c90 int constant - '",
                &cast,
                &constant,
                b"' could be '",
                &constant,
                &suffix,
                b"'\n",
                &l.herecurr,
            ]);
            self.warn("TYPECAST_INT_CONSTANT", &msg);
        }

        // check for sizeof(&)
        if self.t(r"\bsizeof\s*\(\s*\&", &l.line) {
            let msg = cat(&[b"sizeof(& should be avoided\n", &l.herecurr]);
            self.warn("SIZEOF_ADDRESS", &msg);
        }

        // check for sizeof without parenthesis
        if let Some(c) = self.m(r"\bsizeof\s+((?:\*\s*|)$Lval|$Type(?:\s+$Lval|))", &l.line) {
            let x = c.s(1);
            let msg = cat(&[
                b"sizeof ",
                &x,
                b" should be sizeof(",
                &x,
                b")\n",
                &l.herecurr,
            ]);
            self.warn("SIZEOF_PARENTHESIS", &msg);
        }

        // check for struct spinlock declarations
        if self.t(r"^.\s*\bstruct\s+spinlock\s+\w+\s*;", &l.line) {
            let msg = cat(&[b"struct spinlock should be spinlock_t\n", &l.herecurr]);
            self.warn("USE_SPINLOCK_T", &msg);
        }

        // check for seq_printf uses that could be seq_puts
        if self.t(r#"\bseq_printf\s*\(.*"\s*\)\s*;\s*$"#, &l.sline) {
            let mut fmt = self.get_quoted_string(Some(&l.line), Some(&l.rawline));
            self.s(r"%%", "", &mut fmt, true, &mut |_| Vec::new());
            if !fmt.contains(&b'%') {
                let msg = cat(&[b"Prefer seq_puts to seq_printf\n", &l.herecurr]);
                self.warn("PREFER_SEQ_PUTS", &msg);
            }
        }

        // check for vsprintf extension %p<foo> misuses
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.mf(r"^\+(?![^\{]*\{\s*).*\b(\w+)\s*\(.*$String\s*,", "s", &stat)
            && !self.t(r"^_*volatile_*$", &c.s(1))
        {
            self.vsprintf_extensions(l, &stat);
        }

        // Check for misused memsets
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.m(
                r"^\+(?:.*?)\bmemset\s*\(\s*$FuncArg\s*,\s*$FuncArg\s*\,\s*$FuncArg\s*\)",
                &stat,
            )
        {
            let ms_size = c.s(12);
            if self.mf(r"^(0x|)0$", "i", &ms_size).is_some() {
                let msg = cat(&[
                    b"memset to 0's uses 0 as the 2nd argument, not the 3rd\n",
                    &l.here,
                    b"\n",
                    &stat,
                    b"\n",
                ]);
                self.error("MEMSET", &msg);
            } else if self.mf(r"^(0x|)1$", "i", &ms_size).is_some() {
                let msg = cat(&[
                    b"single byte memset is suspicious. Swapped 2nd/3rd argument?\n",
                    &l.here,
                    b"\n",
                    &stat,
                    b"\n",
                ]);
                self.warn("MEMSET", &msg);
            }
        }

        // strcpy uses that should likely be strscpy
        let userspace = self.t(r"^tools/", &p.realfile) || self.t(r"^scripts/", &p.realfile);
        if self.t(r"\bstrcpy\s*\(", &l.line) && !userspace {
            let msg = cat(&[
                b"Prefer strscpy over strcpy - see: https://github.com/KSPP/linux/issues/88\n",
                &l.herecurr,
            ]);
            self.warn("STRCPY", &msg);
        }

        // strlcpy uses that should likely be strscpy
        if self.t(r"\bstrlcpy\s*\(", &l.line) && !userspace {
            let msg = cat(&[
                b"Prefer strscpy over strlcpy - see: https://github.com/KSPP/linux/issues/89\n",
                &l.herecurr,
            ]);
            self.warn("STRLCPY", &msg);
        }

        // strncpy uses that should likely be strscpy or strscpy_pad
        if self.t(r"\bstrncpy\s*\(", &l.line) && !userspace {
            let msg = cat(&[
                b"Prefer strscpy, strscpy_pad, or __nonstring over strncpy - see: https://github.com/KSPP/linux/issues/90\n",
                &l.herecurr,
            ]);
            self.warn("STRNCPY", &msg);
        }

        // ethtool_sprintf uses that should likely be ethtool_puts
        if self.t(
            r"\bethtool_sprintf\s*\(\s*$FuncArg\s*,\s*$FuncArg\s*\)",
            &l.line,
        ) {
            let msg = cat(&[
                b"Prefer ethtool_puts over ethtool_sprintf with only two arguments\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_ETHTOOL_PUTS", &msg);
        }

        // use $rawline because $line loses %s via sanitization and thus we can't match against it.
        if self.t(
            r#"\bethtool_sprintf\s*\(\s*$FuncArg\s*,\s*\"\%s\"\s*,\s*$FuncArg\s*\)"#,
            &l.rawline,
        ) {
            let msg = cat(&[
                b"Prefer ethtool_puts over ethtool_sprintf with standalone \"%s\" specifier\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_ETHTOOL_PUTS", &msg);
        }

        // typecasts on min/max could be min_t/max_t
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.m(
                r"^\+(?:.*?)\b(min|max)\s*\(\s*$FuncArg\s*,\s*$FuncArg\s*\)",
                &stat,
            )
            && (c.defined(2) || c.defined(7))
        {
            let call = c.s(1);
            let cast1 = self.deparenthesize(c.get(2));
            let arg1 = c.s(3);
            let cast2 = self.deparenthesize(c.get(7));
            let arg2 = c.s(8);
            let cast = if !cast1.is_empty() && !cast2.is_empty() && cast1 != cast2 {
                cat(&[&cast1, b" or ", &cast2])
            } else if !cast1.is_empty() {
                cast1
            } else {
                cast2
            };
            let msg = cat(&[
                &call,
                b"() should probably be ",
                &call,
                b"_t(",
                &cast,
                b", ",
                &arg1,
                b", ",
                &arg2,
                b")\n",
                &l.here,
                b"\n",
                &stat,
                b"\n",
            ]);
            self.warn("MINMAX", &msg);
        }

        // check usleep_range arguments
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.m(
                r"^\+(?:.*?)\busleep_range\s*\(\s*($FuncArg)\s*,\s*($FuncArg)\s*\)",
                &stat,
            )
        {
            let min = c.s(1);
            let max = c.s(7);
            if min == max {
                let msg = cat(&[
                    b"usleep_range should not use min == max args;  see function description of usleep_range().\n",
                    &l.here,
                    b"\n",
                    &stat,
                    b"\n",
                ]);
                self.warn("USLEEP_RANGE", &msg);
            } else if self.t(r"^\d+$", &min) && self.t(r"^\d+$", &max) && fnum(&min) > fnum(&max) {
                let msg = cat(&[
                    b"usleep_range args reversed, use min then max;  see function description of usleep_range().\n",
                    &l.here,
                    b"\n",
                    &stat,
                    b"\n",
                ]);
                self.warn("USLEEP_RANGE", &msg);
            }
        }

        // check for naked sscanf
        if let Some(stat) = l.stat.clone()
            && self.t(r"\bsscanf\b", &l.line)
        {
            if !self.t(r"$Ident\s*=\s*sscanf\s*$balanced_parens", &stat)
                && !self.t(r"\bsscanf\s*$balanced_parens\s*(?:$Compare)", &stat)
                && !self.t(r"(?:$Compare)\s*\bsscanf\s*$balanced_parens", &stat)
            {
                let lc = count(&stat, b'\n') + l.linenr;
                let stat_real = self.get_stat_real(l.linenr, lc);
                let msg = cat(&[
                    b"unchecked sscanf return value\n",
                    &l.here,
                    b"\n",
                    &stat_real,
                    b"\n",
                ]);
                self.warn("NAKED_SSCANF", &msg);
            }

            // check for simple sscanf that should be kstrto<foo>
            let lc = count(&stat, b'\n') + l.linenr;
            let stat_real = self.get_stat_real(l.linenr, lc);
            if let Some(c) = self.m(r#"\bsscanf\b\s*\(\s*$FuncArg\s*,\s*("[^"]+")"#, &stat_real) {
                let format = c.s(6);
                if count(&format, b'%') == 1
                    && self.t(
                        r#"^"\%(?i:ll[udxi]|[udxi]ll|ll|[hl]h?[udxi]|[udxi][hl]h?|[hl]h?|[udxi])"$"#,
                        &format,
                    )
                {
                    let msg = cat(&[b"Prefer kstrto<type> to single variable sscanf\n", &l.here, b"\n", &stat_real, b"\n"]);
                    self.warn("SSCANF_TO_KSTRTO", &msg);
                }
            }
        }
    }

    fn vsprintf_extensions(&mut self, l: &L, stat: &[u8]) {
        let mut stat_real: Option<Vec<u8>> = None;
        let lc = count(stat, b'\n') + l.linenr;
        let mut cnt = l.linenr;
        while cnt <= lc {
            let mut bad_specifier: Vec<u8> = Vec::new();
            let line = self.line_at(cnt - 1).map(<[u8]>::to_vec);
            let raw = self.raw_line(cnt, 0);
            let mut fmt = self.get_quoted_string(line.as_deref(), raw.as_deref());
            self.s(r"%%", "", &mut fmt, true, &mut |_| Vec::new());
            for c in self.all(r"(\%[\*\d\.]*p(\w)(\w*))", "", &fmt) {
                let specifier = c.s(1);
                let extension = c.s(2);
                let qualifier = c.s(3);
                if !self.t(r"[4SsBKRraEehMmIiUDdgVCbGNOxtf]", &extension)
                    || (extension == b"f" && !self.t(r"^w", &qualifier))
                    || (extension == b"4" && !self.t(r"^c(?:[hlbc]|hR)$", &qualifier))
                {
                    bad_specifier = specifier;
                    break;
                }
                if extension == b"x" && stat_real.is_none() {
                    let real = self.get_stat_real(l.linenr, lc);
                    let msg = cat(&[
                        b"Using vsprintf specifier '%px' potentially exposes the kernel memory layout, if you don't really need the address please consider using '%p'.\n",
                        &l.here,
                        b"\n",
                        &real,
                        b"\n",
                    ]);
                    stat_real = Some(real);
                    self.warn("VSPRINTF_SPECIFIER_PX", &msg);
                }
            }
            if !bad_specifier.is_empty() {
                let real = self.get_stat_real(l.linenr, lc);
                let mut error = false;
                let mut use_: Vec<u8> = Vec::new();
                if self.t(r"p[Ff]", &bad_specifier) {
                    use_ = b" - use %pS instead".to_vec();
                    if self.t(r"pf", &bad_specifier) {
                        use_ = b" - use %ps instead".to_vec();
                    }
                } else if self.t(r"pA", &bad_specifier) {
                    use_ = b" - '%pA' is only intended to be used from Rust code".to_vec();
                    error = true;
                }
                let msg = cat(&[
                    b"Invalid vsprintf pointer extension '",
                    &bad_specifier,
                    b"'",
                    &use_,
                    b"\n",
                    &l.here,
                    b"\n",
                    &real,
                    b"\n",
                ]);
                if error {
                    self.error("VSPRINTF_POINTER_EXTENSION", &msg);
                } else {
                    self.warn("VSPRINTF_POINTER_EXTENSION", &msg);
                }
            }
            cnt += 1;
        }
    }

    /// From the extern checks to the end of the loop.
    #[allow(clippy::too_many_lines)] // One check after another, in checkpatch's order.
    fn declaration_checks(&mut self, p: &mut P, l: &L) {
        // check for new externs in .h files.
        if self.check
            && self.t(r"\.h$", &p.realfile)
            && self.tf(r"^\+\s*(extern\s+)$Type\s*$Ident\s*\(", "s", &l.line)
        {
            let msg = cat(&[
                b"extern prototypes should be avoided in .h files\n",
                &l.herecurr,
            ]);
            self.chk("AVOID_EXTERNS", &msg);
        }

        // check for new externs in .c files.
        if self.t(r"\.c$", &p.realfile)
            && let Some(stat) = l.stat.clone()
        {
            if let Some(c) = self.mf(r"^.\s*(?:extern\s+)?$Type\s+($Ident)(\s*)\(", "s", &stat) {
                let paren_space = c.s(2);
                let mut s = stat.clone();
                if let Some(cond) = &l.cond {
                    s.drain(..cond.len().min(s.len()));
                }
                if self.t(r"^\s*;", &s) {
                    let msg = cat(&[b"externs should be avoided in .c files\n", &l.herecurr]);
                    self.warn("AVOID_EXTERNS", &msg);
                }
                if paren_space.contains(&b'\n') {
                    let msg = cat(&[
                        b"arguments for function declarations should follow identifier\n",
                        &l.herecurr,
                    ]);
                    self.warn("FUNCTION_ARGUMENTS", &msg);
                }
            } else if let Some(c) = self.m(r"^\+extern struct\s+(\w+)\s+(\w+)\[\];", &stat) {
                let st_type = c.s(1);
                let st_name = c.s(2);
                let symbols: Vec<Vec<u8>> = self.maybe_linker_symbol.keys().cloned().collect();
                let linker = symbols
                    .iter()
                    .any(|s| self.tl(r"$s", &[("s", s)], &st_name));
                if !linker {
                    let msg = cat(&[
                        b"found a file-scoped extern type:",
                        &st_type,
                        b" name:",
                        &st_name,
                        b" in .c file\nis this a linker symbol ?\n",
                        &l.herecurr,
                    ]);
                    self.warn("AVOID_EXTERNS", &msg);
                }
            } else if self.t(r"^.\s*extern\s+", &stat) {
                let msg = cat(&[b"externs should be avoided in .c files\n", &l.herecurr]);
                self.warn("AVOID_EXTERNS", &msg);
            }
        }

        // check for function declarations that have arguments without identifier names
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.mf(
                r"^.\s*(?:extern\s+)?$Type\s*(?:$Ident|\(\s*\*\s*$Ident\s*\))\s*\(\s*([^{]+)\s*\)\s*;",
                "s",
                &stat,
            )
            && c.s(1) != b"void"
        {
            let args = self.trim(&c.s(1));
            for m in self.all(r"\s*($Type\s*(?:$Ident|\(\s*\*\s*$Ident?\s*\)\s*$balanced_parens)?)", "", &args) {
                let arg = self.trim(&m.s(1));
                if self.t(r"^$Type$", &arg) && !self.t(r"enum\s+$Ident$", &arg) {
                    let msg = cat(&[
                        b"function definition argument '",
                        &arg,
                        b"' should also have an identifier name\n",
                        &l.herecurr,
                    ]);
                    self.warn("FUNCTION_ARGUMENTS", &msg);
                }
            }
        }

        // check for function definitions
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.mf(
                r"^.\s*(?:$Storage\s+)?$Type\s*($Ident)\s*$balanced_parens\s*{",
                "s",
                &stat,
            )
        {
            p.context_function = Some(c.s(1));

            // check for multiline function definition with misplaced open brace
            let mut ok = false;
            let cnt = count(&stat, b'\n') + 1;
            let mut herectx = cat(&[&l.here, b"\n"]);
            for k in 0..cnt {
                let rl = self.raw_line(l.linenr, k).unwrap_or_default();
                herectx.extend(cat(&[&rl, b"\n"]));
                if self.t(r"^[ \+]\{", &rl) {
                    ok = true;
                }
                if rl.contains(&b'{') && k == 0 {
                    ok = true;
                }
                if self.t(r"^[ \+].*\{", &rl) {
                    break;
                }
            }
            if !ok {
                let msg = cat(&[
                    b"open brace '{' following function definitions go on the next line\n",
                    &herectx,
                ]);
                self.error("OPEN_BRACE", &msg);
            }
        }

        // checks for new __setup's
        if self.check && self.t(r#"\b__setup\("([^"]*)""#, &l.rawline) {
            let msg = cat(&[
                b"__setup appears un-documented -- check Documentation/admin-guide/kernel-parameters.txt\n",
                &l.herecurr,
            ]);
            self.chk("UNDOCUMENTED_SETUP", &msg);
        }

        // check for pointless casting of alloc functions
        if self.t(r"\*\s*\)\s*$allocFunctions\b", &l.line) {
            let msg = cat(&[
                b"unnecessary cast may hide bugs, see http://c-faq.com/malloc/mallocnocast.html\n",
                &l.herecurr,
            ]);
            self.warn("UNNECESSARY_CASTS", &msg);
        }

        // alloc style
        if self.check
            && let Some(c) = self.m(
                r"\b($Lval)\s*\=\s*(?:$balanced_parens)?\s*((?:kv|k|v)[mz]alloc(?:_node)?)\s*\(\s*(sizeof\s*\(\s*struct\s+$Lval\s*\))",
                &l.line,
            )
        {
            let msg = cat(&[
                b"Prefer ",
                &c.s(3),
                b"(sizeof(*",
                &c.s(1),
                b")...) over ",
                &c.s(3),
                b"(",
                &c.s(4),
                b"...)\n",
                &l.herecurr,
            ]);
            self.chk("ALLOC_SIZEOF_STRUCT", &msg);
        }

        // check for (kv|k)[mz]alloc that could be kmalloc_obj/kvmalloc_obj/kzalloc_obj/kvzalloc_obj
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.m(
                r"^\+\s*($Lval)\s*\=\s*(?:$balanced_parens)?\s*((?:kv|k)[mz]alloc)\s*\(\s*($FuncArg)\s*,",
                &stat,
            )
        {
            let oldfunc = c.s(3);
            let mut a1 = c.s(4);
            let newfunc: &[u8] = match oldfunc.as_slice() {
                b"kvmalloc" => b"kvmalloc_obj",
                b"kvzalloc" => b"kvzalloc_obj",
                b"kzalloc" => b"kzalloc_obj",
                _ => b"kmalloc_obj",
            };
            if self.s(r"^sizeof\s*\S\(?([^\)]*)\)?$", "", &mut a1, false, &mut |c| c.s(1)) > 0 {
                let cnt = count(&stat, b'\n') + 1;
                let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
                let msg = cat(&[b"Prefer ", newfunc, b" over ", &oldfunc, b" with sizeof\n", &herectx]);
                self.warn("ALLOC_WITH_SIZEOF", &msg);
            }
        }

        // check for (kv|k)[mz]alloc with multiplies that could be kmalloc_objs/kvmalloc_objs/kzalloc_objs/kvzalloc_objs
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.m(
                r"^\+\s*($Lval)\s*\=\s*(?:$balanced_parens)?\s*((?:kv|k)[mz]alloc)\s*\(\s*($FuncArg)\s*\*\s*($FuncArg)\s*,",
                &stat,
            )
        {
            let oldfunc = c.s(3);
            let a1 = c.s(4);
            let a2 = c.s(10);
            let newfunc: &[u8] = match oldfunc.as_slice() {
                b"kvmalloc" => b"kvmalloc_objs",
                b"kvzalloc" => b"kvzalloc_objs",
                b"kzalloc" => b"kzalloc_objs",
                _ => b"kmalloc_objs",
            };
            let (r1, r2) = if self.t(r"^sizeof\s*\S", &a1) { (a2, a1) } else { (a1, a2) };
            if !self.t(r"^sizeof\b", &r1)
                && self.t(r"^sizeof\s*\S", &r2)
                && !(self.t(r"^$Constant$", &r1) || self.t(r"^[A-Z_][A-Z0-9_]*$", &r1))
            {
                let cnt = count(&stat, b'\n') + 1;
                let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
                let msg = cat(&[b"Prefer ", newfunc, b" over ", &oldfunc, b" with multiply\n", &herectx]);
                self.warn("ALLOC_WITH_MULTIPLY", &msg);
            }
        }

        // check for krealloc arg reuse
        if let Some(c) = self.m(
            r"\b($Lval)\s*\=\s*(?:$balanced_parens)?\s*krealloc\s*\(\s*($Lval)\s*,",
            &l.line,
        ) && c.s(1) == c.s(3)
        {
            let msg = cat(&[
                b"Reusing the krealloc arg is almost always a bug\n",
                &l.herecurr,
            ]);
            self.warn("KREALLOC_ARG_REUSE", &msg);
        }

        // check for alloc argument mismatch
        if let Some(c) = self.m(
            r"\b((?:devm_)?((?:k|kv)?(calloc|malloc_array)(?:_node)?))\s*\(\s*sizeof\b",
            &l.line,
        ) {
            let msg = cat(&[
                &c.s(1),
                b" uses number as first arg, sizeof is generally wrong\n",
                &l.herecurr,
            ]);
            self.warn("ALLOC_ARRAY_ARGS", &msg);
        }

        // check for multiple semicolons
        if self.t(r";\s*;\s*$", &l.line) {
            let msg = cat(&[b"Statements terminations use 1 semicolon\n", &l.herecurr]);
            self.warn("ONE_SEMICOLON", &msg);
        }

        // check for #defines like: 1 << <digit> that could be BIT(digit), it is not exported to uapi
        if self.check
            && !self.t(r"^include/uapi/", &p.realfile)
            && let Some(c) = self.m(
                r"#\s*define\s+\w+\s+\(?\s*1\s*([ulUL]*)\s*\<\<\s*(?:\d+|$Ident)\s*\)?",
                &l.line,
            )
        {
            let ull: &[u8] = if self.mf(r"ll", "i", &c.s(1)).is_some() {
                b"_ULL"
            } else {
                b""
            };
            let msg = cat(&[b"Prefer using the BIT", ull, b" macro\n", &l.herecurr]);
            self.chk("BIT_MACRO", &msg);
        }

        // check for IS_ENABLED() without CONFIG_<FOO> ($rawline for comments too)
        if let Some(c) = self.m(r"\bIS_ENABLED\s*\(\s*(\w+)\s*\)", &l.rawline)
            && !self.t(r"^${CONFIG_}", &c.s(1))
        {
            let msg = cat(&[
                b"IS_ENABLED(",
                &c.s(1),
                b") is normally used as IS_ENABLED(CONFIG_",
                &c.s(1),
                b")\n",
                &l.herecurr,
            ]);
            self.warn("IS_ENABLED_CONFIG", &msg);
        }

        // check for #if defined CONFIG_<FOO> || defined CONFIG_<FOO>_MODULE
        if self.t(
            r"^\+\s*#\s*if\s+defined(?:\s*\(?\s*|\s+)(${CONFIG_}[A-Z_]+)\s*\)?\s*\|\|\s*defined(?:\s*\(?\s*|\s+)\1_MODULE\s*\)?\s*$",
            &l.line,
        ) {
            let msg = cat(&[
                b"Prefer IS_ENABLED(<FOO>) to CONFIG_<FOO> || CONFIG_<FOO>_MODULE\n",
                &l.herecurr,
            ]);
            self.warn("PREFER_IS_ENABLED", &msg);
        }

        // check for /* fallthrough */ like comment, prefer fallthrough;
        if self.check && !l.raw_comment.is_empty() {
            const FALLTHROUGHS: &[&str] = &[
                r"fallthrough",
                r"@fallthrough@",
                r"lint -fallthrough[ \t]*",
                r"intentional(?:ly)?[ \t]*fall(?:(?:s | |-)[Tt]|t)hr(?:ough|u|ew)",
                r"(?:else,?\s*)?FALL(?:S | |-)?THR(?:OUGH|U|EW)[ \t.!]*(?:-[^\n\r]*)?",
                r"Fall(?:(?:s | |-)[Tt]|t)hr(?:ough|u|ew)[ \t.!]*(?:-[^\n\r]*)?",
                r"fall(?:s | |-)?thr(?:ough|u|ew)[ \t.!]*(?:-[^\n\r]*)?",
            ];
            for ft in FALLTHROUGHS {
                if self.t(ft, &l.raw_comment) {
                    let msg = cat(&[
                        b"Prefer 'fallthrough;' over fallthrough comment\n",
                        &l.herecurr,
                    ]);
                    self.chk("PREFER_FALLTHROUGH", &msg);
                    break;
                }
            }
        }

        // check for switch/default statements without a break;
        if let Some(stat) = l.stat.clone()
            && self.t(
                r"^\+[$;\s]*(?:case[$;\s]+\w+[$;\s]*:[$;\s]*|)*[$;\s]*\bdefault[$;\s]*:[$;\s]*;",
                &stat,
            )
        {
            let cnt = count(&stat, b'\n') + 1;
            let herectx = self.get_stat_here(l.linenr, cnt, &l.here);
            let msg = cat(&[b"switch default: should use break\n", &herectx]);
            self.warn("DEFAULT_NO_BREAK", &msg);
        }

        // check for gcc specific __FUNCTION__
        if self.t(r"\b__FUNCTION__\b", &l.line) {
            let msg = cat(&[
                b"__func__ should be used instead of gcc specific __FUNCTION__\n",
                &l.herecurr,
            ]);
            self.warn("USE_FUNC", &msg);
        }

        // check for uses of __DATE__, __TIME__, __TIMESTAMP__
        for c in self.all(r"\b(__(?:DATE|TIME|TIMESTAMP)__)\b", "", &l.line) {
            let msg = cat(&[
                b"Use of the '",
                &c.s(1),
                b"' macro makes the build non-deterministic\n",
                &l.herecurr,
            ]);
            self.error("DATE_TIME", &msg);
        }

        // check for use of yield()
        if self.t(r"\byield\s*\(\s*\)", &l.line) {
            let msg = cat(&[
                b"Using yield() is generally wrong. See yield() kernel-doc (sched/core.c)\n",
                &l.herecurr,
            ]);
            self.warn("YIELD", &msg);
        }

        // check for comparisons against true and false
        if self.check
            && let Some(c) = self.mf(
                r"\+\s*(.*?)\b(true|false|$Lval)\s*(==|\!=)\s*(true|false|$Lval)\b(.*)$",
                "i",
                &l.line,
            )
        {
            let mut otype = c.s(4);
            let arg = c.s(2);
            if self.mf(r"^(?:true|false)$", "i", &arg).is_some() {
                otype = arg;
            }
            let ty = otype.to_ascii_lowercase();
            if self.t(r"^(?:true|false)$", &ty) {
                let msg = cat(&[
                    b"Using comparison to ",
                    &otype,
                    b" is error prone\n",
                    &l.herecurr,
                ]);
                self.chk("BOOL_COMPARISON", &msg);
            }
        }

        // check for semaphores initialized locked
        if self.t(r"^.\s*sema_init.+,\W?0\W?\)", &l.line) {
            let msg = cat(&[b"consider using a completion\n", &l.herecurr]);
            self.warn("CONSIDER_COMPLETION", &msg);
        }

        // recommend kstrto* over simple_strto* and strict_strto*
        if let Some(c) = self.m(r"\b((simple|strict)_(strto(l|ll|ul|ull)))\s*\(", &l.line) {
            let msg = cat(&[
                &c.s(1),
                b" is obsolete, use k",
                &c.s(3),
                b" instead\n",
                &l.herecurr,
            ]);
            self.warn("CONSIDER_KSTRTO", &msg);
        }

        // check for __initcall(), use device_initcall() explicitly or more appropriate function please
        if self.t(r"^.\s*__initcall\s*\(", &l.line) {
            let msg = cat(&[
                b"please use device_initcall() or more appropriate function instead of __initcall() (see include/linux/init.h)\n",
                &l.herecurr,
            ]);
            self.warn("USE_DEVICE_INITCALL", &msg);
        }

        // check for spin_is_locked(), suggest lockdep instead
        if self.t(r"\bspin_is_locked\(", &l.line) {
            let msg = cat(&[
                b"Where possible, use lockdep_assert_held instead of assertions based on spin_is_locked\n",
                &l.herecurr,
            ]);
            self.warn("USE_LOCKDEP", &msg);
        }

        // check for deprecated apis
        if let Some(c) = self.m(r"\b($deprecated_apis_search)\b\s*\(", &l.line) {
            let api = c.s(1);
            let new_api = DEPRECATED_APIS
                .iter()
                .find(|(k, _)| k.as_bytes() == api.as_slice())
                .map(|(_, v)| *v)
                .unwrap_or_default();
            let msg = cat(&[
                b"Deprecated use of '",
                &api,
                b"', prefer '",
                new_api.as_bytes(),
                b"' instead\n",
                &l.herecurr,
            ]);
            self.warn("DEPRECATED_API", &msg);
        }

        // use of NR_CPUS is usually wrong
        if self.t(r"\bNR_CPUS\b", &l.line)
            && !self.t(r"^.\s*\s*#\s*if\b.*\bNR_CPUS\b", &l.line)
            && !self.t(r"^.\s*\s*#\s*define\b.*\bNR_CPUS\b", &l.line)
            && !self.t(r"^.\s*$Declare\s.*\[[^\]]*NR_CPUS[^\]]*\]", &l.line)
            && !self.t(r"\[[^\]]*\.\.\.[^\]]*NR_CPUS[^\]]*\]", &l.line)
            && !self.t(r"\[[^\]]*NR_CPUS[^\]]*\.\.\.[^\]]*\]", &l.line)
            && !self.t(r"^.\s*\.\w+\s*=\s*.*\bNR_CPUS\b", &l.line)
        {
            let msg = cat(&[
                b"usage of NR_CPUS is often wrong - consider using cpu_possible(), num_possible_cpus(), for_each_possible_cpu(), etc\n",
                &l.herecurr,
            ]);
            self.warn("NR_CPUS", &msg);
        }

        // Use of __ARCH_HAS_<FOO> or ARCH_HAVE_<BAR> is wrong.
        if let Some(c) = self.m(
            r"\+\s*#\s*define\s+((?:__)?ARCH_(?:HAS|HAVE)\w*)\b",
            &l.line,
        ) {
            let msg = cat(&[
                b"#define of '",
                &c.s(1),
                b"' is wrong - use Kconfig variables or standard guards instead\n",
                &l.herecurr,
            ]);
            self.error("DEFINE_ARCH_HAS", &msg);
        }

        // likely/unlikely comparisons similar to "(likely(foo) > 0)"
        if let Some(c) = self.m(
            r"\b((?:un)?likely)\s*\(\s*$FuncArg\s*\)\s*$Compare",
            &l.line,
        ) {
            let msg = cat(&[
                b"Using ",
                &c.s(1),
                b" should generally have parentheses around the comparison\n",
                &l.herecurr,
            ]);
            self.warn("LIKELY_MISUSE", &msg);
        }

        // return sysfs_emit(foo, fmt, ...) fmt without newline
        if let Some(c) = self.m(
            r"\breturn\s+sysfs_emit\s*\(\s*$FuncArg\s*,\s*($String)",
            &l.line,
        ) {
            let (s, e) = c.c.get(6).expect("group");
            let fmt = substr_s(
                &l.rawline,
                i64::try_from(s).expect("start"),
                Some(i64::try_from(e - s).expect("length")),
            );
            if !self.t(r#"\\n"$"#, &fmt) {
                let msg = cat(&[
                    b"return sysfs_emit(...) formats should include a terminating newline\n",
                    &l.herecurr,
                ]);
                self.warn("SYSFS_EMIT", &msg);
            }
        }

        // check for array definition/declarations that should use flexible arrays instead
        if self.t(r"^[\+ ]\s*\}(?:\s*__packed)?\s*;\s*$", &l.sline)
            && self.t(
                r"^\+\s*(?:\}(?:\s*__packed\s*)?|$Type)\s*$Ident\s*\[\s*(0|1)\s*\]\s*;\s*$",
                &p.prevline,
            )
        {
            let msg = cat(&[
                b"Use C99 flexible arrays - see https://docs.kernel.org/process/deprecated.html#zero-length-and-one-element-arrays\n",
                &l.hereprev,
            ]);
            self.error("FLEXIBLE_ARRAY", &msg);
        }

        // nested likely/unlikely calls
        if let Some(c) = self.m(
            r"\b(?:(?:un)?likely)\s*\(\s*!?\s*(IS_ERR(?:_OR_NULL|_VALUE)?|WARN)",
            &l.line,
        ) {
            let msg = cat(&[
                b"nested (un)?likely() calls, ",
                &c.s(1),
                b" already uses unlikely() internally\n",
                &l.herecurr,
            ]);
            self.warn("LIKELY_MISUSE", &msg);
        }

        // whine mightly about in_atomic
        if self.t(r"\bin_atomic\s*\(", &l.line) {
            if self.t(r"^drivers/", &p.realfile) {
                let msg = cat(&[b"do not use in_atomic in drivers\n", &l.herecurr]);
                self.error("IN_ATOMIC", &msg);
            } else if !self.t(r"^kernel/", &p.realfile) {
                let msg = cat(&[
                    b"use of in_atomic() is incorrect outside core kernel code\n",
                    &l.herecurr,
                ]);
                self.warn("IN_ATOMIC", &msg);
            }
        }

        // Complain about RCU Tasks Trace used outside of BPF (and of course, RCU).
        if self.t(r"\b($rcu_trace_funcs)\s*\(", &l.line)
            && !self.t(r"^$rcu_trace_paths", &p.realfile)
        {
            let msg = cat(&[
                b"use of RCU tasks trace is incorrect outside BPF or core RCU code\n",
                &l.herecurr,
            ]);
            self.warn("RCU_TASKS_TRACE", &msg);
        }

        // check for lockdep_set_novalidate_class
        if (self.t(r"^.\s*lockdep_set_novalidate_class\s*\(", &l.line)
            || self.t(r"__lockdep_no_validate__\s*\)", &l.line))
            && !self.t(r"^kernel/lockdep", &p.realfile)
            && !self.t(r"^include/linux/lockdep", &p.realfile)
            && !self.t(r"^drivers/base/core", &p.realfile)
        {
            let msg = cat(&[
                b"lockdep_no_validate class is reserved for device->mutex.\n",
                &l.herecurr,
            ]);
            self.error("LOCKDEP", &msg);
        }

        if self.t(
            r"debugfs_create_\w+.*\b$mode_perms_world_writable\b",
            &l.line,
        ) || self.t(r"DEVICE_ATTR.*\b$mode_perms_world_writable\b", &l.line)
        {
            let msg = cat(&[
                b"Exporting world writable files is usually an error. Consider more restrictive permissions.\n",
                &l.herecurr,
            ]);
            self.warn("EXPORTED_WORLD_WRITABLE", &msg);
        }

        // check for DEVICE_ATTR uses that could be DEVICE_ATTR_<FOO>
        if let Some(stat) = l.stat.clone()
            && let Some(c) = self.m(
                r"\bDEVICE_ATTR\s*\(\s*(\w+)\s*,\s*\(?\s*(\s*(?:${multi_mode_perms_string_search}|0[0-7]{3,3})\s*)\s*\)?\s*,\s*(\w+)\s*,\s*(\w+)\s*\)",
                &stat,
            )
        {
            self.device_attr(l, &c.s(1), &c.s(2), &c.s(3), &c.s(4));
        }

        // Mode permission misuses where it seems decimal should be octal
        if let Some(stat) = l.stat.clone()
            && self.t(r"$mode_perms_search", &l.line)
        {
            self.mode_permissions(l, &stat);
        }

        // check for uses of S_<PERMS> that could be octal for readability
        for c in self.all(r"\b($multi_mode_perms_string_search)\b", "", &l.line) {
            let oval = c.s(1);
            let octal = self.perms_to_octal(&oval);
            let msg = cat(&[
                b"Symbolic permissions '",
                &oval,
                b"' are not preferred. Consider using octal permissions '",
                &octal,
                b"'.\n",
                &l.herecurr,
            ]);
            self.warn("SYMBOLIC_PERMS", &msg);
        }

        // validate content of MODULE_LICENSE against list from include/linux/module.h
        if self.t(r"\bMODULE_LICENSE\s*\(\s*($String)\s*\)", &l.line) {
            let extracted = self.get_quoted_string(Some(&l.line), Some(&l.rawline));
            if !self.tf(r#"^"(?:$valid_licenses)"$"#, "x", &extracted) {
                let msg = cat(&[b"unknown module license ", &extracted, b"\n", &l.herecurr]);
                self.warn("MODULE_LICENSE", &msg);
            }
        }

        // check for sysctl duplicate constants
        if let Some(c) = self.m(r"\.extra[12]\s*=\s*&(zero|one|int_max)\b", &l.line) {
            let msg = cat(&[
                b"duplicated sysctl range checking value '",
                &c.s(1),
                b"', consider using the shared one in include/linux/sysctl.h\n",
                &l.herecurr,
            ]);
            self.warn("DUPLICATED_SYSCTL_CONST", &msg);
        }

        // Check that *_device_id tables have sentinel entries.
        if let Some(stat) = l.stat.clone()
            && self.t(r"struct\s+$dev_id_types\s+\w+\s*\[\s*\]\s*=\s*\{", &l.line)
        {
            let mut stripped = stat.clone();
            self.s(r"(^|\n).", "", &mut stripped, true, &mut |c| c.s(1));
            self.s(r"\\\n", "", &mut stripped, true, &mut |_| b"\n".to_vec());
            self.s(r#""""#, "", &mut stripped, true, &mut |_| Vec::new());
            self.s(r"0x0", "", &mut stripped, true, &mut |_| Vec::new());
            self.s(r"[\s$;,0]", "", &mut stripped, true, &mut |_| Vec::new());
            self.s(r"\.$Ident=", "", &mut stripped, true, &mut |_| Vec::new());
            if !(substr(&stripped, -4, None) == Some(b"{}};")
                || substr(&stripped, -6, None) == Some(b"{{}}};")
                || self.t(r"ISAPNP_DEVICE_SINGLE_END}};$", &stripped)
                || self.t(r"ISAPNP_CARD_END}};$", &stripped)
                || self.t(r"NULL};$", &stripped)
                || self.t(r"PCMCIA_DEVICE_NULL};$", &stripped))
            {
                let msg = cat(&[
                    b"missing sentinel in ID array\n",
                    &l.here,
                    b"\n",
                    &stat,
                    b"\n",
                ]);
                self.error("MISSING_SENTINEL", &msg);
            }
        }

        // check for uninitialized pointers with __free attribute
        for c in self.all(
            r"\*\s*($Ident)\s+__free\s*\(\s*$Ident\s*\)\s*[,;]",
            "",
            &l.line,
        ) {
            let msg = cat(&[
                b"pointer '",
                &c.s(1),
                b"' with __free attribute should be initialized\n",
                &l.herecurr,
            ]);
            self.error("UNINITIALIZED_PTR_WITH_FREE", &msg);
        }
    }

    fn device_attr(&mut self, l: &L, var: &[u8], perms: &[u8], show: &[u8], store: &[u8]) {
        let octal_perms = self.perms_to_octal(perms);
        let v = [("var", var)];
        let is_show = self.tl(r"^${var}_show$", &v, show);
        let is_store = self.tl(r"^${var}_store$", &v, store);
        let warn = |this: &mut Self, ty: &str, what: &[u8]| {
            let msg = cat(&[what, &l.herecurr]);
            this.warn(ty, &msg);
        };
        if is_show && is_store && octal_perms == b"0644" {
            warn(self, "DEVICE_ATTR_RW", b"Use DEVICE_ATTR_RW\n");
        } else if is_show && self.t(r"^NULL$", store) && octal_perms == b"0444" {
            warn(self, "DEVICE_ATTR_RO", b"Use DEVICE_ATTR_RO\n");
        } else if self.t(r"^NULL$", show) && is_store && octal_perms == b"0200" {
            warn(self, "DEVICE_ATTR_WO", b"Use DEVICE_ATTR_WO\n");
        } else if octal_perms == b"0644" || octal_perms == b"0444" || octal_perms == b"0200" {
            let var_show = cat(&[var, b"_show"]);
            let var_store = cat(&[var, b"_store"]);
            let newshow = if show != b"NULL" && show != var_show.as_slice() {
                var_show
            } else {
                show.to_vec()
            };
            let newstore = if store != b"NULL" && store != var_store.as_slice() {
                var_store
            } else {
                store.to_vec()
            };
            let mut rename: Vec<u8> = Vec::new();
            if show != newshow.as_slice() {
                rename.extend(cat(&[b" '", show, b"' to '", &newshow, b"'"]));
            }
            if store != newstore.as_slice() {
                rename.extend(cat(&[b" '", store, b"' to '", &newstore, b"'"]));
            }
            let what = cat(&[b"Consider renaming function(s)", &rename, b"\n"]);
            warn(self, "DEVICE_ATTR_FUNCTIONS", &what);
        } else {
            let what = cat(&[b"DEVICE_ATTR unusual permissions '", perms, b"' used\n"]);
            warn(self, "DEVICE_ATTR_PERMS", &what);
        }
    }

    fn mode_permissions(&mut self, l: &L, stat: &[u8]) {
        let funcarg = self.v.val("FuncArg").to_vec();
        for (func, arg_pos) in MODE_PERMISSION_FUNCS {
            let lc = count(stat, b'\n') + l.linenr;
            let stat_real = self.get_stat_real(l.linenr, lc);
            let mut skip_args: Vec<u8> = Vec::new();
            if *arg_pos > 1 {
                let skip = arg_pos - 1;
                skip_args = cat(&[
                    br"(?:\s*",
                    &funcarg,
                    br"\s*,\s*){",
                    skip.to_string().as_bytes(),
                    b",",
                    skip.to_string().as_bytes(),
                    b"}",
                ]);
            }
            let test = cat(&[
                br"\b",
                func.as_bytes(),
                br"\s*\(",
                &skip_args,
                b"(",
                &funcarg,
                br"(?:\|\s*",
                &funcarg,
                br")*)\s*[,\)]",
            ]);
            let Some(c) = self.ml(r"$test", "", &[("test", &test)], stat) else {
                continue;
            };
            let val = if skip_args.is_empty() { c.s(1) } else { c.s(6) };
            let is_int = self.t(r"^$Int$", &val);
            let is_octal = self.t(r"^$Octal$", &val);
            if !(self.t(r"^(?:module_param|proc_create)", func.as_bytes()) && val == b"0")
                && ((is_int && !is_octal) || (is_octal && val.len() != 4))
            {
                let msg = cat(&[
                    b"Use 4 digit octal (0777) not decimal permissions\n",
                    &l.here,
                    b"\n",
                    &stat_real,
                ]);
                self.error("NON_OCTAL_PERMISSIONS", &msg);
            }
            if is_octal {
                let octal: u64 = val
                    .iter()
                    .take_while(|b| (b'0'..=b'7').contains(*b))
                    .fold(0u64, |acc, &b| {
                        acc.wrapping_mul(8).wrapping_add(u64::from(b - b'0'))
                    });
                if octal & 0o2 != 0 {
                    let msg = cat(&[
                        b"Exporting writable files is usually an error. Consider more restrictive permissions.\n",
                        &l.here,
                        b"\n",
                        &stat_real,
                    ]);
                    self.error("EXPORTED_WORLD_WRITABLE", &msg);
                }
            }
        }
    }
}

fn len_of<T>(v: &[T]) -> i64 {
    i64::try_from(v.len()).expect("length")
}
