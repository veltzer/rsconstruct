//! The input character stream (tidy's streamio.c `StreamIn` for UTF-8
//! input): one decoded character at a time, with the line and column
//! bookkeeping, tab expansion, CR/LF folding and the push-back buffer the
//! lexer relies on. The column of a pushed-back character is restored from
//! a ring of the last 64 columns, exactly as tidy does, so positions in
//! messages come out the same.

/// tidy's `EndOfStream`.
pub const END_OF_STREAM: u32 = u32::MAX;

const LASTPOS_SIZE: usize = 64;

/// A UTF-8 decoding error the stream met, reported by the lexer at the
/// position the stream recorded.
#[derive(Clone, Copy, Debug)]
pub struct DecodeError {
    pub code: u32,
    pub line: i32,
    pub column: i32,
}

pub struct Stream {
    data: Vec<u8>,
    pos: usize,
    /// The push-back stack (tidy's `charbuf`).
    pushed: Vec<u32>,
    /// Spaces still to deliver for an expanded tab.
    tabs: i32,
    lastcols: [i32; LASTPOS_SIZE],
    curlastpos: usize,
    firstlastpos: usize,
    pub curcol: i32,
    pub curline: i32,
    tab_size: u32,
    keep_tabs: bool,
    /// Decoding errors not yet reported.
    pub errors: Vec<DecodeError>,
}

impl Stream {
    pub const fn new(data: Vec<u8>, tab_size: u32, keep_tabs: bool) -> Self {
        Self {
            data,
            pos: 0,
            pushed: Vec::new(),
            tabs: 0,
            lastcols: [0; LASTPOS_SIZE],
            curlastpos: 0,
            firstlastpos: 0,
            curcol: 1,
            curline: 1,
            tab_size,
            keep_tabs,
            errors: Vec::new(),
        }
    }

    /// `TY_(ReadBOMEncoding)` for the UTF-8 case: swallow a UTF-8 byte
    /// order mark. A UTF-16 mark is reported to the caller, which does not
    /// support that input.
    pub fn skip_bom(&mut self) -> Result<(), &'static str> {
        if self.data.starts_with(&[0xEF, 0xBB, 0xBF]) {
            self.pos = 3;
            return Ok(());
        }
        if self.data.starts_with(&[0xFE, 0xFF]) || self.data.starts_with(&[0xFF, 0xFE]) {
            return Err("UTF-16 input (byte order mark found) is not supported");
        }
        Ok(())
    }

    const fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn read_byte(&mut self) -> u32 {
        if self.pos >= self.data.len() {
            return END_OF_STREAM;
        }
        let b = self.data[self.pos];
        self.pos += 1;
        u32::from(b)
    }

    const fn unget_byte(&mut self) {
        self.pos -= 1;
    }

    const fn pop_last_pos(&mut self) {
        self.curlastpos = (self.curlastpos + 1) % LASTPOS_SIZE;
        if self.curlastpos == self.firstlastpos {
            self.firstlastpos = (self.firstlastpos + 1) % LASTPOS_SIZE;
        }
    }

    const fn save_last_pos(&mut self) {
        self.pop_last_pos();
        self.lastcols[self.curlastpos] = self.curcol;
    }

    const fn restore_last_pos(&mut self) {
        if self.firstlastpos == self.curlastpos {
            self.curcol = 0;
        } else {
            self.curcol = self.lastcols[self.curlastpos];
            if self.curlastpos == 0 {
                self.curlastpos = LASTPOS_SIZE;
            }
            self.curlastpos -= 1;
        }
    }

    /// `TY_(ReadChar)`.
    pub fn read_char(&mut self) -> u32 {
        if !self.pushed.is_empty() {
            return self.pop_char();
        }
        self.save_last_pos();
        if self.tabs > 0 {
            self.curcol += 1;
            self.tabs -= 1;
            return u32::from(b' ');
        }
        loop {
            let mut c = self.read_char_from_stream();
            if c == END_OF_STREAM {
                return END_OF_STREAM;
            }
            if c == u32::from(b'\n') {
                self.curcol = 1;
                self.curline += 1;
                return c;
            }
            if c == u32::from(b'\t') {
                if !self.keep_tabs {
                    let tabsize = i32::try_from(self.tab_size).unwrap_or(i32::MAX);
                    self.tabs = if tabsize > 0 {
                        tabsize - ((self.curcol - 1) % tabsize) - 1
                    } else {
                        0
                    };
                    c = u32::from(b' ');
                }
                self.curcol += 1;
                return c;
            }
            if c == u32::from(b'\r') {
                let next = self.read_char_from_stream();
                if next != u32::from(b'\n') {
                    self.unget_char(next);
                }
                self.curcol = 1;
                self.curline += 1;
                return u32::from(b'\n');
            }
            // Control characters other than ESC are discarded; ESC is kept
            // but, as in tidy, not counted as a column.
            if c == 0x1b {
                return c;
            }
            if c < 32 {
                continue;
            }
            self.curcol += 1;
            return c;
        }
    }

    fn pop_char(&mut self) -> u32 {
        let Some(c) = self.pushed.pop() else {
            return END_OF_STREAM;
        };
        if c == u32::from(b'\n') {
            self.curcol = 1;
            self.curline += 1;
            self.pop_last_pos();
            return c;
        }
        self.curcol += 1;
        self.pop_last_pos();
        c
    }

    /// `TY_(UngetChar)`.
    pub fn unget_char(&mut self, c: u32) {
        if c == END_OF_STREAM {
            return;
        }
        self.pushed.push(c);
        if c == u32::from(b'\n') {
            self.curline -= 1;
        }
        self.restore_last_pos();
    }

    /// `EndOfInput`: nothing pushed back and nothing left to read.
    pub const fn end_of_input(&self) -> bool {
        self.pushed.is_empty() && self.is_eof()
    }

    /// `ReadCharFromStream` for UTF-8: decode one character, recording an
    /// error (and yielding U+FFFD) for an invalid sequence.
    fn read_char_from_stream(&mut self) -> u32 {
        if self.is_eof() {
            return END_OF_STREAM;
        }
        let first = self.read_byte();
        let (code, valid) = self.decode_utf8(first);
        if valid {
            code
        } else {
            self.errors.push(DecodeError {
                code,
                line: self.curline,
                column: self.curcol,
            });
            0xFFFD
        }
    }

    /// `TY_(DecodeUTF8BytesToChar)` reading the successor bytes from the
    /// stream. Returns the code point and whether the sequence was valid.
    fn decode_utf8(&mut self, first: u32) -> (u32, bool) {
        let mut has_error = false;
        let (mut n, mut bytes) = if first <= 0x7F {
            (first, 1)
        } else if (first & 0xE0) == 0xC0 {
            (first & 31, 2)
        } else if (first & 0xF0) == 0xE0 {
            (first & 15, 3)
        } else if (first & 0xF8) == 0xF0 {
            (first & 7, 4)
        } else if (first & 0xFC) == 0xF8 {
            has_error = true;
            (first & 3, 5)
        } else if (first & 0xFE) == 0xFC {
            has_error = true;
            (first & 1, 6)
        } else {
            has_error = true;
            (first, 1)
        };
        let mut buf = [0u32; 6];
        let mut i = 0;
        while i < bytes - 1 && !self.is_eof() {
            let b = self.read_byte();
            buf[i] = b;
            if (b & 0xC0) != 0x80 {
                has_error = true;
                bytes = i + 1;
                self.unget_byte();
                break;
            }
            n = (n << 6) | (b & 0x3F);
            i += 1;
        }
        if i < bytes - 1 && !has_error {
            // Ran out of input inside the sequence.
            has_error = true;
            bytes = i + 1;
        }
        if !has_error && (n == 0xFFFE || n == 0xFFFF) {
            has_error = true;
        }
        if !has_error && n > 0x0010_FFFF {
            has_error = true;
        }
        if !has_error {
            has_error = !sequence_is_valid(first, &buf, bytes, n);
        }
        (n, !has_error)
    }
}

