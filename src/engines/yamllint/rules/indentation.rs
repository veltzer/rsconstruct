//! yamllint's `indentation` rule: a stack of the collections being walked,
//! each with the indent its children must have, checked against every
//! token that starts a line.

use super::super::Problem;
use super::super::config::RuleConf;
use super::super::parser::{Kind, Style, Token};
use super::{Alt, Check, Ctx, Default, OptKind, OptSpec, RuleSpec, States};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParentType {
    Root,
    BMap,
    FMap,
    BSeq,
    FSeq,
    BEnt,
    Key,
    Val,
}

#[derive(Debug, Clone, Copy)]
struct Parent {
    ty: ParentType,
    indent: i64,
    /// The indent of the line a flow collection opened on; what its closing
    /// bracket must align with when it starts a line.
    line_indent: Option<i64>,
    explicit_key: bool,
    implicit_block_seq: bool,
}

impl Parent {
    const fn new(ty: ParentType, indent: i64) -> Self {
        Self {
            ty,
            indent,
            line_indent: None,
            explicit_key: false,
            implicit_block_seq: false,
        }
    }
}

/// `spaces`: a width, or `consistent` until the first indent fixes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spaces {
    Consistent,
    Width(i64),
}

/// `indent-sequences`: `true`, `false`, `whatever`, or `consistent` until
/// the first sequence under a key decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndentSequences {
    Yes,
    No,
    Whatever,
    Consistent,
}

struct Inner {
    stack: Vec<Parent>,
    cur_line: i64,
    cur_line_indent: i64,
    spaces: Spaces,
    indent_sequences: IndentSequences,
}

impl Inner {
    fn new(conf: &RuleConf) -> Self {
        let spaces = match conf.opt("spaces").as_int() {
            Some(n) => Spaces::Width(n),
            None => Spaces::Consistent,
        };
        let indent_sequences = match conf.opt("indent-sequences").as_bool() {
            Some(true) => IndentSequences::Yes,
            Some(false) => IndentSequences::No,
            None => {
                if conf.opt("indent-sequences").as_str() == Some("consistent") {
                    IndentSequences::Consistent
                } else {
                    IndentSequences::Whatever
                }
            }
        };
        Self {
            stack: vec![Parent::new(ParentType::Root, 0)],
            cur_line: -1,
            cur_line_indent: 0,
            spaces,
            indent_sequences,
        }
    }

    /// Mirrors `detect_indent`: the first indent seen fixes `consistent`.
    fn detect_indent(&mut self, base_indent: i64, found: i64) -> i64 {
        if self.spaces == Spaces::Consistent {
            self.spaces = Spaces::Width(found - base_indent);
        }
        match self.spaces {
            Spaces::Width(n) => base_indent + n,
            Spaces::Consistent => base_indent,
        }
    }

    fn last(&self) -> Parent {
        *self
            .stack
            .last()
            .expect("the indentation stack keeps its ROOT")
    }

    fn last_mut(&mut self) -> &mut Parent {
        self.stack
            .last_mut()
            .expect("the indentation stack keeps its ROOT")
    }

    /// The parent below the top, where yamllint reads `stack[-2]`.
    fn below(&self) -> Option<Parent> {
        let len = self.stack.len();
        (len >= 2).then(|| self.stack[len - 2])
    }
}

/// The rule's state; built from the config at the first token.
#[derive(Default)]
pub struct State {
    inner: Option<Inner>,
}

/// Mirrors `check_scalar_indentation`, run when `check-multi-line-strings`
/// is set: the continuation lines of a multi-line scalar.
fn check_scalar_indentation(token: &Token, st: &mut Inner, buffer: &str, out: &mut Vec<Problem>) {
    if token.start.line == token.end.line {
        return;
    }
    let bytes = buffer.as_bytes();
    let mut expected_indent: Option<i64> = None;
    let mut line_no = token.start.line + 1;
    let mut line_start = token.start.index;
    let limit = token.end.index.saturating_sub(1);
    loop {
        if line_start >= limit {
            break;
        }
        let Some(pos) = bytes[line_start..limit].iter().position(|&b| b == b'\n') else {
            break;
        };
        line_start += pos + 1;
        line_no += 1;

        let mut indent = 0;
        while line_start + indent < bytes.len() && bytes[line_start + indent] == b' ' {
            indent += 1;
        }
        if line_start + indent >= bytes.len() || bytes[line_start + indent] == b'\n' {
            continue;
        }
        let indent = indent as i64;

        let expected =
            *expected_indent.get_or_insert_with(|| compute_expected_indent(token, st, indent));
        if indent != expected {
            out.push(Problem::new(
                line_no,
                (indent + 1) as usize,
                format!("wrong indentation: expected {expected} but found {indent}"),
            ));
        }
    }
}

