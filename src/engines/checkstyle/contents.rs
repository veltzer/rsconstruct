//! checkstyle's `FileText` and `FileContents`: the file decoded as Java
//! decodes it, split into lines as `BufferedReader.readLine` splits, and the
//! comments the lexer reports, kept the way `FileContents` keeps them for
//! the checks that ask about comments (`FallThrough`) and Javadoc
//! (`UnusedImports`).
//!
//! Lines and columns here are Java's: UTF-16 code units. The lexer's line
//! count and `FileText`'s can disagree (a line end inside a string literal
//! moves one and not the other); where `FileContents` then indexes past a
//! line or the line array, Java throws, and so does this (an `Err` with the
//! exception as Java prints it), which aborts the file.

use std::collections::HashMap;

/// Java's `StringIndexOutOfBoundsException` from `substring`.
fn range_error(begin: i64, end: i64, len: usize) -> String {
    format!(
        "java.lang.StringIndexOutOfBoundsException: Range [{begin}, {end}) out of bounds for length {len}"
    )
}

/// Java's `ArrayIndexOutOfBoundsException`.
pub fn index_error(index: i64, len: usize) -> String {
    format!(
        "java.lang.ArrayIndexOutOfBoundsException: Index {index} out of bounds for length {len}"
    )
}

/// `String.substring(begin, end)` on UTF-16 units.
pub fn substring(s: &[u16], begin: i64, end: i64) -> Result<&[u16], String> {
    let len = s.len();
    if begin < 0 || end < begin || end > len as i64 {
        return Err(range_error(begin, end, len));
    }
    Ok(&s[begin as usize..end as usize])
}

/// `Character.isWhitespace(int)`.
pub fn is_java_whitespace(c: char) -> bool {
    use unicode_general_category::GeneralCategory as G;
    if matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{1C}'..='\u{1F}'
    ) {
        return true;
    }
    if matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}') {
        return false;
    }
    matches!(
        unicode_general_category::get_general_category(c),
        G::SpaceSeparator | G::LineSeparator | G::ParagraphSeparator
    )
}

/// `CommonUtil.isBlank`: only Java whitespace.
pub fn is_blank(line: &[u16]) -> bool {
    char::decode_utf16(line.iter().copied()).all(|c| c.is_ok_and(is_java_whitespace))
}

/// Java regex `\s` (without `UNICODE_CHARACTER_CLASS`).
pub const fn is_regex_space(c: u16) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D)
}

/// A line terminator for Java regex `.` and `$`.
pub const fn is_line_terminator(c: u16) -> bool {
    matches!(c, 0x0A | 0x0D | 0x85 | 0x2028 | 0x2029)
}

/// The file as checkstyle reads it.
pub struct FileText {
    /// The whole text in UTF-16, what the lexer reads.
    pub full: Vec<u16>,
    /// `[start, end)` of each line in `full`, terminators excluded.
    lines: Vec<(usize, usize)>,
}

impl FileText {
    /// Decode as UTF-8, malformed sequences replaced (Java's decoder with
    /// `CodingErrorAction.REPLACE`), and split at `\n`, `\r` and `\r\n`.
    pub fn new(bytes: &[u8]) -> Self {
        let full: Vec<u16> = String::from_utf8_lossy(bytes).encode_utf16().collect();
        let mut lines = Vec::new();
        let mut start = 0;
        let mut i = 0;
        while i < full.len() {
            match full[i] {
                0x0A => {
                    lines.push((start, i));
                    i += 1;
                    start = i;
                }
                0x0D => {
                    lines.push((start, i));
                    i += 1;
                    if full.get(i) == Some(&0x0A) {
                        i += 1;
                    }
                    start = i;
                }
                _ => i += 1,
            }
        }
        if start < full.len() {
            lines.push((start, full.len()));
        }
        Self { full, lines }
    }

    /// `FileText.get(index)`, which throws past the end.
    pub fn get(&self, index: i64) -> Result<&[u16], String> {
        if index < 0 || index >= self.lines.len() as i64 {
            return Err(index_error(index, self.lines.len()));
        }
        let (start, end) = self.lines[index as usize];
        Ok(&self.full[start..end])
    }
}

/// A comment as `FileContents` records it (`Comment`, a `TextBlock`).
#[derive(Clone, Debug)]
pub struct Comment {
    pub text: Vec<Vec<u16>>,
    pub start_line: i64,
    pub end_line: i64,
    pub start_col: i64,
    pub end_col: i64,
}

impl Comment {
    const fn new(text: Vec<Vec<u16>>, first_col: i64, last_line: i64, last_col: i64) -> Self {
        let start_line = last_line - text.len() as i64 + 1;
        Self {
            text,
            start_line,
            end_line: last_line,
            start_col: first_col,
            end_col: last_col,
        }
    }