/// The valid UTF-8 byte ranges per sequence length (utf8.c `validUTF8`):
/// low and high code point, then (low, high) byte pairs.
const VALID_UTF8: &[(u32, u32, usize, [u8; 8])] = &[
    (
        0x0000,
        0x007F,
        1,
        [0x00, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    ),
    (
        0x0080,
        0x07FF,
        2,
        [0xC2, 0xDF, 0x80, 0xBF, 0x00, 0x00, 0x00, 0x00],
    ),
    (
        0x0800,
        0x0FFF,
        3,
        [0xE0, 0xE0, 0xA0, 0xBF, 0x80, 0xBF, 0x00, 0x00],
    ),
    (
        0x1000,
        0xFFFF,
        3,
        [0xE1, 0xEF, 0x80, 0xBF, 0x80, 0xBF, 0x00, 0x00],
    ),
    (
        0x10000,
        0x3FFFF,
        4,
        [0xF0, 0xF0, 0x90, 0xBF, 0x80, 0xBF, 0x80, 0xBF],
    ),
    (
        0x40000,
        0xFFFFF,
        4,
        [0xF1, 0xF3, 0x80, 0xBF, 0x80, 0xBF, 0x80, 0xBF],
    ),
    (
        0x0010_0000,
        0x0010_FFFF,
        4,
        [0xF4, 0xF4, 0x80, 0x8F, 0x80, 0xBF, 0x80, 0xBF],
    ),
];

/// Offsets into `VALID_UTF8` per byte count (utf8.c `offsetUTF8Sequences`).
const OFFSETS: [usize; 5] = [0, 1, 2, 4, 7];

/// The overlong/range check of `TY_(DecodeUTF8BytesToChar)`.
fn sequence_is_valid(first: u32, successors: &[u32; 6], bytes: usize, n: u32) -> bool {
    if bytes == 0 || bytes > 4 {
        return false;
    }
    let lo = OFFSETS[bytes - 1];
    let hi = OFFSETS[bytes] - 1;
    if n < VALID_UTF8[lo].0 || n > VALID_UTF8[hi].1 {
        return false;
    }
    for entry in &VALID_UTF8[lo..=hi] {
        let mut ok = false;
        for k in 0..bytes {
            let byte = if k == 0 { first } else { successors[k - 1] };
            let low = u32::from(entry.3[k * 2]);
            let high = u32::from(entry.3[k * 2 + 1]);
            ok = byte >= low && byte <= high;
            if !ok {
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}
