//! `ShellCheck`'s parser (`ShellCheck.Parser`), part one: combinators,
//! spacing, comments and directives, words, quoting, `$` expansions, here
//! documents and redirections.
//!
//! Every function is a method on the Parsec engine `P` named after its
//! Haskell original (`readNormalWord` is `read_normal_word`), with the same
//! order of alternatives, the same `try`s and the same warnings, because the
//! warnings `ShellCheck` emits depend on exactly which branches run.

use super::ast::{Annotation, Dashed, Id, Inner, Quoted, SourcePos, Token};
use super::astlib::{e4m, only_literal_string};
use super::data::shell_for_executable;
use super::hchar::{is_alpha, is_lower, is_upper, lower_string};
use super::parsec::{
    HereDocPending, P, R,
    Severity::{ErrorC, InfoC, StyleC, WarningC},
};

use Inner::{
    T_Arithmetic, T_Backticked, T_BraceExpansion, T_DollarArithmetic,
    T_DollarBraceCommandExpansion, T_DollarBraced, T_DollarBracket, T_DollarDoubleQuoted,
    T_DollarExpansion, T_DollarSingleQuoted, T_DoubleQuoted, T_Extglob, T_FdRedirect, T_Glob,
    T_Greater, T_HereDoc, T_HereString, T_IoDuplicate, T_IoFile, T_Less, T_Literal, T_NormalWord,
    T_ParamSubSpecialChar, T_ProcSub, T_SingleQuoted,
};

pub const QUOTABLE_CHARS: &str = "|&;<>()\\ '\t\n\r\u{A0}\\\"$`";
pub const DOUBLE_QUOTABLE_CHARS: &str = "\\\"$`";
pub const EXTGLOB_START_CHARS: &str = "?*@!+";
pub const UNICODE_DOUBLE_QUOTES: &str = "\u{201C}\u{201D}\u{2033}\u{2036}";
pub const UNICODE_SINGLE_QUOTES: &str = "\u{2018}\u{2019}";
const UNICODE_SPACES: &str =
    "\u{A0}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200B}\u{202F}";
const UNICODE_DASHES: &str =
    "\u{058A}\u{05BE}\u{2010}\u{2011}\u{2012}\u{2013}\u{2014}\u{2015}\u{FE63}\u{FF0D}";
pub const SPECIAL_VARIABLE_CHARS: &str = "-$?!#@*";

pub const fn tk(id: Id, inner: Inner) -> Token {
    Token::new(id, inner)
}

#[allow(
    clippy::unnecessary_box_returns,
    reason = "the boxing helper for the AST's recursive fields"
)]
pub fn bx(t: Token) -> Box<Token> {
    Box::new(t)
}

fn standard_end() -> String {
    format!(
        "[{{}}{QUOTABLE_CHARS}{EXTGLOB_START_CHARS}{UNICODE_DOUBLE_QUOTES}{UNICODE_SINGLE_QUOTES}"
    )
}

fn is_variable_start(c: char) -> bool {
    is_upper(c) || is_lower(c) || c == '_'
}

fn is_variable_char(c: char) -> bool {
    is_upper(c) || is_lower(c) || c.is_ascii_digit() || c == '_'
}

pub fn is_function_start_char(c: char) -> bool {
    is_variable_char(c) || ":+?-./^@,".contains(c)
}

pub fn is_function_char(c: char) -> bool {
    is_variable_char(c) || "#:+?-./^@,".contains(c)
}

pub fn is_extended_function_start_char(c: char) -> bool {
    is_function_start_char(c) || "[]*=!".contains(c)
}

impl P {
    // ----- convenience combinators

    /// `unexpecting s p`.
    pub fn unexpecting<T>(&mut self, s: &str, p: impl FnOnce(&mut Self) -> R<T>) -> R<()> {
        let msg = format!("Unexpected {s}");
        self.try_(|st| {
            st.alt(
                |st| {
                    st.try_(p)?;
                    st.fail(&msg)
                },
                |st| st.ok(()),
            )
        })
    }

    /// `notFollowedBy2`.
    pub fn not_followed_by2<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<()> {
        self.unexpecting("", p)
    }

    /// `isFollowedBy`.
    pub fn is_followed_by<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<bool> {
        self.alt(
            |s| s.look_ahead(|s| s.try_(p)).map(|_| true),
            |s| s.ok(false),
        )
    }

    /// `reluctantlyTill p end`.
    pub fn reluctantly_till<T, E>(
        &mut self,
        mut p: impl FnMut(&mut Self) -> R<T>,
        mut end: impl FnMut(&mut Self) -> R<E>,
    ) -> R<Vec<T>> {
        let mut out = Vec::new();
        loop {
            let m = self.mark();
            let ahead = self.look_ahead(|s| s.alt(|s| s.try_(&mut end).map(|_| ()), Self::eof));
            match ahead {
                Ok(()) => return Ok(out),
                Err(f) if self.consumed_since(&f, &m) => return Err(f),
                Err(f) => {
                    self.restore(&m);
                    self.err = f.err;
                }
            }
            match p(self) {
                Ok(x) => out.push(x),
                Err(f) if self.consumed_since(&f, &m) => return Err(f),
                Err(f) => {
                    self.restore(&m);
                    self.err = f.err;
                    return Ok(out);
                }
            }
        }
    }

    /// `reluctantlyTill1 p end`.
    pub fn reluctantly_till1<T, E>(
        &mut self,
        mut p: impl FnMut(&mut Self) -> R<T>,
        mut end: impl FnMut(&mut Self) -> R<E>,
    ) -> R<Vec<T>> {
        self.not_followed_by2(&mut end)?;
        let x = p(self)?;
        let mut more = self.reluctantly_till(p, end)?;
        more.insert(0, x);
        Ok(more)
    }

    /// `attempting rest branch`: `(try branch >> rest) <|> rest`.
    pub fn attempting<T, B>(
        &mut self,
        mut rest: impl FnMut(&mut Self) -> R<T>,
        branch: impl FnOnce(&mut Self) -> R<B>,
    ) -> R<T> {
        let m = self.mark();
        let first = match self.try_(branch) {
            Ok(_) => rest(self),
            Err(f) => Err(f),
        };
        match first {
            Ok(v) => Ok(v),
            Err(f) if self.consumed_since(&f, &m) => Err(f),
            Err(f) => {
                self.restore(&m);
                self.err = f.err;
                rest(self)
            }
        }
    }

    /// `orFail parser errorAction`.
    pub fn or_fail<T>(
        &mut self,
        parser: impl FnOnce(&mut Self) -> R<T>,
        error_action: impl FnOnce(&mut Self) -> String,
    ) -> R<T> {
        self.alt(
            |s| s.try_(parser),
            |s| {
                let msg = error_action(s);
                s.fail(&msg)
            },
        )
    }

    /// `acceptButWarn parser level code note`.
    pub fn accept_but_warn<T>(
        &mut self,
        parser: impl FnOnce(&mut Self) -> R<T>,
        level: super::parsec::Severity,
        code: i64,
        note: &str,
    ) -> R<()> {
        self.optional(|s| {
            s.try_(|s| {
                let pos = s.pos;
                parser(s)?;
                s.parse_problem_at(pos, level, code, note);
                Ok(())
            })
        })
    }

    /// `main `thenSkip` follow`.
    pub fn then_skip<T, F>(
        &mut self,
        main: impl FnOnce(&mut Self) -> R<T>,
        follow: impl FnOnce(&mut Self) -> R<F>,
    ) -> R<T> {
        let v = main(self)?;
        self.optional(follow)?;
        Ok(v)
    }

    /// `ifNextToken parser action`.
    pub fn if_next_token<T>(
        &mut self,
        parser: impl FnOnce(&mut Self) -> R<T>,
        action: impl FnOnce(&mut Self),
    ) -> R<()> {
        self.optional(|s| {
            s.try_(|s| s.look_ahead(parser))?;
            action(s);
            Ok(())
        })
    }

    /// `wasIncluded p`.
    pub fn was_included<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<bool> {
        self.option(false, |s| p(s).map(|_| true))
    }