    /// `Comment.intersects`.
    pub const fn intersects(
        &self,
        start_line: i64,
        start_col: i64,
        end_line: i64,
        end_col: i64,
    ) -> bool {
        let m = i32::MAX as i64;
        let this_start = self.start_line * m + self.start_col;
        let this_end = self.end_line * m + self.end_col;
        let in_start = start_line * m + start_col;
        let in_end = end_line * m + end_col;
        this_end >= in_start && in_end >= this_start
    }
}

/// The comments the lexer reported, kept as `FileContents` keeps them.
#[derive(Default)]
pub struct Comments {
    /// `cppComments`: the single-line comment starting on each line.
    single_line: HashMap<i64, Comment>,
    /// `clangComments`: the block comments starting on each line.
    block: HashMap<i64, Vec<Comment>>,
    /// `javadocComments`: possible Javadoc, by the 0-based line it ends on.
    javadoc: HashMap<i64, Comment>,
}

impl Comments {
    /// `reportSingleLineComment(startLineNo, startColNo)`, 1-based line,
    /// 0-based column.
    pub fn report_single_line_comment(
        &mut self,
        text: &FileText,
        line_no: i32,
        col_no: i32,
    ) -> Result<(), String> {
        let line = text.get(i64::from(line_no) - 1)?;
        let len = line.len() as i64;
        let content = substring(line, i64::from(col_no), len)?.to_vec();
        let comment = Comment::new(
            vec![content],
            i64::from(col_no),
            i64::from(line_no),
            len - 1,
        );
        self.single_line.insert(i64::from(line_no), comment);
        Ok(())
    }

    /// `reportBlockComment(startLineNo, startColNo, endLineNo, endColNo)`.
    pub fn report_block_comment(
        &mut self,
        text: &FileText,
        start_line: i32,
        start_col: i32,
        end_line: i32,
        end_col: i32,
    ) -> Result<(), String> {
        let (sl, sc, el, ec) = (
            i64::from(start_line),
            i64::from(start_col),
            i64::from(end_line),
            i64::from(end_col),
        );
        let lines = if sl == el {
            vec![substring(text.get(sl - 1)?, sc, ec + 1)?.to_vec()]
        } else {
            let first = text.get(sl - 1)?;
            let mut lines = vec![substring(first, sc, first.len() as i64)?.to_vec()];
            for i in sl..el {
                lines.push(text.get(i)?.to_vec());
            }
            let last = substring(text.get(el - 1)?, 0, ec + 1)?.to_vec();
            if let Some(slot) = lines.last_mut() {
                *slot = last;
            }
            lines
        };
        let comment = Comment::new(lines, sc, el, ec);
        self.block.entry(sl).or_default().push(comment.clone());
        let first_line = String::from_utf16_lossy(text.get(sl - 1)?);
        if first_line.contains("/**") && !first_line.contains("/**/") {
            self.javadoc.insert(el - 1, comment);
        }
        Ok(())
    }
}

/// `FileContents`: the text and the comments the lexer reported.
pub struct FileContents {
    pub text: FileText,
    pub comments: Comments,
}

impl FileContents {
    pub fn new(text: FileText) -> Self {
        Self {
            text,
            comments: Comments::default(),
        }
    }

    pub fn line(&self, index: i64) -> Result<&[u16], String> {
        self.text.get(index)
    }

    /// `lineIsComment`: the line matches `^\s*//.*$`.
    fn line_is_comment(line: &[u16]) -> bool {
        let rest: Vec<u16> = line
            .iter()
            .copied()
            .skip_while(|&c| is_regex_space(c))
            .collect();
        rest.starts_with(&[0x2F, 0x2F]) && !rest[2..].iter().any(|&c| is_line_terminator(c))
    }

    /// `getJavadocBefore(lineNoBefore)`: the Javadoc ending on the last
    /// line before `line_no` that is neither blank nor a `//` comment.
    pub fn javadoc_before(&self, line_no: i64) -> Result<Option<&Comment>, String> {
        let mut index = line_no - 2;
        while index > 0 {
            let line = self.line(index)?;
            if is_blank(line) || Self::line_is_comment(line) {
                index -= 1;
            } else {
                break;
            }
        }
        Ok(self.comments.javadoc.get(&index))
    }

    /// `hasIntersectionWithComment`.
    pub fn has_intersection_with_comment(
        &self,
        start_line: i64,
        start_col: i64,
        end_line: i64,
        end_col: i64,
    ) -> bool {
        let block = self
            .comments
            .block
            .values()
            .flatten()
            .any(|c| c.intersects(start_line, start_col, end_line, end_col));
        block
            || (start_line..=end_line).any(|line| {
                self.comments
                    .single_line
                    .get(&line)
                    .is_some_and(|c| c.intersects(start_line, start_col, end_line, end_col))
            })
    }
}