/// Mirrors the inner `compute_expected_indent` of `check_scalar_indentation`.
fn compute_expected_indent(token: &Token, st: &mut Inner, found_indent: i64) -> i64 {
    let column = token.start.column as i64;
    let detect = |st: &mut Inner, base_indent: i64| -> i64 {
        if st.spaces == Spaces::Consistent {
            st.spaces = Spaces::Width(found_indent - base_indent);
        }
        match st.spaces {
            Spaces::Width(n) => base_indent + n,
            Spaces::Consistent => base_indent,
        }
    };
    match token.scalar_style() {
        Some(Style::Plain) | None => column,
        Some(Style::SingleQuoted | Style::DoubleQuoted) => column + 1,
        Some(Style::Literal | Style::Folded) => {
            let last = st.last();
            match last.ty {
                // - >
                //     multi
                //     line
                ParentType::BEnt => detect(st, column),
                // - ? >
                //       multi-line
                //       key
                //   : >
                //       multi-line
                //       value
                ParentType::Key => detect(st, column),
                ParentType::Val => {
                    if token.start.line as i64 + 1 > st.cur_line {
                        // - key:
                        //     >
                        //       multi
                        //       line
                        detect(st, last.indent)
                    } else if st.below().is_some_and(|p| p.explicit_key) {
                        // - ? key
                        //   : >
                        //       multi-line
                        //       value
                        detect(st, column)
                    } else {
                        // - key: >
                        //     multi
                        //     line
                        let base = st.below().map_or(last.indent, |p| p.indent);
                        detect(st, base)
                    }
                }
                _ => detect(st, last.indent),
            }
        }
    }
}