    /// `parseForgettingContext alsoOnSuccess parser`.
    pub fn parse_forgetting_context<T>(
        &mut self,
        also_on_success: bool,
        parser: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<T> {
        let saved = self.sys.clone();
        let m = self.mark();
        match self.try_(parser) {
            Ok(v) => {
                if also_on_success {
                    self.sys = saved;
                }
                Ok(v)
            }
            Err(f) => {
                self.restore(&m);
                self.err = f.err;
                self.sys = saved;
                self.fail("")
            }
        }
    }

    /// `inSeparateContext`.
    pub fn in_separate_context<T>(&mut self, parser: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.parse_forgetting_context(true, parser)
    }

    /// `forgetOnFailure`.
    pub fn forget_on_failure<T>(&mut self, parser: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.parse_forgetting_context(false, parser)
    }

    /// `ignoreProblemsOf`.
    pub fn ignore_problems_of<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let saved = self.sys.clone();
        let v = p(self)?;
        self.sys = saved;
        Ok(v)
    }

    // ----- characters

    pub fn backslash(&mut self) -> R<char> {
        self.char_('\\')
    }

    pub fn linefeed(&mut self) -> R<char> {
        self.optional(Self::carriage_return)?;
        let c = self.char_('\n')?;
        self.read_pending_here_docs()?;
        Ok(c)
    }

    pub fn single_quote(&mut self) -> R<char> {
        self.char_('\'')
    }

    pub fn double_quote(&mut self) -> R<char> {
        self.char_('"')
    }

    pub fn variable_start(&mut self) -> R<char> {
        self.satisfy(is_variable_start)
    }

    pub fn variable_chars(&mut self) -> R<char> {
        self.satisfy(is_variable_char)
    }

    pub fn special_variable(&mut self) -> R<char> {
        self.one_of(SPECIAL_VARIABLE_CHARS)
    }

    pub fn quotable(&mut self) -> R<char> {
        self.alt(Self::almost_space, |s| s.one_of(QUOTABLE_CHARS))
    }

    pub fn braced_quotable(&mut self) -> R<char> {
        self.one_of("}\"$`'")
    }

    pub fn double_quotable(&mut self) -> R<char> {
        self.one_of(DOUBLE_QUOTABLE_CHARS)
    }

    pub fn whitespace(&mut self) -> R<char> {
        self.alt(
            |s| s.one_of(" \t"),
            |s| {
                s.alt(Self::carriage_return, |s| {
                    s.alt(Self::almost_space, Self::linefeed)
                })
            },
        )
    }

    pub fn linewhitespace(&mut self) -> R<char> {
        self.alt(|s| s.one_of(" \t"), Self::almost_space)
    }

    pub fn suspect_char_after_quotes(&mut self) -> R<char> {
        self.alt(Self::variable_chars, |s| s.char_('%'))
    }

    pub fn extglob_start(&mut self) -> R<char> {
        self.one_of(EXTGLOB_START_CHARS)
    }

    pub fn weird_dash(&mut self) -> R<char> {
        let pos = self.pos;
        self.one_of(UNICODE_DASHES)?;
        self.parse_problem_at(
            pos,
            ErrorC,
            1100,
            "This is a unicode dash. Delete and retype as ASCII minus.",
        );
        Ok('-')
    }

    // ----- spacing

    pub fn spacing(&mut self) -> R<String> {
        let x = self.many(|s| {
            s.alt(
                |s| {
                    s.many1(Self::linewhitespace)
                        .map(|v| v.into_iter().collect::<String>())
                },
                Self::continuation,
            )
        })?;
        self.optional(Self::read_comment)?;
        Ok(x.concat())
    }

    fn continuation(&mut self) -> R<String> {
        self.try_(|s| s.string("\\\n"))?;
        let ws: String = self.many(Self::linewhitespace)?.into_iter().collect();
        self.optional(|s| {
            let x = s.read_comment()?;
            if x.ends_with('\\') {
                s.parse_problem(
                    ErrorC,
                    1143,
                    "This backslash is part of a comment and does not continue the line.",
                );
            }
            Ok(())
        })?;
        Ok(ws)
    }

    pub fn spacing1(&mut self) -> R<String> {
        let s = self.spacing()?;
        if s.is_empty() {
            return self.fail("Expected whitespace");
        }
        Ok(s)
    }

    /// `allspacing`.
    pub fn allspacing(&mut self) -> R<String> {
        let mut out = self.spacing()?;
        loop {
            let more = self.option(false, |s| s.linefeed().map(|_| true))?;
            if !more {
                return Ok(out);
            }
            out.push('\n');
            out.push_str(&self.spacing()?);
        }
    }

    pub fn allspacing_or_fail(&mut self) -> R<String> {
        let s = self.allspacing()?;
        if s.is_empty() {
            return self.fail("Expected whitespace");
        }
        Ok(s)
    }

    pub fn read_unicode_quote(&mut self) -> R<Token> {
        let start = self.start_span();
        let quotes = format!("{UNICODE_SINGLE_QUOTES}{UNICODE_DOUBLE_QUOTES}");
        let c = self.one_of(&quotes)?;
        let id = self.end_span(start);
        self.parse_problem_at_id(
            id,
            WarningC,
            1110,
            "This is a unicode quote. Delete and retype it (or quote to make literal).",
        );
        Ok(tk(id, T_Literal(c.to_string())))
    }

    pub fn carriage_return(&mut self) -> R<char> {
        let pos = self.pos;
        self.char_('\r')?;
        self.parse_problem_at(
            pos,
            ErrorC,
            1017,
            "Literal carriage return. Run script through tr -d '\\r' .",
        );
        Ok('\r')
    }

    pub fn almost_space(&mut self) -> R<char> {
        self.parse_note(
            ErrorC,
            1018,
            "This is a unicode space. Delete and retype it.",
        );
        self.one_of(UNICODE_SPACES)?;
        Ok(' ')
    }

    // ----- comments and directives

    pub fn read_annotation_prefix(&mut self) -> R<String> {
        self.char_('#')?;
        self.many(Self::linewhitespace)?;
        self.string("shellcheck")
    }

    pub fn read_annotation(&mut self) -> R<Vec<Annotation>> {
        self.called("shellcheck directive", |s| {
            s.try_(Self::read_annotation_prefix)?;
            s.many1(Self::linewhitespace)?;
            s.read_annotation_without_prefix(true)
        })
    }

    pub fn read_annotation_without_prefix(&mut self, sandboxed: bool) -> R<Vec<Annotation>> {
        let values = self.many1(|s| s.read_key(sandboxed))?;
        self.optional(Self::read_any_comment)?;
        self.alt(
            |s| s.linefeed().map(|_| ()),
            |s| {
                s.alt(Self::eof, |s| {
                    s.parse_note(ErrorC, 1125, "Invalid key=value pair? Ignoring the rest of this directive starting here.");
                    s.many(|s| s.none_of("\n"))?;
                    s.alt(|s| s.linefeed().map(|_| ()), Self::eof)
                })
            },
        )?;
        self.many(Self::linewhitespace)?;
        Ok(values.concat())
    }

