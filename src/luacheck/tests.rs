//! Unit tests for the luacheck port: the Lua pattern matcher, and whole
//! sources checked against luacheck 1.2.0's output for them
//! (`luacheck --no-config --formatter plain --codes --no-color`).

use super::filter::{self, FileInput, FileReport};
use super::format::plain_line;
use super::pattern::{Capture, find, gsub, lua_match, pmatch};
use super::{check_source, config};

#[test]
fn pattern_find_basics() {
    let m = find(b"hello world", b"o w", 1).unwrap().unwrap();
    assert_eq!((m.start, m.end), (5, 7));
    let m = find(b"  x  ", b"^%s*", 1).unwrap().unwrap();
    assert_eq!((m.start, m.end), (1, 2));
    let m = find(b"abc   ", b"%s*$", 1).unwrap().unwrap();
    assert_eq!((m.start, m.end), (4, 6));
    assert!(find(b"abc", b"^b", 1).unwrap().is_none());
}

#[test]
fn pattern_captures_and_positions() {
    let m = find(b"ab  \n", b"^[^\r\n]-()[ \t]+()[\r\n]", 1)
        .unwrap()
        .unwrap();
    assert_eq!(m.captures, vec![Capture::Pos(3), Capture::Pos(5)]);
    let caps = lua_match(b"push  x", b"^push%s+(.*)", 1).unwrap().unwrap();
    assert_eq!(caps[0].bytes(b"push  x"), b"x");
}

#[test]
fn pattern_errors_are_lazy() {
    assert!(!pmatch(b"b", b"a%").unwrap());
    assert_eq!(
        pmatch(b"a", b"a%").unwrap_err().0,
        "malformed pattern (ends with '%')"
    );
    assert_eq!(
        pmatch(b"a", b"[a").unwrap_err().0,
        "malformed pattern (missing ']')"
    );
}

#[test]
fn pattern_gsub_balanced() {
    let (out, n) = gsub(b"a (b) c (d)", b"%b()", |_| Some(b" ".to_vec())).unwrap();
    assert_eq!(out, b"a   c  ");
    assert_eq!(n, 2);
}

/// One source checked with luacheck's defaults (no configuration file).
fn luacheck(name: &str, src: &str) -> Vec<String> {
    let current_dir = config::current_dir().unwrap();
    let stack =
        config::stack_configs(vec![config::load_config(None, &current_dir).unwrap()]).unwrap();
    let options = stack.get_options(name, &current_dir).unwrap();
    let result = check_source(src.as_bytes().to_vec());
    let reports = filter::filter(
        vec![FileInput::Checked {
            result,
            stack: options,
        }],
        &stack.stds,
    )
    .unwrap();
    match reports.into_iter().next() {
        Some(FileReport::Warnings(warnings)) => warnings
            .iter()
            .map(|w| String::from_utf8_lossy(&plain_line(name, w)).into_owned())
            .collect(),
        _ => panic!("no report"),
    }
}

#[test]
fn analysis_matches_luacheck() {
    let src = "local x = 1\ny = 2\nlocal function f(a)\n  return\nend\nprint(z.w)\nfor i = #t, 1 do end\n\
               local t2 = {a = 1, a = 2}\nwhile true do break; print(1) end\n";
    assert_eq!(
        luacheck("t.lua", src),
        vec![
            "t.lua:1:7: (W211) unused variable 'x'",
            "t.lua:2:1: (W111) setting non-standard global variable 'y'",
            "t.lua:3:16: (W211) unused function 'f'",
            "t.lua:3:18: (W212) unused argument 'a'",
            "t.lua:6:7: (W113) accessing undefined variable 'z'",
            "t.lua:7:1: (W571) numeric for loop goes from #(expr) down to 1 but loop step is not negative",
            "t.lua:7:5: (W213) unused loop variable 'i'",
            "t.lua:7:10: (W113) accessing undefined variable 't'",
            "t.lua:8:7: (W211) unused variable 't2'",
            "t.lua:8:13: (W314) value assigned to field 'a' is overwritten on line 8 before use",
            "t.lua:9:22: (W511) unreachable code",
        ]
    );
}

#[test]
fn missing_end_is_guessed_by_indentation() {
    assert_eq!(
        luacheck("s.lua", "if x then\n  y()\n\nlocal q = 1\n"),
        vec![
            "s.lua:4:1: (E011) expected 'end' (to close 'if' on line 1) near 'local' (indentation-based guess)"
        ]
    );
}
