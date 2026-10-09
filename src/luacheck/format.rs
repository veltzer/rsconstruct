//! luacheck's messages (each stage's `message_format`) and the `plain`
//! formatter with `--codes`: `file:line:column: (Wnnn) message`.

use super::check::Warning;
use super::value::number_to_string;

fn quoted(value: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    out.extend_from_slice(value);
    out.push(b'\'');
    out
}

fn name(w: &Warning) -> Vec<u8> {
    quoted(w.name.as_deref().unwrap_or_default())
}

fn field(w: &Warning) -> Vec<u8> {
    quoted(w.field.as_deref().unwrap_or_default())
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn prefix_if_indirect(w: &Warning, message: Vec<u8>) -> Vec<u8> {
    if w.indirect {
        cat(&[b"indirectly ", &message])
    } else {
        message
    }
}

fn unused_or_overwritten(w: &Warning, subject: &[&[u8]]) -> Vec<u8> {
    let mut out = cat(subject);
    match w.overwritten_line {
        Some(line) => {
            out.extend_from_slice(b" is overwritten on line ");
            out.extend_from_slice(line.to_string().as_bytes());
            out.extend_from_slice(b" before use");
        }
        None => out.extend_from_slice(b" is unused"),
    }
    out
}

fn redefined(w: &Warning, before: &[u8], after: &[u8]) -> Vec<u8> {
    let prev_line = w.prev_line.unwrap_or(0).to_string();
    cat(&[before, &name(w), after, prev_line.as_bytes()])
}

/// The message for a warning, as luacheck formats it without color.
pub fn message(w: &Warning) -> Vec<u8> {
    match w.code.as_str() {
        "011" | "021" => w.msg.clone().unwrap_or_default(),
        "022" => b"unpaired push directive".to_vec(),
        "023" => b"unpaired pop directive".to_vec(),
        "033" => cat(&[
            b"assignment uses compound operator ",
            w.operator.as_deref().unwrap_or_default(),
        ]),
        "111" => {
            if w.module {
                cat(&[b"setting non-module global variable ", &name(w)])
            } else {
                cat(&[b"setting non-standard global variable ", &name(w)])
            }
        }
        "112" => cat(&[b"mutating non-standard global variable ", &name(w)]),
        "113" => cat(&[b"accessing undefined variable ", &name(w)]),
        "121" => cat(&[b"setting read-only global variable ", &name(w)]),
        "122" => prefix_if_indirect(
            w,
            cat(&[
                b"setting read-only field ",
                &field(w),
                b" of global ",
                &name(w),
            ]),
        ),
        "131" => cat(&[b"unused global variable ", &name(w)]),
        "142" => prefix_if_indirect(
            w,
            cat(&[
                b"setting undefined field ",
                &field(w),
                b" of global ",
                &name(w),
            ]),
        ),
        "143" => prefix_if_indirect(
            w,
            cat(&[
                b"accessing undefined field ",
                &field(w),
                b" of global ",
                &name(w),
            ]),
        ),
        "211" => {
            if w.func {
                if w.recursive {
                    cat(&[b"unused recursive function ", &name(w)])
                } else if w.mutually_recursive {
                    cat(&[b"unused mutually recursive function ", &name(w)])
                } else {
                    cat(&[b"unused function ", &name(w)])
                }
            } else {
                cat(&[b"unused variable ", &name(w)])
            }
        }
        "212" => {
            if w.name.as_deref() == Some(b"...") {
                b"unused variable length argument".to_vec()
            } else {
                cat(&[b"unused argument ", &name(w)])
            }
        }
        "213" => cat(&[b"unused loop variable ", &name(w)]),
        "214" => cat(&[b"used variable ", &name(w), b" with unused hint"]),
        "221" => cat(&[b"variable ", &name(w), b" is never set"]),
        "231" => cat(&[b"variable ", &name(w), b" is never accessed"]),
        "232" => cat(&[b"argument ", &name(w), b" is never accessed"]),
        "233" => cat(&[b"loop variable ", &name(w), b" is never accessed"]),
        "241" => cat(&[b"variable ", &name(w), b" is mutated but never accessed"]),
        "311" => unused_or_overwritten(w, &[b"value assigned to variable ", &name(w)]),
        "312" => unused_or_overwritten(w, &[b"value of argument ", &name(w)]),
        "313" => unused_or_overwritten(w, &[b"value of loop variable ", &name(w)]),
        "314" => {
            let target: &[u8] = if w.index { b"index" } else { b"field" };
            let line = w.overwritten_line.unwrap_or(0).to_string();
            cat(&[
                b"value assigned to ",
                target,
                b" ",
                &field(w),
                b" is overwritten on line ",
                line.as_bytes(),
                b" before use",
            ])
        }
        "321" => cat(&[b"accessing uninitialized variable ", &name(w)]),
        "331" => cat(&[
            b"value assigned to variable ",
            &name(w),
            b" is mutated but never accessed",
        ]),
        "341" => cat(&[b"mutating uninitialized variable ", &name(w)]),
        "411" => redefined(w, b"variable ", b" was previously defined on line "),
        "412" => redefined(
            w,
            b"variable ",
            b" was previously defined as an argument on line ",
        ),
        "413" => redefined(
            w,
            b"variable ",
            b" was previously defined as a loop variable on line ",
        ),
        "421" => redefined(w, b"shadowing definition of variable ", b" on line "),
        "422" => redefined(w, b"shadowing definition of argument ", b" on line "),
        "423" => redefined(w, b"shadowing definition of loop variable ", b" on line "),
        "431" => redefined(w, b"shadowing upvalue ", b" on line "),
        "432" => redefined(w, b"shadowing upvalue argument ", b" on line "),
        "433" => redefined(w, b"shadowing upvalue loop variable ", b" on line "),
        "511" => b"unreachable code".to_vec(),
        "512" => b"loop is executed at most once".to_vec(),
        "521" => cat(&[
            b"unused label ",
            &quoted(w.label.as_deref().unwrap_or_default()),
        ]),
        "531" => b"right side of assignment has more values than left side expects".to_vec(),
        "532" => b"right side of assignment has less values than left side expects".to_vec(),
        "541" => b"empty do..end block".to_vec(),
        "542" => b"empty if branch".to_vec(),
        "551" => b"empty statement".to_vec(),
        "561" => {
            let descr = if w.function_type == Some("main_chunk") {
                b"main chunk".to_vec()
            } else if let Some(function_name) = &w.function_name {
                cat(&[
                    w.function_type.unwrap_or("function").as_bytes(),
                    b" ",
                    &quoted(function_name),
                ])
            } else {
                b"function".to_vec()
            };
            let complexity = w.complexity.unwrap_or(0).to_string();
            let max = number_to_string(w.max_complexity.unwrap_or(0.0));
            cat(&[
                b"cyclomatic complexity of ",
                &descr,
                b" is too high (",
                complexity.as_bytes(),
                b" > ",
                max.as_bytes(),
                b")",
            ])
        }
        "571" => cat(&[
            b"numeric for loop goes from #(expr) down to ",
            w.limit.as_deref().unwrap_or_default(),
            b" but loop step is not negative",
        ]),
        "581" => cat(&[
            b"'not (x ",
            w.operator.as_deref().unwrap_or_default(),
            b" y)' can be replaced by 'x ",
            w.replacement_operator.as_deref().unwrap_or_default(),
            b" y' (if neither side is a table or NaN)",
        ]),
        "582" => b"Error prone negation: negation is executed before relational operator.".to_vec(),
        "611" => b"line contains only whitespace".to_vec(),
        "612" => b"line contains trailing whitespace".to_vec(),
        "613" => b"trailing whitespace in a string".to_vec(),
        "614" => b"trailing whitespace in a comment".to_vec(),
        "621" => b"inconsistent indentation (SPACE followed by TAB)".to_vec(),
        "631" => {
            let max = number_to_string(w.max_length.unwrap_or(0.0));
            cat(&[
                b"line is too long (",
                w.end_column.to_string().as_bytes(),
                b" > ",
                max.as_bytes(),
                b")",
            ])
        }
        other => format!("unknown warning code {other}").into_bytes(),
    }
}

/// `(Wnnn)` for a warning, `(Ennn)` for an error (codes starting with 0).
pub fn event_code(w: &Warning) -> String {
    let kind = if w.code.starts_with('0') { 'E' } else { 'W' };
    format!("{kind}{}", w.code)
}

/// A warning as `luacheck --formatter plain --codes` prints it.
pub fn plain_line(file_name: &str, w: &Warning) -> Vec<u8> {
    let mut out = format!("{file_name}:{}:{}: ({}) ", w.line, w.column, event_code(w)).into_bytes();
    out.extend(message(w));
    out
}