    /// `quoted p` in `readAnnotationWithoutPrefix`.
    fn directive_quoted<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let c = self.one_of("'\"")?;
        let start = self.pos;
        let stop = format!("{c}\n");
        let s: String = self.many1(|s| s.none_of(&stop))?.into_iter().collect();
        self.alt(
            |st| st.char_(c).map(|_| ()),
            |st| st.fail("Missing terminating quote for directive."),
        )?;
        self.sub_parse(start, &s, p)
    }

    fn directive_plain_or_quoted<T>(&mut self, mut p: impl FnMut(&mut Self) -> R<T>) -> R<T> {
        let m = self.mark();
        match self.directive_quoted(&mut p) {
            Ok(v) => Ok(v),
            Err(f) if self.consumed_since(&f, &m) => Err(f),
            Err(f) => {
                self.restore(&m);
                self.err = f.err;
                p(self)
            }
        }
    }

    fn directive_string(&mut self) -> R<String> {
        self.alt(
            |s| s.directive_quoted(|s| s.many1(Self::any_char).map(|v| v.into_iter().collect())),
            |s| {
                s.many1(|s| s.none_of(" \n"))
                    .map(|v| v.into_iter().collect())
            },
        )
    }

    fn read_code(&mut self) -> R<i64> {
        self.optional(|s| s.string("SC"))?;
        let digits: String = self.many1(Self::digit)?.into_iter().collect();
        Ok(digits.parse::<i64>().unwrap_or(i64::MAX))
    }

    fn read_key(&mut self, sandboxed: bool) -> R<Vec<Annotation>> {
        let key_pos = self.pos;
        let key: String = self
            .many1(|s| s.alt(Self::letter, |s| s.char_('-')))?
            .into_iter()
            .collect();
        self.alt(
            |s| s.char_('=').map(|_| ()),
            |s| s.fail("Expected '=' after directive key"),
        )?;
        let annotations = match key.as_str() {
            "disable" => self.directive_plain_or_quoted(|s| {
                s.sep_by(
                    |s| {
                        s.alt(
                            |s| {
                                let from = s.read_code()?;
                                let to = s.alt(
                                    |s| {
                                        s.char_('-')?;
                                        s.read_code()
                                    },
                                    |s| s.ok(from + 1),
                                )?;
                                Ok(Annotation::DisableComment(from, to))
                            },
                            |s| {
                                s.string("all")?;
                                Ok(Annotation::DisableComment(0, 1_000_000))
                            },
                        )
                    },
                    |s| s.char_(','),
                )
            })?,
            "enable" => self.directive_plain_or_quoted(|s| {
                s.sep_by(
                    |s| {
                        let name: String = s
                            .many1(|s| s.alt(Self::letter, |s| s.char_('-')))?
                            .into_iter()
                            .collect();
                        Ok(Annotation::EnableComment(name))
                    },
                    |s| s.char_(','),
                )
            })?,
            "source" => vec![Annotation::SourceOverride(self.directive_string()?)],
            "source-path" => vec![Annotation::SourcePath(self.directive_string()?)],
            "shell" => {
                let pos = self.pos;
                let shell = self.directive_string()?;
                if shell_for_executable(&shell).is_none() {
                    self.parse_note_at(
                        pos,
                        ErrorC,
                        1103,
                        "This shell type is unknown. Use e.g. sh or bash.",
                    );
                }
                vec![Annotation::ShellOverride(shell)]
            }
            "extended-analysis" => {
                let pos = self.pos;
                let value: String = self
                    .directive_plain_or_quoted(|s| s.many1(Self::letter))?
                    .into_iter()
                    .collect();
                match value.as_str() {
                    "true" => vec![Annotation::ExtendedAnalysis(true)],
                    "false" => vec![Annotation::ExtendedAnalysis(false)],
                    _ => {
                        self.parse_note_at(
                            pos,
                            ErrorC,
                            1146,
                            "Unknown extended-analysis value. Expected true/false.",
                        );
                        Vec::new()
                    }
                }
            }
            "external-sources" => {
                let pos = self.pos;
                let value: String = self
                    .directive_plain_or_quoted(|s| s.many1(Self::letter))?
                    .into_iter()
                    .collect();
                match value.as_str() {
                    "true" => {
                        if sandboxed {
                            self.parse_note_at(
                                pos,
                                ErrorC,
                                1144,
                                "external-sources can only be enabled in .shellcheckrc, not in individual files.",
                            );
                            Vec::new()
                        } else {
                            vec![Annotation::ExternalSources(true)]
                        }
                    }
                    "false" => vec![Annotation::ExternalSources(false)],
                    _ => {
                        self.parse_note_at(
                            pos,
                            ErrorC,
                            1145,
                            "Unknown external-sources value. Expected true/false.",
                        );
                        Vec::new()
                    }
                }
            }
            _ => {
                self.parse_note_at(
                    key_pos,
                    WarningC,
                    1107,
                    "This directive is unknown. It will be ignored.",
                );
                self.reluctantly_till(Self::any_char, Self::whitespace)?;
                Vec::new()
            }
        };
        self.many(Self::linewhitespace)?;
        Ok(annotations)
    }

    pub fn read_annotations(&mut self) -> R<Vec<Annotation>> {
        let annotations = self.many(|s| s.then_skip(Self::read_annotation, Self::allspacing))?;
        Ok(annotations.concat())
    }

    pub fn read_comment(&mut self) -> R<String> {
        self.unexpecting("shellcheck annotation", Self::read_annotation_prefix)?;
        self.read_any_comment()
    }

    pub fn read_any_comment(&mut self) -> R<String> {
        self.char_('#')?;
        Ok(self.many(|s| s.none_of("\r\n"))?.into_iter().collect())
    }

    // ----- words

    pub fn read_normal_word(&mut self) -> R<Token> {
        self.read_normalish_word("", &["do", "done", "then", "fi", "esac"])
    }

    pub fn read_pattern_word(&mut self) -> R<Token> {
        self.read_normalish_word("", &["esac"])
    }

    pub fn read_normalish_word(&mut self, end: &str, terms: &[&str]) -> R<Token> {
        let start = self.start_span();
        let pos = self.pos;
        let x = self.many1(|s| s.read_normal_word_part(end))?;
        let id = self.end_span(start);
        self.check_possible_termination(pos, &x, terms);
        Ok(tk(id, T_NormalWord(x)))
    }

    pub fn read_index_span(&mut self) -> R<Token> {
        let start = self.start_span();
        let x = self.many(|s| {
            s.alt(
                |s| s.read_normal_word_part("]"),
                |s| {
                    s.alt(
                        |s| {
                            let start = s.start_span();
                            let str_ = s.spacing1()?;
                            let id = s.end_span(start);
                            Ok(tk(id, T_Literal(str_)))
                        },
                        |s| {
                            let start = s.start_span();
                            let str_: String =
                                s.many1(|s| s.one_of(QUOTABLE_CHARS))?.into_iter().collect();
                            let id = s.end_span(start);
                            Ok(tk(id, T_Literal(str_)))
                        },
                    )
                },
            )
        })?;
        let id = self.end_span(start);
        Ok(tk(id, T_NormalWord(x)))
    }

    fn check_possible_termination(&mut self, pos: SourcePos, x: &[Token], terminators: &[&str]) {
        if let [
            Token {
                inner: T_Literal(s),
                ..
            },
        ] = x
            && terminators.contains(&s.as_str())
        {
            self.parse_problem_at(
                pos,
                WarningC,
                1010,
                &format!("Use semicolon or linefeed before '{s}' (or quote to make it literal)."),
            );
        }
    }

    pub fn read_normal_word_part(&mut self, end: &str) -> R<Token> {
        self.not_followed_by2(|s| s.one_of(end))?;
        self.attempting(
            |s| s.ok(()),
            |s| {
                let pos = s.pos;
                s.look_ahead(|s| s.char_('('))?;
                s.parse_problem_at(
                    pos,
                    ErrorC,
                    1036,
                    "'(' is invalid here. Did you forget to escape it?",
                );
                Ok(())
            },
        )?;
        self.choice(&mut [
            &mut Self::read_single_quoted,
            &mut Self::read_double_quoted,
            &mut Self::read_glob,
            &mut Self::read_normal_dollar,
            &mut Self::read_braced,
            &mut Self::read_unquoted_back_ticked,
            &mut Self::read_proc_sub,
            &mut Self::read_unicode_quote,
            &mut |s: &mut Self| s.read_normal_literal(end),
            &mut Self::read_literal_curly_braces,
        ])
    }

    fn read_literal_curly_braces(&mut self) -> R<Token> {
        let start = self.start_span();
        let str_ = self.alt(
            |s| s.try_(|s| s.string("{}")),
            |s| {
                let pos = s.pos;
                let c = s.one_of("{}")?;
                s.parse_problem_at(
                    pos,
                    WarningC,
                    1083,
                    &format!("This {c} is literal. Check expression (missing ;/\\n?) or quote it."),
                );
                Ok(c.to_string())
            },
        )?;
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(str_)))
    }

    pub fn read_space_part(&mut self) -> R<Token> {
        let start = self.start_span();
        let x: String = self.many1(Self::whitespace)?.into_iter().collect();
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(x)))
    }

    pub fn read_dollar_braced_word(&mut self) -> R<Token> {
        let start = self.start_span();
        let list = self.many(Self::read_dollar_braced_part)?;
        let id = self.end_span(start);
        Ok(tk(id, T_NormalWord(list)))
    }

    fn read_dollar_braced_part(&mut self) -> R<Token> {
        self.alt(Self::read_single_quoted, |s| {
            s.alt(Self::read_double_quoted, |s| {
                s.alt(Self::read_param_sub_special_char, |s| {
                    s.alt(Self::read_extglob, |s| {
                        s.alt(Self::read_normal_dollar, |s| {
                            s.alt(
                                Self::read_unquoted_back_ticked,
                                Self::read_dollar_braced_literal,
                            )
                        })
                    })
                })
            })
        })
    }

    fn read_dollar_braced_literal(&mut self) -> R<Token> {
        let start = self.start_span();
        let vars = self.reluctantly_till1(
            |s| {
                s.alt(Self::read_brace_escaped, |s| {
                    s.any_char().map(|c| c.to_string())
                })
            },
            Self::braced_quotable,
        )?;
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(vars.concat())))
    }

    fn read_param_sub_special_char(&mut self) -> R<Token> {
        let start = self.start_span();
        let x: String = self.many1(|s| s.one_of("/:+-=%"))?.into_iter().collect();
        let id = self.end_span(start);
        Ok(tk(id, T_ParamSubSpecialChar(x)))
    }

    pub fn read_proc_sub(&mut self) -> R<Token> {
        self.called("process substitution", |s| {
            let start = s.start_span();
            let dir = s.try_(|s| {
                let x = s.one_of("<>")?;
                s.char_('(')?;
                Ok(x.to_string())
            })?;
            let list = s.read_compound_list_or_empty()?;
            s.allspacing()?;
            s.char_(')')?;
            let id = s.end_span(start);
            Ok(tk(id, T_ProcSub(dir, list)))
        })
    }

    pub fn read_single_quoted(&mut self) -> R<Token> {
        self.called("single quoted string", |s| {
            let start = s.start_span();
            let start_pos = s.pos;
            s.single_quote()?;
            let string = s.many(Self::read_single_quoted_part)?.concat();
            let end_pos = s.pos;
            s.alt(
                |s| s.single_quote().map(|_| ()),
                |s| s.fail("Expected end of single quoted string"),
            )?;
            s.optional(|s| {
                let c = s.try_(|s| {
                    s.look_ahead(|s| s.alt(Self::suspect_char_after_quotes, |s| s.one_of("'")))
                })?;
                if !string.is_empty() && is_alpha(c) && string.chars().last().is_some_and(is_alpha)
                {
                    s.parse_problem_at(
                        end_pos,
                        WarningC,
                        1011,
                        "This apostrophe terminated the single quoted string!",
                    );
                } else if string.contains('\n') && !string.starts_with('\n') {
                    s.suggest_forgot_closing_quote(start_pos, end_pos, "single quoted string");
                }
                Ok(())
            })?;
            let id = s.end_span(start);
            Ok(tk(id, T_SingleQuoted(string)))
        })
    }

    fn read_single_quoted_part(&mut self) -> R<String> {
        let stop = format!("'\\{UNICODE_SINGLE_QUOTES}");
        self.alt(Self::read_single_escaped, |s| {
            s.alt(
                |s| s.many1(|s| s.none_of(&stop)).map(|v| v.into_iter().collect()),
                |s| {
                    let pos = s.pos;
                    let x = s.one_of(UNICODE_SINGLE_QUOTES)?;
                    s.parse_problem_at(
                        pos,
                        WarningC,
                        1112,
                        "This is a unicode quote. Delete and retype it (or ignore/doublequote for literal).",
                    );
                    Ok(x.to_string())
                },
            )
        })
    }

    pub fn read_quoted_back_ticked(&mut self) -> R<Token> {
        self.read_back_ticked(true)
    }

    pub fn read_unquoted_back_ticked(&mut self) -> R<Token> {
        self.read_back_ticked(false)
    }

    fn backtick(&mut self) -> R<()> {
        self.alt(
            |s| s.char_('`').map(|_| ()),
            |s| {
                let pos = s.pos;
                s.char_('´')?;
                s.parse_problem_at(
                    pos,
                    ErrorC,
                    1077,
                    "For command expansion, the tick should slant left (` vs ´). Use $(..) instead.",
                );
                Ok(())
            },
        )
    }

    fn read_back_ticked(&mut self, quoted: bool) -> R<Token> {
        self.called("backtick expansion", |s| {
            let start = s.start_span();
            let start_pos = s.pos;
            s.backtick()?;
            let sub_start = s.pos;
            let sub_string = s.read_generic_literal("`´")?;
            let end_pos = s.pos;
            s.backtick()?;
            let id = s.end_span(start);
            s.optional(|s| {
                s.try_(|s| s.look_ahead(Self::suspect_char_after_quotes))?;
                if sub_string.contains('\n') && !sub_string.starts_with('\n') {
                    s.suggest_forgot_closing_quote(start_pos, end_pos, "backtick expansion");
                }
                Ok(())
            })?;
            let unescaped = unescape_backticks(&sub_string, quoted);
            let result = s.sub_parse(sub_start, &unescaped, |s| {
                s.alt(
                    |s| {
                        s.try_with_errors(|s| {
                            let cmds = s.read_compound_list_or_empty()?;
                            s.verify_eof()?;
                            Ok(cmds)
                        })
                    },
                    |s| s.ok(Vec::new()),
                )
            })?;
            Ok(tk(id, T_Backticked(result)))
        })
    }

    pub fn read_double_quoted(&mut self) -> R<Token> {
        self.called("double quoted string", |s| {
            let start = s.start_span();
            let start_pos = s.pos;
            s.double_quote()?;
            let x = s.many(Self::double_quoted_part)?;
            let end_pos = s.pos;
            s.alt(|s| s.double_quote().map(|_| ()), |s| s.fail("Expected end of double quoted string"))?;
            let id = s.end_span(start);
            s.optional(|s| {
                s.try_(|s| s.look_ahead(|s| s.alt(Self::suspect_char_after_quotes, |s| s.one_of("$\""))))?;
                let has_line_feed = x.iter().any(|t| matches!(&t.inner, T_Literal(s) if s.contains('\n')));
                let starts_with_line_feed = matches!(x.first(), Some(Token { inner: T_Literal(s), .. }) if s.starts_with('\n'));
                if has_line_feed && !starts_with_line_feed {
                    s.suggest_forgot_closing_quote(start_pos, end_pos, "double quoted string");
                }
                Ok(())
            })?;
            Ok(tk(id, T_DoubleQuoted(x)))
        })
    }

    fn suggest_forgot_closing_quote(
        &mut self,
        start_pos: SourcePos,
        end_pos: SourcePos,
        name: &str,
    ) {
        self.parse_problem_at(
            start_pos,
            WarningC,
            1078,
            &format!("Did you forget to close this {name}?"),
        );
        self.parse_problem_at(
            end_pos,
            InfoC,
            1079,
            "This is actually an end quote, but due to next char it looks suspect.",
        );
    }

    pub fn double_quoted_part(&mut self) -> R<Token> {
        self.alt(Self::read_double_literal, |s| {
            s.alt(Self::read_double_quoted_dollar, |s| {
                s.alt(Self::read_quoted_back_ticked, |s| {
                    let pos = s.pos;
                    let start = s.start_span();
                    let c = s.one_of(UNICODE_DOUBLE_QUOTES)?;
                    let id = s.end_span(start);
                    s.parse_problem_at(
                        pos,
                        WarningC,
                        1111,
                        "This is a unicode quote. Delete and retype it (or ignore/singlequote for literal).",
                    );
                    Ok(tk(id, T_Literal(c.to_string())))
                })
            })
        })
    }

    fn read_double_literal(&mut self) -> R<Token> {
        let start = self.start_span();
        let s = self.many1(Self::read_double_literal_part)?;
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(s.concat())))
    }

    fn read_double_literal_part(&mut self) -> R<String> {
        let stop = format!("{DOUBLE_QUOTABLE_CHARS}{UNICODE_DOUBLE_QUOTES}");
        let x = self.many1(|s| {
            s.alt(Self::read_double_escaped, |s| {
                s.many1(|s| s.none_of(&stop))
                    .map(|v| v.into_iter().collect())
            })
        })?;
        Ok(x.concat())
    }

    pub fn read_normal_literal(&mut self, end: &str) -> R<Token> {
        let start = self.start_span();
        let s = self.many1(|s| s.read_normal_literal_part(end))?;
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(s.concat())))
    }

    pub fn read_glob(&mut self) -> R<Token> {
        self.alt(Self::read_extglob, |s| {
            s.alt(
                |s| {
                    let start = s.start_span();
                    let c = s.one_of("*?")?;
                    let id = s.end_span(start);
                    Ok(tk(id, T_Glob(c.to_string())))
                },
                |s| {
                    s.alt(Self::read_glob_class, |s| {
                        let start = s.start_span();
                        let c = s.alt(Self::extglob_start, |s| s.char_('['))?;
                        let id = s.end_span(start);
                        Ok(tk(id, T_Literal(c.to_string())))
                    })
                },
            )
        })
    }

    fn read_glob_class(&mut self) -> R<Token> {
        self.try_(|s| {
            let start = s.start_span();
            s.char_('[')?;
            let negation = s.alt(
                |s| s.one_of("!^").map(|c| c.to_string()),
                |s| s.ok(String::new()),
            )?;
            let leading_bracket = s.alt(
                |s| s.one_of("]").map(|c| c.to_string()),
                |s| s.ok(String::new()),
            )?;
            let globchars = format!("![{EXTGLOB_START_CHARS}");
            let parts = s.many(|s| {
                s.alt(
                    |s| {
                        s.try_(|s| s.string("[:"))?;
                        let name: String = s.many1(Self::letter)?.into_iter().collect();
                        s.string(":]")?;
                        Ok(format!("[:{name}:]"))
                    },
                    |s| {
                        s.alt(
                            |s| s.read_normal_literal_part("]"),
                            |s| s.one_of(&globchars).map(|c| c.to_string()),
                        )
                    },
                )
            })?;
            s.guard(!leading_bracket.is_empty() || !parts.is_empty())?;
            s.char_(']')?;
            let id = s.end_span(start);
            Ok(tk(
                id,
                T_Glob(format!("[{negation}{leading_bracket}{}]", parts.concat())),
            ))
        })
    }

    pub fn read_normal_literal_part(&mut self, custom_end: &str) -> R<String> {
        let stop = format!("{custom_end}{}", standard_end());
        self.alt(Self::read_normal_escaped, |s| {
            s.many1(|s| s.none_of(&stop))
                .map(|v| v.into_iter().collect())
        })
    }

    fn read_normal_escaped(&mut self) -> R<String> {
        self.called("escaped char", |s| {
            let pos = s.pos;
            s.backslash()?;
            s.alt(
                |s| {
                    let next = s.alt(Self::quotable, |s| s.one_of("?*@!+[]{}.,~#"))?;
                    if next == ' ' {
                        s.alt(|s| s.check_trailing_spaces(pos), |s| s.ok(()))?;
                    }
                    if next == '\n' {
                        s.try_(|s| s.look_ahead(Self::spacing))?;
                    }
                    Ok(if next == '\n' { String::new() } else { next.to_string() })
                },
                |s| {
                    let next = s.any_char()?;
                    let escaped_name = match next {
                        'n' => Some("line feed"),
                        't' => Some("tab"),
                        'r' => Some("carriage return"),
                        _ => None,
                    };
                    match escaped_name {
                        Some(name) => {
                            let alternative = if next == 'n' {
                                "a quoted, literal line feed".to_string()
                            } else {
                                format!("\"$(printf '\\{next}')\"")
                            };
                            s.parse_note_at(
                                pos,
                                WarningC,
                                1012,
                                &format!("\\{next} is just literal '{next}' here. For {name}, use {alternative} instead."),
                            );
                        }
                        None => s.parse_note_at(
                            pos,
                            InfoC,
                            1001,
                            &format!("This \\{next} will be a regular '{next}' in this context."),
                        ),
                    }
                    Ok(next.to_string())
                },
            )
        })
    }

    fn check_trailing_spaces(&mut self, pos: SourcePos) -> R<()> {
        self.look_ahead(|s| {
            s.try_(|s| {
                s.many(Self::linewhitespace)?;
                s.alt(|s| s.linefeed().map(|_| ()), Self::eof)?;
                s.parse_problem_at(
                    pos,
                    ErrorC,
                    1101,
                    "Delete trailing spaces after \\ to break line (or use quotes for literal space).",
                );
                Ok(())
            })
        })
    }

    pub fn read_extglob(&mut self) -> R<Token> {
        self.called("extglob", |s| {
            let start = s.start_span();
            let c = s.try_(|s| {
                let f = s.extglob_start()?;
                s.char_('(')?;
                Ok(f)
            })?;
            let contents = s.sep_by(Self::read_extglob_part, |s| s.char_('|'))?;
            s.char_(')')?;
            let id = s.end_span(start);
            Ok(tk(id, T_Extglob(c.to_string(), contents)))
        })
    }

    fn read_extglob_part(&mut self) -> R<Token> {
        let start = self.start_span();
        let x = self.many(|s| {
            s.alt(Self::read_extglob_group, |s| {
                s.alt(
                    |s| s.read_normal_word_part(""),
                    |s| {
                        s.alt(Self::read_space_part, |s| {
                            let start = s.start_span();
                            let str_: String =
                                s.many1(|s| s.one_of("<>#;&"))?.into_iter().collect();
                            let id = s.end_span(start);
                            Ok(tk(id, T_Literal(str_)))
                        })
                    },
                )
            })
        })?;
        let id = self.end_span(start);
        Ok(tk(id, T_NormalWord(x)))
    }

    fn read_extglob_group(&mut self) -> R<Token> {
        self.char_('(')?;
        let start = self.start_span();
        let contents = self.sep_by(Self::read_extglob_part, |s| s.char_('|'))?;
        let id = self.end_span(start);
        self.char_(')')?;
        Ok(tk(id, T_Extglob(String::new(), contents)))
    }

    fn read_single_escaped(&mut self) -> R<String> {
        let pos = self.pos;
        let s = self.backslash()?;
        let x = self.look_ahead(Self::any_char)?;
        if x == '\'' {
            self.parse_problem_at(
                pos,
                InfoC,
                1003,
                "Want to escape a single quote? echo 'This is how it'\\''s done'.",
            );
        }
        Ok(s.to_string())
    }

    fn read_double_escaped(&mut self) -> R<String> {
        let bs = self.backslash()?;
        self.alt(
            |s| s.linefeed().map(|_| String::new()),
            |s| {
                s.alt(
                    |s| s.double_quotable().map(|c| c.to_string()),
                    |s| {
                        let c = s.any_char()?;
                        Ok(format!("{bs}{c}"))
                    },
                )
            },
        )
    }

    fn read_brace_escaped(&mut self) -> R<String> {
        let bs = self.backslash()?;
        self.alt(
            |s| s.linefeed().map(|_| String::new()),
            |s| {
                s.alt(
                    |s| s.braced_quotable().map(|c| c.to_string()),
                    |s| s.any_char().map(|x| format!("{bs}{x}")),
                )
            },
        )
    }

    pub fn read_generic_literal(&mut self, end_chars: &str) -> R<String> {
        let stop = format!("\\{end_chars}");
        let strings = self.many(|s| {
            s.alt(Self::read_generic_escaped, |s| {
                s.many1(|s| s.none_of(&stop))
                    .map(|v| v.into_iter().collect())
            })
        })?;
        Ok(strings.concat())
    }

    pub fn read_generic_literal1<E>(
        &mut self,
        end_exp: impl FnMut(&mut Self) -> R<E>,
    ) -> R<String> {
        let strings = self.reluctantly_till1(
            |s| {
                s.alt(Self::read_generic_escaped, |s| {
                    s.any_char().map(|c| c.to_string())
                })
            },
            end_exp,
        )?;
        Ok(strings.concat())
    }

    fn read_generic_escaped(&mut self) -> R<String> {
        self.backslash()?;
        let x = self.any_char()?;
        Ok(if x == '\n' {
            String::new()
        } else {
            format!("\\{x}")
        })
    }

    pub fn read_braced(&mut self) -> R<Token> {
        self.try_(Self::brace_expansion)
    }

    fn brace_expansion(&mut self) -> R<Token> {
        let start = self.start_span();
        self.char_('{')?;
        let elements = self.sep_by1(Self::braced_element, |s| s.char_(','))?;
        let ok = match elements.as_slice() {
            [] => false,
            [t] => only_literal_string(t).contains(".."),
            _ => true,
        };
        self.guard(ok)?;
        self.char_('}')?;
        let id = self.end_span(start);
        Ok(tk(id, T_BraceExpansion(elements)))
    }

    fn braced_element(&mut self) -> R<Token> {
        let start = self.start_span();
        let parts = self.many(|s| {
            s.choice(&mut [
                &mut Self::brace_expansion,
                &mut Self::read_dollar_expression,
                &mut Self::read_single_quoted,
                &mut Self::read_double_quoted,
                &mut |s: &mut Self| {
                    let start = s.start_span();
                    let lit = s.read_generic_literal1(|s| {
                        s.alt(|s| s.one_of("{}\"$',"), Self::whitespace)
                    })?;
                    let id = s.end_span(start);
                    Ok(tk(id, T_Literal(lit)))
                },
            ])
        })?;
        let id = self.end_span(start);
        Ok(tk(id, T_NormalWord(parts)))
    }

    // ----- dollar expressions

    fn ensure_dollar(&mut self) -> R<char> {
        self.look_ahead(|s| s.char_('$'))
    }

    pub fn read_normal_dollar(&mut self) -> R<Token> {
        self.ensure_dollar()?;
        self.alt(Self::read_dollar_exp, |s| {
            s.alt(Self::read_dollar_double_quote, |s| {
                s.alt(Self::read_dollar_single_quote, |s| {
                    s.read_dollar_lonely(false)
                })
            })
        })
    }

    pub fn read_double_quoted_dollar(&mut self) -> R<Token> {
        self.ensure_dollar()?;
        self.alt(Self::read_dollar_exp, |s| s.read_dollar_lonely(true))
    }

    pub fn read_dollar_expression(&mut self) -> R<Token> {
        self.ensure_dollar()?;
        self.read_dollar_exp()
    }

    fn read_dollar_exp(&mut self) -> R<Token> {
        self.alt(
            |s| {
                s.read_ambiguous("$((", Self::read_dollar_arithmetic, Self::read_dollar_expansion, |s, pos| {
                    s.parse_note_at(
                        pos,
                        ErrorC,
                        1102,
                        "Shells disambiguate $(( differently or not at all. For $(command substitution), add space after $( . For $((arithmetics)), fix parsing errors.",
                    );
                })
            },
            |s| {
                s.alt(Self::read_dollar_expansion, |s| {
                    s.alt(Self::read_dollar_bracket, |s| {
                        s.alt(Self::read_dollar_brace_command_expansion, |s| {
                            s.alt(Self::read_dollar_braced, Self::read_dollar_variable)
                        })
                    })
                })
            },
        )
    }

    fn read_dollar_single_quote(&mut self) -> R<Token> {
        self.called("$'..' expression", |s| {
            let start = s.start_span();
            s.try_(|s| s.string("$'"))?;
            let str_ = s.read_generic_literal("'")?;
            s.char_('\'')?;
            let id = s.end_span(start);
            Ok(tk(id, T_DollarSingleQuoted(str_)))
        })
    }

    fn read_dollar_double_quote(&mut self) -> R<Token> {
        self.look_ahead(|s| s.try_(|s| s.string("$\"")))?;
        let start = self.start_span();
        self.char_('$')?;
        self.double_quote()?;
        let x = self.many(Self::double_quoted_part)?;
        self.alt(
            |s| s.double_quote().map(|_| ()),
            |s| s.fail("Expected end of translated double quoted string"),
        )?;
        let id = self.end_span(start);
        Ok(tk(id, T_DollarDoubleQuoted(x)))
    }

    fn read_dollar_arithmetic(&mut self) -> R<Token> {
        self.called("$((..)) expression", |s| {
            let start = s.start_span();
            s.try_(|s| s.string("$(("))?;
            let c = s.read_arithmetic_contents()?;
            s.char_(')')?;
            s.alt(
                |s| s.char_(')').map(|_| ()),
                |s| s.fail("Expected a double )) to end the $((..))"),
            )?;
            let id = s.end_span(start);
            Ok(tk(id, T_DollarArithmetic(bx(c))))
        })
    }

    fn read_dollar_bracket(&mut self) -> R<Token> {
        self.called("$[..] expression", |s| {
            let start = s.start_span();
            s.try_(|s| s.string("$["))?;
            let c = s.read_arithmetic_contents()?;
            s.string("]")?;
            let id = s.end_span(start);
            Ok(tk(id, T_DollarBracket(bx(c))))
        })
    }

    pub fn read_arithmetic_expression(&mut self) -> R<Token> {
        self.called("((..)) command", |s| {
            let start = s.start_span();
            s.try_(|s| s.string("(("))?;
            let c = s.read_arithmetic_contents()?;
            s.string("))")?;
            let id = s.end_span(start);
            s.spacing()?;
            Ok(tk(id, T_Arithmetic(bx(c))))
        })
    }

    /// `readAmbiguous prefix expected alternative warner`.
    pub fn read_ambiguous(
        &mut self,
        prefix: &str,
        expected: fn(&mut Self) -> R<Token>,
        alternative: fn(&mut Self) -> R<Token>,
        warner: impl FnOnce(&mut Self, SourcePos),
    ) -> R<Token> {
        let pos = self.pos;
        self.try_(|s| s.look_ahead(|s| s.string(prefix)))?;
        self.alt(
            |s| s.try_(expected),
            |s| {
                s.alt(
                    |s| {
                        s.try_(|s| {
                            let t = s.forget_on_failure(alternative)?;
                            warner(s, pos);
                            Ok(t)
                        })
                    },
                    expected,
                )
            },
        )
    }

    fn read_dollar_brace_command_expansion(&mut self) -> R<Token> {
        self.called("ksh-style ${ ..; } command expansion", |s| {
            let start = s.start_span();
            let c = s.try_(|s| {
                s.string("${")?;
                s.alt(|s| s.char_('|'), Self::whitespace)
            })?;
            s.allspacing()?;
            let term = s.read_term()?;
            s.alt(
                |s| s.char_('}').map(|_| ()),
                |s| s.fail("Expected } to end the ksh-style ${ ..; } command expansion"),
            )?;
            let id = s.end_span(start);
            let piped = if c == '|' {
                super::ast::Piped::Piped
            } else {
                super::ast::Piped::Unpiped
            };
            Ok(tk(id, T_DollarBraceCommandExpansion(piped, term)))
        })
    }

    fn read_dollar_braced(&mut self) -> R<Token> {
        self.called("parameter expansion", |s| {
            let start = s.start_span();
            s.try_(|s| s.string("${"))?;
            let word = s.read_dollar_braced_word()?;
            s.char_('}')?;
            let id = s.end_span(start);
            Ok(tk(id, T_DollarBraced(true, bx(word))))
        })
    }

    pub fn read_dollar_expansion(&mut self) -> R<Token> {
        self.called("command expansion", |s| {
            let start = s.start_span();
            s.try_(|s| s.string("$("))?;
            let cmds = s.read_compound_list_or_empty()?;
            s.alt(
                |s| s.char_(')').map(|_| ()),
                |s| s.fail("Expected end of $(..) expression"),
            )?;
            let id = s.end_span(start);
            Ok(tk(id, T_DollarExpansion(cmds)))
        })
    }

    fn wrap_string(&mut self, p: impl FnOnce(&mut Self) -> R<String>) -> R<Token> {
        let start = self.pos;
        let s = p(self)?;
        let end = self.pos;
        let id1 = self.next_id_between(start, end);
        let id2 = self.next_id_between(start, end);
        Ok(tk(id1, T_NormalWord(vec![tk(id2, T_Literal(s))])))
    }

    fn read_dollar_variable(&mut self) -> R<Token> {
        let start = self.start_span();
        let pos = self.pos;
        self.try_(|s| {
            s.char_('$')?;
            s.alt(
                |s| {
                    // positional
                    let value = s.wrap_string(|s| s.digit().map(|c| c.to_string()))?;
                    let id = s.end_span(start);
                    let t = tk(id, T_DollarBraced(false, bx(value)));
                    s.attempting(
                        |s| s.ok(t.clone()),
                        |s| {
                            s.look_ahead(Self::digit)?;
                            s.parse_note_at(pos, ErrorC, 1037, "Braces are required for positionals over 9, e.g. ${10}.");
                            Ok(())
                        },
                    )
                },
                |s| {
                    s.alt(
                        |s| {
                            let value = s.wrap_string(|s| s.special_variable().map(|c| c.to_string()))?;
                            let id = s.end_span(start);
                            Ok(tk(id, T_DollarBraced(false, bx(value))))
                        },
                        |s| {
                            let value = s.wrap_string(Self::read_variable_name)?;
                            let id = s.end_span(start);
                            let t = tk(id, T_DollarBraced(false, bx(value)));
                            s.attempting(
                                |s| s.ok(t.clone()),
                                |s| {
                                    s.look_ahead(|s| s.char_('['))?;
                                    s.parse_note_at(
                                        pos,
                                        ErrorC,
                                        1087,
                                        "Use braces when expanding arrays, e.g. ${array[idx]} (or ${var}[.. to quiet).",
                                    );
                                    Ok(())
                                },
                            )
                        },
                    )
                },
            )
        })
    }

    pub fn read_variable_name(&mut self) -> R<String> {
        let f = self.variable_start()?;
        let rest = self.many(Self::variable_chars)?;
        let mut out = String::new();
        out.push(f);
        out.extend(rest);
        Ok(out)
    }

    fn read_dollar_lonely(&mut self, quoted: bool) -> R<Token> {
        let start = self.start_span();
        self.char_('$')?;
        let id = self.end_span(start);
        if quoted {
            let is_hack = self.option(false, |s| {
                s.try_(|s| {
                    s.look_ahead(|s| {
                        s.char_('"')?;
                        s.optional(|s| s.char_('"'))?;
                        let c = s.alt(Self::variable_start, |s| {
                            s.alt(Self::digit, Self::special_variable)
                        })?;
                        Ok(c != '*' && c != '$')
                    })
                })
            })?;
            if is_hack {
                self.parse_problem_at_id(
                    id,
                    StyleC,
                    1135,
                    "Prefer escape over ending quote to make $ literal. Instead of \"It costs $\"5, use \"It costs \\$5\".",
                );
            }
        }
        Ok(tk(id, T_Literal("$".to_string())))
    }

    // ----- here documents

    pub fn read_here_doc(&mut self) -> R<Token> {
        self.called("here document", |s| {
            let pos = s.pos;
            s.try_(|s| s.string("<<"))?;
            let dashed = s.alt(
                |s| s.char_('-').map(|_| Dashed::Dashed),
                |s| s.ok(Dashed::Undashed),
            )?;
            let sp = s.spacing()?;
            s.optional(|s| {
                s.try_(|s| s.look_ahead(|s| s.char_('(')))?;
                let message =
                    format!("Shells are space sensitive. Use '< <(cmd)', not '<<{sp}(cmd)'.");
                s.parse_problem_at(pos, ErrorC, 1038, &message);
                Ok(())
            })?;
            let start = s.start_span();
            let str_ = s.read_string_for_parser(Self::read_normal_word)?;
            let crstr = s.alt(
                |s| {
                    s.carriage_return()?;
                    Ok(format!("{str_}\r"))
                },
                |s| s.ok(str_.clone()),
            )?;
            let (quoted, end_token) = unquote_here_doc_token(&crstr);
            let hid = s.end_span(start);
            let contexts = s.sys.contexts.clone();
            s.user.pending_here_docs.push(HereDocPending {
                id: hid,
                dashed,
                quoted,
                end_token: end_token.clone(),
                contexts,
            });
            Ok(tk(hid, T_HereDoc(dashed, quoted, end_token, Vec::new())))
        })
    }

    pub fn read_pending_here_docs(&mut self) -> R<()> {
        let docs = std::mem::take(&mut self.user.pending_here_docs);
        for doc in docs {
            self.read_doc(doc)?;
        }
        Ok(())
    }

    fn read_doc(&mut self, doc: HereDocPending) -> R<()> {
        let HereDocPending {
            id,
            dashed,
            quoted,
            end_token,
            contexts,
        } = doc;
        self.swap_context(contexts, |s| {
            let doc_start = s.pos;
            let (terminated, was_warned, lines) = s.read_doc_lines(dashed, &end_token)?;
            let doc_end = s.pos;
            let here_data: String = lines.iter().flat_map(|l| [l.as_str(), "\n"]).collect();
            if !terminated {
                if !was_warned {
                    s.debug_here_doc(id, &end_token, &here_data);
                }
                return s.fail("Here document was not correctly terminated");
            }
            let list = match quoted {
                Quoted::Quoted => {
                    let lid = s.next_id_between(doc_start, doc_end);
                    vec![tk(lid, T_Literal(here_data))]
                }
                Quoted::Unquoted => s.sub_parse(doc_start, &here_data, |s| {
                    s.many(|s| {
                        s.alt(Self::double_quoted_part, |s| {
                            let start = s.start_span();
                            let chars: String =
                                s.many1(|s| s.none_of("`$\\"))?.into_iter().collect();
                            let id = s.end_span(start);
                            Ok(tk(id, T_Literal(chars)))
                        })
                    })
                })?,
            };
            s.user.here_docs.push((id, list));
            Ok(())
        })
    }

    fn read_doc_lines(&mut self, dashed: Dashed, end_token: &str) -> R<(bool, bool, Vec<String>)> {
        let mut lines = Vec::new();
        let mut warned = false;
        loop {
            let pos = self.pos;
            let line: String = self.many(|s| s.none_of("\n"))?.into_iter().collect();
            self.alt(|s| s.char_('\n').map(|_| ()), Self::eof)?;
            let is_eof = self.option(false, |s| s.eof().map(|()| true))?;
            let (is_end, was_warned) =
                self.sub_parse(pos, &line, |s| s.check_here_doc_end(dashed, end_token))?;
            warned |= was_warned;
            if is_end {
                return Ok((true, warned, lines));
            }
            lines.push(line);
            if is_eof {
                return Ok((false, warned, lines));
            }
        }
    }

    fn check_here_doc_end(&mut self, dashed: Dashed, end_token: &str) -> R<(bool, bool)> {
        self.option((false, false), |s| {
            s.try_(|s| {
                let leading_space_pos = s.pos;
                let leading_space: String = s
                    .reluctantly_till(Self::linewhitespace, |s| s.string(end_token))?
                    .into_iter()
                    .collect();
                s.string(end_token)?;
                let trailing_space_pos = s.pos;
                let trailing_space: String = s.many(Self::linewhitespace)?.into_iter().collect();
                let trailer_pos = s.pos;
                let trailer: String = s.many(Self::any_char)?.into_iter().collect();

                let leading_spaces_are_tabs = leading_space.chars().all(|c| c == '\t');
                let there_is_no_trailer = trailing_space.is_empty() && trailer.is_empty();
                let leader_is_ok = leading_space.is_empty() || (dashed == Dashed::Dashed && leading_spaces_are_tabs);
                let trailer_start = trailer.chars().next().unwrap_or('\0');
                let has_trailing_space = !trailing_space.is_empty();
                let has_trailer = !trailer.is_empty();

                if leader_is_ok && there_is_no_trailer {
                    return Ok((true, false));
                }
                let found_cause = (false, true);
                let skip_line = (false, false);
                if trailer_start == ')' {
                    s.parse_problem_at(trailer_pos, ErrorC, 1119, "Add a linefeed between end token and terminating ')'.");
                    Ok(found_cause)
                } else if trailer_start == '#' {
                    s.parse_problem_at(
                        trailer_pos,
                        ErrorC,
                        1120,
                        "No comments allowed after here-doc token. Comment the next line instead.",
                    );
                    Ok(found_cause)
                } else if ";>|&".contains(trailer_start) {
                    s.parse_problem_at(
                        trailer_pos,
                        ErrorC,
                        1121,
                        "Add ;/& terminators (and other syntax) on the line with the <<, not here.",
                    );
                    Ok(found_cause)
                } else if has_trailing_space && has_trailer {
                    s.parse_problem_at(
                        trailer_pos,
                        ErrorC,
                        1122,
                        "Nothing allowed after end token. To continue a command, put it on the line with the <<.",
                    );
                    Ok(found_cause)
                } else if leader_is_ok && has_trailing_space && !has_trailer {
                    s.parse_problem_at(trailing_space_pos, ErrorC, 1118, "Delete whitespace after the here-doc end token.");
                    Ok((true, true))
                } else if !has_trailing_space && has_trailer {
                    Ok(skip_line)
                } else if has_trailer {
                    panic!("ShellCheck internal error, please report: unexpected heredoc trailer");
                } else if dashed == Dashed::Undashed && !leading_space.is_empty() {
                    s.parse_problem_at(
                        leading_space_pos,
                        ErrorC,
                        1039,
                        "Remove indentation before end token (or use <<- and indent with tabs).",
                    );
                    Ok(found_cause)
                } else if dashed == Dashed::Dashed && !leading_spaces_are_tabs {
                    s.parse_problem_at(leading_space_pos, ErrorC, 1040, "When using <<-, you can only indent with tabs.");
                    Ok(found_cause)
                } else {
                    Ok(skip_line)
                }
            })
        })
    }

    fn debug_here_doc(&mut self, token_id: Id, end_token: &str, doc: &str) {
        if doc.contains(end_token) {
            self.parse_problem_at_id(
                token_id,
                ErrorC,
                1041,
                &format!(
                    "Found '{}' further down, but not on a separate line.",
                    e4m(end_token)
                ),
            );
            for line in haskell_lines(doc) {
                if line.contains(end_token) {
                    self.parse_problem_at_id(
                        token_id,
                        ErrorC,
                        1042,
                        &format!(
                            "Close matches include '{}' (!= '{}').",
                            e4m(line),
                            e4m(end_token)
                        ),
                    );
                }
            }
        } else if lower_string(doc).contains(&lower_string(end_token)) {
            self.parse_problem_at_id(
                token_id,
                ErrorC,
                1043,
                &format!(
                    "Found {} further down, but with wrong casing.",
                    e4m(end_token)
                ),
            );
        } else {
            self.parse_problem_at_id(
                token_id,
                ErrorC,
                1044,
                &format!(
                    "Couldn't find end token `{}' in the here document.",
                    e4m(end_token)
                ),
            );
        }
    }

    // ----- redirections

    pub fn read_io_file_op(&mut self) -> R<Token> {
        self.choice(&mut [
            &mut Self::g_dgreat,
            &mut Self::g_lessgreat,
            &mut Self::g_greatand,
            &mut Self::g_lessand,
            &mut Self::g_clobber,
            &mut |s: &mut Self| s.redir_token('<', T_Less),
            &mut |s: &mut Self| s.redir_token('>', T_Greater),
        ])
    }

    fn read_io_duplicate(&mut self) -> R<Token> {
        self.try_(|s| {
            let start = s.start_span();
            let op = s.alt(Self::g_greatand, Self::g_lessand)?;
            let target = s.alt(Self::read_io_variable, |s| {
                let digits: String = s.many(Self::digit)?.into_iter().collect();
                let dash = if digits.is_empty() {
                    s.string("-")?
                } else {
                    s.option(String::new(), |s| s.string("-"))?
                };
                Ok(format!("{digits}{dash}"))
            })?;
            let id = s.end_span(start);
            Ok(tk(id, T_IoDuplicate(bx(op), target)))
        })
    }

    fn read_io_file(&mut self) -> R<Token> {
        self.called("redirection", |s| {
            let start = s.start_span();
            let op = s.read_io_file_op()?;
            s.spacing()?;
            let file = s.read_normal_word()?;
            let id = s.end_span(start);
            Ok(tk(id, T_IoFile(bx(op), bx(file))))
        })
    }

    fn read_io_variable(&mut self) -> R<String> {
        self.try_(|s| {
            s.char_('{')?;
            let x = s.read_variable_name()?;
            s.char_('}')?;
            Ok(format!("{{{x}}}"))
        })
    }

    fn read_io_source(&mut self) -> R<String> {
        self.try_(|s| {
            let x = s.alt(
                |s| s.string("&"),
                |s| {
                    s.alt(Self::read_io_variable, |s| {
                        s.many(Self::digit).map(|v| v.into_iter().collect())
                    })
                },
            )?;
            s.look_ahead(|s| {
                s.alt(
                    |s| s.read_io_file_op().map(|_| ()),
                    |s| s.string("<<").map(|_| ()),
                )
            })?;
            Ok(x)
        })
    }

    pub fn read_io_redirect(&mut self) -> R<Token> {
        let start = self.start_span();
        let n = self.read_io_source()?;
        let redir = self.alt(Self::read_here_string, |s| {
            s.alt(Self::read_here_doc, |s| {
                s.alt(Self::read_io_duplicate, Self::read_io_file)
            })
        })?;
        let id = self.end_span(start);
        self.skip_annotation_and_warn()?;
        self.spacing()?;
        Ok(tk(id, T_FdRedirect(n, bx(redir))))
    }

    fn read_here_string(&mut self) -> R<Token> {
        self.called("here string", |s| {
            let start = s.start_span();
            s.try_(|s| s.string("<<<"))?;
            let id = s.end_span(start);
            s.spacing()?;
            let word = s.read_normal_word()?;
            Ok(tk(id, T_HereString(bx(word))))
        })
    }

    /// `readStringForParser`: the text a parser would consume.
    pub fn read_string_for_parser<T>(
        &mut self,
        parser: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<String> {
        let end_pos = self.in_separate_context(|s| {
            s.look_ahead(|s| {
                parser(s)?;
                Ok(s.pos)
            })
        })?;
        let chars = self.reluctantly_till(Self::any_char, |s| {
            let here = s.pos;
            s.guard(here == end_pos)
        })?;
        Ok(chars.into_iter().collect())
    }

    /// `readLiteralForParser`.
    pub fn read_literal_for_parser<T>(
        &mut self,
        parser: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<Token> {
        let start = self.start_span();
        let s = self.read_string_for_parser(parser)?;
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(s)))
    }
}

/// The token of `<<token`: whether it is quoted, and its text.
fn unquote_here_doc_token(s: &str) -> (Quoted, String) {
    let chars: Vec<char> = s.chars().collect();
    match chars.as_slice() {
        [] | [_] => (Quoted::Unquoted, s.to_string()),
        [cl, .., cr] if cl == cr && (*cl == '"' || *cl == '\'') => {
            (Quoted::Quoted, chars[1..chars.len() - 1].iter().collect())
        }
        _ => {
            if s.contains('\\') {
                (Quoted::Quoted, s.chars().filter(|&c| c != '\\').collect())
            } else {
                (Quoted::Unquoted, s.to_string())
            }
        }
    }
}

/// The text of a backtick expansion as the shell sees it.
fn unescape_backticks(s: &str, quoted: bool) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '\\' && i + 1 < cs.len() {
            let x = cs[i + 1];
            if x == '"' && quoted {
                out.push('"');
                i += 2;
                continue;
            }
            if "$`\\".contains(x) {
                out.push(x);
                i += 2;
                continue;
            }
            if x == '\n' {
                i += 2;
                continue;
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

/// Haskell's `lines`.
pub fn haskell_lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.split('\n').collect()
}