/// Mirrors `_check`. `Err(())` stands for the `AssertionError` yamllint
/// raises on a token sequence its model does not expect.
fn check_inner(
    conf: &RuleConf,
    ctx: &Ctx<'_, '_>,
    st: &mut Inner,
    out: &mut Vec<Problem>,
) -> Result<(), ()> {
    let token = ctx.token();
    let prev = ctx.prev();
    let next = ctx.next();
    let nextnext = ctx.nextnext();
    let buffer = ctx.buffer();

    // Step 1: Lint

    let is_visible = !matches!(
        token.kind,
        Kind::StreamStart | Kind::StreamEnd | Kind::BlockEnd
    ) && token.scalar_value() != Some("");
    let first_in_line = is_visible && token.start.line as i64 + 1 > st.cur_line;

    let mut found_indentation = 0;
    if first_in_line {
        found_indentation = token.start.column as i64;
        let last = st.last();
        let mut expected = last.indent;

        if matches!(token.kind, Kind::FlowMappingEnd | Kind::FlowSequenceEnd) {
            expected = last.line_indent.unwrap_or(-1);
        } else if last.ty == ParentType::Key && last.explicit_key && token.kind != Kind::Value {
            expected = st.detect_indent(expected, token.start.column as i64);
        }

        if found_indentation != expected {
            let message = if expected < 0 {
                format!(
                    "wrong indentation: expected at least {}",
                    found_indentation + 1
                )
            } else {
                format!("wrong indentation: expected {expected} but found {found_indentation}")
            };
            out.push(Problem::new(
                token.start.line + 1,
                (found_indentation + 1) as usize,
                message,
            ));
        }
    }

    if token.is_scalar() && conf.bool_opt("check-multi-line-strings") {
        check_scalar_indentation(token, st, buffer, out);
    }

    // Step 2.a:

    if is_visible {
        st.cur_line = token.real_end_line(buffer) as i64;
        if first_in_line {
            st.cur_line_indent = found_indentation;
        }
    }

    // Step 2.b: Update state

    match &token.kind {
        Kind::BlockMappingStart => {
            //   - a: 1
            // or
            //   - ? a
            //     : 1
            // or
            //   - ?
            //       a
            //     : 1
            let next = next.ok_or(())?;
            if next.kind != Kind::Key || next.start.line != token.start.line {
                return Err(());
            }
            st.stack
                .push(Parent::new(ParentType::BMap, token.start.column as i64));
        }
        Kind::FlowMappingStart => {
            let next = next.ok_or(())?;
            let indent = if next.start.line == token.start.line {
                //   - {a: 1, b: 2}
                next.start.column as i64
            } else {
                //   - {
                //     a: 1, b: 2
                //   }
                let base = st.cur_line_indent;
                st.detect_indent(base, next.start.column as i64)
            };
            let mut parent = Parent::new(ParentType::FMap, indent);
            parent.line_indent = Some(st.cur_line_indent);
            st.stack.push(parent);
        }
        Kind::BlockSequenceStart => {
            //   - - a
            //     - b
            let next = next.ok_or(())?;
            if next.kind != Kind::BlockEntry || next.start.line != token.start.line {
                return Err(());
            }
            st.stack
                .push(Parent::new(ParentType::BSeq, token.start.column as i64));
        }
        Kind::BlockEntry
            if !next.is_some_and(|n| matches!(n.kind, Kind::BlockEntry | Kind::BlockEnd)) =>
        {
            // (an empty entry is skipped by the guard above)
            let next = next.ok_or(())?;
            // pyyaml issues no BlockSequenceStartToken when the list is not
            // indented; compensate.
            if st.last().ty != ParentType::BSeq {
                let mut parent = Parent::new(ParentType::BSeq, token.start.column as i64);
                parent.implicit_block_seq = true;
                st.stack.push(parent);
            }

            let indent = if next.start.line == token.end.line {
                //   - item 1
                //   - item 2
                next.start.column as i64
            } else if next.start.column == token.start.column {
                //   -
                //   key: value
                next.start.column as i64
            } else {
                //   -
                //     item 1
                //   -
                //     key:
                //       value
                st.detect_indent(token.start.column as i64, next.start.column as i64)
            };
            st.stack.push(Parent::new(ParentType::BEnt, indent));
        }
        Kind::FlowSequenceStart => {
            let next = next.ok_or(())?;
            let indent = if next.start.line == token.start.line {
                //   - [a, b]
                next.start.column as i64
            } else {
                //   - [
                //   a, b
                // ]
                let base = st.cur_line_indent;
                st.detect_indent(base, next.start.column as i64)
            };
            let mut parent = Parent::new(ParentType::FSeq, indent);
            parent.line_indent = Some(st.cur_line_indent);
            st.stack.push(parent);
        }
        Kind::Key => {
            let indent = st.last().indent;
            let mut parent = Parent::new(ParentType::Key, indent);
            parent.explicit_key = token.is_explicit_key(buffer);
            st.stack.push(parent);
        }
        Kind::Value => {
            if st.last().ty != ParentType::Key {
                return Err(());
            }
            let prev = prev.ok_or(())?;

            // Special cases:
            //     key: &anchor
            //       value
            // and:
            //     key: !!tag
            //       value
            let mut next = next;
            if let Some(n) = next
                && matches!(n.kind, Kind::Anchor(_) | Kind::Tag { .. })
                && let Some(nn) = nextnext
                && n.start.line == prev.start.line
                && n.start.line < nn.start.line
            {
                next = Some(nn);
            }

            // Only if value is not empty
            if let Some(n) = next
                && !matches!(
                    n.kind,
                    Kind::BlockEnd | Kind::FlowMappingEnd | Kind::FlowSequenceEnd | Kind::Key
                )
            {
                let last = st.last();
                let n_col = n.start.column as i64;
                let indent = if last.explicit_key {
                    //   ? k
                    //   : value
                    // or
                    //   ? k
                    //   :
                    //     value
                    st.detect_indent(last.indent, n_col)
                } else if n.start.line == prev.start.line {
                    //   k: value
                    n_col
                } else if matches!(n.kind, Kind::BlockSequenceStart | Kind::BlockEntry) {
                    // BlockEntryToken is tested too because sometimes
                    // BlockSequenceStartTokens are not issued, e.g. for
                    //     '- lib:\n'
                    //     '  - var\n'
                    match st.indent_sequences {
                        IndentSequences::No => last.indent,
                        IndentSequences::Yes => {
                            if st.spaces == Spaces::Consistent && n_col - last.indent == 0 {
                                // The block sequence item is not indented
                                // (while it should be), but the indentation
                                // it should have is not known yet: `spaces`
                                // is `consistent` and still undecided, so
                                // this is probably the start of the
                                // document. Use an unknown value (-1).
                                -1
                            } else {
                                st.detect_indent(last.indent, n_col)
                            }
                        }
                        IndentSequences::Whatever | IndentSequences::Consistent => {
                            if n_col == last.indent {
                                //   key:
                                //   - e1
                                //   - e2
                                if st.indent_sequences == IndentSequences::Consistent {
                                    st.indent_sequences = IndentSequences::No;
                                }
                                last.indent
                            } else {
                                if st.indent_sequences == IndentSequences::Consistent {
                                    st.indent_sequences = IndentSequences::Yes;
                                }
                                //   key:
                                //     - e1
                                //     - e2
                                st.detect_indent(last.indent, n_col)
                            }
                        }
                    }
                } else {
                    //   k:
                    //     value
                    st.detect_indent(last.indent, n_col)
                };
                st.stack.push(Parent::new(ParentType::Val, indent));
            }
        }
        _ => {}
    }

    let next_is = |kinds: &dyn Fn(&Kind) -> bool| next.is_some_and(|n| kinds(&n.kind));
    let token_is_property = matches!(token.kind, Kind::Anchor(_) | Kind::Tag { .. });
    let mut consumed_current_token = false;
    loop {
        let last = st.last();
        // The token closes the collection on top of the stack: `]`, `}`, or
        // the block end of an explicit block mapping or sequence.
        let closes_last = (last.ty == ParentType::FSeq && token.kind == Kind::FlowSequenceEnd)
            || (last.ty == ParentType::FMap && token.kind == Kind::FlowMappingEnd)
            || (matches!(last.ty, ParentType::BMap | ParentType::BSeq)
                && token.kind == Kind::BlockEnd
                && !last.implicit_block_seq);
        if closes_last && !consumed_current_token {
            st.stack.pop();
            consumed_current_token = true;
        } else if last.ty == ParentType::BEnt
            && token.kind != Kind::BlockEntry
            && st.below().is_some_and(|p| p.implicit_block_seq)
            && !token_is_property
            && !next_is(&|k| *k == Kind::BlockEntry)
        {
            st.stack.pop();
            st.stack.pop();
        } else if last.ty == ParentType::BEnt
            && next_is(&|k| matches!(k, Kind::BlockEntry | Kind::BlockEnd))
        {
            st.stack.pop();
        } else if last.ty == ParentType::Val && token.kind != Kind::Value && !token_is_property {
            if st.below().map(|p| p.ty) != Some(ParentType::Key) {
                return Err(());
            }
            st.stack.pop();
            st.stack.pop();
        } else if last.ty == ParentType::Key
            && next_is(&|k| {
                matches!(
                    k,
                    Kind::BlockEnd | Kind::FlowMappingEnd | Kind::FlowSequenceEnd | Kind::Key
                )
            })
        {
            // A key without a value: it's part of a set. Drop this key and
            // leave room for the next one.
            st.stack.pop();
        } else {
            break;
        }
    }
    // Keep the ROOT no matter what the pops above did.
    if st.stack.is_empty() {
        st.stack.push(Parent::new(ParentType::Root, 0));
    }
    let _ = st.last_mut();
    Ok(())
}

fn check_indentation(
    conf: &RuleConf,
    ctx: &Ctx<'_, '_>,
    states: &mut States,
    out: &mut Vec<Problem>,
) {
    let st = states
        .indentation
        .inner
        .get_or_insert_with(|| Inner::new(conf));
    if check_inner(conf, ctx, st, out).is_err() {
        let token = ctx.token();
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            "cannot infer indentation: unexpected token",
        ));
    }
}

const SPACES_ALTS: &[Alt] = &[Alt::AnyInt, Alt::Str("consistent")];
const INDENT_SEQUENCES_ALTS: &[Alt] = &[Alt::AnyBool, Alt::Str("whatever"), Alt::Str("consistent")];

pub const INDENTATION: RuleSpec = RuleSpec {
    id: "indentation",
    options: &[
        OptSpec {
            name: "spaces",
            kind: OptKind::OneOf(SPACES_ALTS),
            default: Default::Str("consistent"),
        },
        OptSpec {
            name: "indent-sequences",
            kind: OptKind::OneOf(INDENT_SEQUENCES_ALTS),
            default: Default::Bool(true),
        },
        OptSpec {
            name: "check-multi-line-strings",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
    ],
    validate: None,
    check: Check::Token(check_indentation),
};
