//! What checkpatch.pl reads in `--file` mode: the output of
//! `diff -u /dev/null FILE` (GNU diff, in a UTF-8 locale), as lines.

/// A modification time as diff labels a file with it: local time, to the
/// nanosecond, with the UTC offset.
pub fn timestamp(mtime: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(mtime)
        .format("%Y-%m-%d %H:%M:%S%.9f %z")
        .to_string()
}

/// The lines of `diff -u /dev/null NAME` for a file holding `content`,
/// `block` bytes of which diff reads first to tell a binary file, last
/// modified at `mtime` (as `timestamp` writes it).
pub fn rawlines(name: &[u8], content: &[u8], block: usize, mtime: &str) -> Vec<Vec<u8>> {
    // checkpatch runs `diff -u /dev/null $filename` through the shell with
    // the name unquoted: one the shell splits or reinterprets makes diff
    // fail, and checkpatch reads nothing.
    if content.is_empty() || name.iter().any(|b| b" \t\n\"'\\$`;|&<>()".contains(b)) {
        return Vec::new();
    }
    let label = label(name);
    if content[..content.len().min(block)].contains(&0) {
        let mut line = b"Binary files /dev/null and ".to_vec();
        line.extend_from_slice(&label);
        line.extend_from_slice(b" differ");
        return vec![line];
    }
    let mut lines: Vec<&[u8]> = content.split(|&b| b == b'\n').collect();
    let no_eol = content.last() != Some(&b'\n');
    if !no_eol {
        lines.pop();
    }
    let mut out = Vec::with_capacity(lines.len() + 4);
    // checkpatch takes the name up to the first whitespace, but quotes the
    // whole `+++` line where a message shows the line before the first.
    out.push(b"--- /dev/null\t1970-01-01 00:00:00.000000000 +0000".to_vec());
    let mut plus = b"+++ ".to_vec();
    plus.extend_from_slice(&label);
    plus.push(b'\t');
    plus.extend_from_slice(mtime.as_bytes());
    out.push(plus);
    out.push(if lines.len() == 1 {
        b"@@ -0,0 +1 @@".to_vec()
    } else {
        format!("@@ -0,0 +1,{} @@", lines.len()).into_bytes()
    });
    for line in lines {
        let mut l = Vec::with_capacity(line.len() + 1);
        l.push(b'+');
        l.extend_from_slice(line);
        out.push(l);
    }
    if no_eol {
        out.push(b"\\ No newline at end of file".to_vec());
    }
    out
}

/// A file name as diff prints it in a header: C-quoted when it holds a
/// space, a double quote, a control character or bytes that are not UTF-8.
fn label(name: &[u8]) -> Vec<u8> {
    let plain = std::str::from_utf8(name).is_ok()
        && !name
            .iter()
            .any(|&b| b < 0x20 || b == 0x7f || b == b' ' || b == b'"');
    if plain {
        return name.to_vec();
    }
    // Valid UTF-8 sequences stay as they are; other bytes are escaped.
    let mut out = vec![b'"'];
    let mut rest = name;
    while !rest.is_empty() {
        let valid = match std::str::from_utf8(rest) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        for &b in &rest[..valid] {
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                0x07 => out.extend_from_slice(b"\\a"),
                0x08 => out.extend_from_slice(b"\\b"),
                b'\t' => out.extend_from_slice(b"\\t"),
                b'\n' => out.extend_from_slice(b"\\n"),
                0x0b => out.extend_from_slice(b"\\v"),
                0x0c => out.extend_from_slice(b"\\f"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b if b < 0x20 || b == 0x7f => {
                    out.extend_from_slice(format!("\\{b:03o}").as_bytes());
                }
                b => out.push(b),
            }
        }
        rest = &rest[valid..];
        if let Some((&b, tail)) = rest.split_first() {
            out.extend_from_slice(format!("\\{b:03o}").as_bytes());
            rest = tail;
        }
    }
    out.push(b'"');
    out
}
