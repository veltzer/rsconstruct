//! `ShellCheck`'s parser, part two: test expressions (`[ ]`, `[[ ]]`) and
//! arithmetic (`$(( ))`, `(( ))`, `let`, array indices).

use super::ast::{ConditionType, Inner, Token};
use super::astlib::{get_trailing_unquoted_literal, only_literal_string};
use super::data::{BINARY_TEST_OPS, COMMON_COMMANDS};
use super::parsec::{
    Alternative, P, R,
    Severity::{ErrorC, WarningC},
};
use super::parser::{bx, tk};

use Inner::{
    T_Condition, T_Literal, T_NormalWord, T_UnparsedIndex, TA_Assignment, TA_Binary, TA_Expansion,
    TA_Parenthesis, TA_Sequence, TA_Trinary, TA_Unary, TA_Variable, TC_And, TC_Binary, TC_Empty,
    TC_Group, TC_Nullary, TC_Or, TC_Unary,
};

type Combine = Box<dyn FnOnce(Token, Token) -> Token>;

impl P {
    /// `readConditionContents single`.
    pub fn read_condition_contents(&mut self, single: bool) -> R<Token> {
        self.attempting(
            |s| s.read_cond_contents(single),
            |s| {
                s.look_ahead(|s| {
                    let pos = s.pos;
                    let name = s.read_variable_name()?;
                    s.spacing1()?;
                    if COMMON_COMMANDS.contains(&name.as_str()) {
                        s.parse_problem_at(
                            pos,
                            WarningC,
                            1014,
                            "Use 'if cmd; then ..' to check exit code, or 'if [[ $(cmd) == .. ]]' to check output.",
                        );
                    }
                    Ok(())
                })
            },
        )
    }

    const fn cond_type(single: bool) -> ConditionType {
        if single {
            ConditionType::SingleBracket
        } else {
            ConditionType::DoubleBracket
        }
    }

    fn cond_spacing(&mut self, single: bool, required: bool) -> R<String> {
        let pos = self.pos;
        let space = self.allspacing()?;
        if required && space.is_empty() {
            self.parse_problem_at(pos, ErrorC, 1035, "You are missing a required space here.");
        }
        if single && space.contains('\n') {
            self.parse_problem_at(
                pos,
                ErrorC,
                1080,
                "When breaking lines in [ ], you need \\ before the linefeed.",
            );
        }
        Ok(space)
    }

    fn spacing_or_lf(&mut self, single: bool) -> R<String> {
        self.cond_spacing(single, true)
    }

    fn read_cond_binary_op(&mut self, single: bool) -> R<(Combine, bool)> {
        self.try_(|s| {
            s.optional(|s| s.guard_arithmetic(single))?;
            let start = s.start_span();
            let op = s.read_regular_or_escaped(|s| {
                s.alt(
                    |s| {
                        s.try_(|s| {
                            let op = s.read_op()?;
                            if op == "-a" || op == "-o" {
                                return s.fail("Unexpected operator");
                            }
                            Ok(op)
                        })
                    },
                    |s| {
                        s.alt(
                            |s| {
                                s.choice(&mut [
                                    &mut |s: &mut Self| s.try_(|s| s.string("==")),
                                    &mut |s: &mut Self| s.try_(|s| s.string("!=")),
                                    &mut |s: &mut Self| s.try_(|s| s.string("<=")),
                                    &mut |s: &mut Self| s.try_(|s| s.string(">=")),
                                    &mut |s: &mut Self| s.try_(|s| s.string("=~")),
                                    &mut |s: &mut Self| s.try_(|s| s.string(">")),
                                    &mut |s: &mut Self| s.try_(|s| s.string("<")),
                                    &mut |s: &mut Self| s.try_(|s| s.string("=")),
                                ])
                            },
                            |s| {
                                s.fail(
                                    "Expected comparison operator (don't wrap commands in []/[[]])",
                                )
                            },
                        )
                    },
                )
            })?;
            let id = s.end_span(start);
            s.spacing_or_lf(single)?;
            let typ = Self::cond_type(single);
            let f: Combine = Box::new(move |x, y| tk(id, TC_Binary(typ, op, bx(x), bx(y))));
            Ok((f, true))
        })
    }

    /// `readEscaped p`: an operator written as `\<` or `"<"`.
    fn read_escaped(&mut self, mut p: impl FnMut(&mut Self) -> R<String>) -> R<String> {
        fn escaped(s: String) -> String {
            if s.chars().any(|c| "<>()".contains(c)) {
                format!("\\{s}")
            } else {
                s
            }
        }
        self.try_(|s| {
            let m = s.mark();
            let with_escape = (|| -> R<String> {
                s.char_('\\')?;
                p(s).map(escaped)
            })();
            match with_escape {
                Ok(v) => Ok(v),
                Err(fail) if s.consumed_since(&fail, &m) => Err(fail),
                Err(fail) => {
                    s.restore(&m);
                    s.err = fail.err;
                    let quote = s.one_of("'\"")?;
                    let v = p(s)?;
                    s.char_(quote)?;
                    Ok(escaped(v))
                }
            }
        })
    }

    fn read_regular_or_escaped(&mut self, mut p: impl FnMut(&mut Self) -> R<String>) -> R<String> {
        let m = self.mark();
        match self.read_escaped(&mut p) {
            Ok(v) => Ok(v),
            Err(f) if self.consumed_since(&f, &m) => Err(f),
            Err(f) => {
                self.restore(&m);
                self.err = f.err;
                p(self)
            }
        }
    }

    fn guard_arithmetic(&mut self, single: bool) -> R<()> {
        self.try_(|s| {
            s.look_ahead(|s| {
                s.alt(
                    |s| s.one_of("+*/%").map(|_| ()),
                    |s| s.string("- ").map(|_| ()),
                )
            })
        })?;
        self.parse_problem(
            ErrorC,
            1076,
            if single {
                "Trying to do math? Use e.g. [ $((i/2+7)) -ge 18 ]."
            } else {
                "Trying to do math? Use e.g. [[ $((i/2+7)) -ge 18 ]]."
            },
        );
        Ok(())
    }

    fn read_cond_unary_exp(&mut self, single: bool) -> R<Token> {
        let (id, op) = self.read_cond_unary_op(single)?;
        let pos = self.pos;
        let typ = Self::cond_type(single);
        self.or_fail(
            |s| {
                s.read_cond_word(single)
                    .map(|w| tk(id, TC_Unary(typ, op, bx(w))))
            },
            |s| {
                s.parse_problem_at(
                    pos,
                    ErrorC,
                    1019,
                    "Expected this to be an argument to the unary condition.",
                );
                "Expected an argument for the unary operator".to_string()
            },
        )
    }

    fn read_cond_unary_op(&mut self, single: bool) -> R<(super::ast::Id, String)> {
        self.try_(|s| {
            let start = s.start_span();
            let op = s.read_op()?;
            let id = s.end_span(start);
            s.spacing_or_lf(single)?;
            Ok((id, op))
        })
    }

    fn read_op(&mut self) -> R<String> {
        self.try_(|s| {
            s.alt(|s| s.char_('-'), Self::weird_dash)?;
            let letters: String = s
                .alt(
                    |s| s.many1(Self::letter),
                    |s| s.fail("Expected a test operator"),
                )?
                .into_iter()
                .collect();
            Ok(format!("-{letters}"))
        })
    }

    fn read_cond_word(&mut self, single: bool) -> R<Token> {
        self.not_followed_by2(|s| {
            s.try_(|s| {
                s.spacing()?;
                s.string("]")
            })
        })?;
        let x = self.read_normal_word()?;
        let pos = self.pos;
        let ended_with = |suffix: &str, x: &Token| match &x.inner {
            T_NormalWord(parts) => {
                matches!(parts.last(), Some(Token { inner: T_Literal(s), .. }) if s.ends_with(suffix))
            }
            _ => false,
        };
        let not_array_index = match &x.inner {
            T_NormalWord(parts) if parts.len() >= 2 => match &parts[1].inner {
                T_Literal(t) => t != "[",
                _ => true,
            },
            _ => true,
        };
        if not_array_index && ended_with("]", &x) && !only_literal_string(&x).contains('[') {
            self.parse_problem_at(
                pos,
                ErrorC,
                1020,
                &format!(
                    "You need a space before the {}.",
                    if single { "]" } else { "]]" }
                ),
            );
            return self.fail("Missing space before ]");
        }
        if single && ended_with(")", &x) {
            self.parse_problem_at(pos, ErrorC, 1021, "You need a space before the \\)");
            return self.fail("Missing space before )");
        }
        self.spacing()?;
        Ok(x)
    }

    fn read_cond_and_op(&mut self, single: bool) -> R<Combine> {
        self.alt(
            |s| s.read_and_or_op(single, true, "&&", false),
            |s| s.read_and_or_op(single, true, "-a", true),
        )
    }

    fn read_cond_or_op(&mut self, single: bool) -> R<Combine> {
        self.optional(|s| s.guard_arithmetic(single))?;
        self.alt(
            |s| s.read_and_or_op(single, false, "||", false),
            |s| s.read_and_or_op(single, false, "-o", true),
        )
    }

    fn read_and_or_op(
        &mut self,
        single: bool,
        and: bool,
        op: &str,
        requires_spacing: bool,
    ) -> R<Combine> {
        self.optional(|s| s.look_ahead(Self::weird_dash))?;
        let start = self.start_span();
        let x = self.try_(|s| s.string(op))?;
        let id = self.end_span(start);
        self.cond_spacing(single, requires_spacing)?;
        let typ = Self::cond_type(single);
        Ok(if and {
            Box::new(move |a, b| tk(id, TC_And(typ, x, bx(a), bx(b))))
        } else {
            Box::new(move |a, b| tk(id, TC_Or(typ, x, bx(a), bx(b))))
        })
    }

    fn read_cond_nullary_or_binary(&mut self, single: bool) -> R<Token> {
        let start = self.start_span();
        let x = self.attempting(
            |s| s.read_cond_word(single),
            |s| {
                let pos = s.pos;
                s.look_ahead(|s| s.char_('['))?;
                s.parse_problem_at(
                    pos,
                    ErrorC,
                    1026,
                    if single {
                        "If grouping expressions inside [..], use \\( ..\\)."
                    } else {
                        "If grouping expressions inside [[..]], use ( .. )."
                    },
                );
                Ok(())
            },
        )?;
        let id = self.end_span(start);
        let typ = Self::cond_type(single);
        let x2 = x.clone();
        self.alt(
            |s| {
                let pos = s.pos;
                let is_regex = s.regex_operator_ahead()?;
                let (op, _) = s.read_cond_binary_op(single)?;
                let y = if is_regex {
                    s.read_regex()?
                } else {
                    s.alt(
                        |s| s.read_cond_word(single),
                        |s| {
                            s.parse_problem_at(
                                pos,
                                ErrorC,
                                1027,
                                "Expected another argument for this operator.",
                            );
                            s.zero()
                        },
                    )?
                };
                Ok(op(x, y))
            },
            |s| {
                s.check_trailing_op(&x2);
                Ok(tk(id, TC_Nullary(typ, bx(x2))))
            },
        )
    }

    fn check_trailing_op(&mut self, x: &Token) {
        if let Some(Token {
            id,
            inner: T_Literal(s),
        }) = get_trailing_unquoted_literal(x)
            && let Some(op) = BINARY_TEST_OPS.iter().find(|op| s.ends_with(*op))
        {
            let id = *id;
            self.parse_problem_at_id(
                id,
                ErrorC,
                1108,
                &format!("You need a space before and after the {op} ."),
            );
        }
    }

    fn read_cond_group(&mut self, single: bool) -> R<Token> {
        let start = self.start_span();
        let pos = self.pos;
        let lparen = self.try_(|s| s.read_regular_or_escaped(|s| s.string("(")))?;
        if single && lparen == "(" {
            self.cond_single_warning(pos);
        }
        if !single && lparen == "\\(" {
            self.cond_double_warning(pos);
        }
        self.cond_spacing(single, single)?;
        let x = self.read_cond_contents(single)?;
        let cpos = self.pos;
        let rparen = self.read_regular_or_escaped(|s| s.string(")"))?;
        let id = self.end_span(start);
        self.cond_spacing(single, single)?;
        if single && rparen == ")" {
            self.cond_single_warning(cpos);
        }
        if !single && rparen == "\\)" {
            self.cond_double_warning(cpos);
        }
        Ok(tk(id, TC_Group(Self::cond_type(single), bx(x))))
    }

    fn cond_single_warning(&mut self, pos: super::ast::SourcePos) {
        self.parse_problem_at(
            pos,
            ErrorC,
            1028,
            "In [..] you have to escape \\( \\) or preferably combine [..] expressions.",
        );
    }

    fn cond_double_warning(&mut self, pos: super::ast::SourcePos) {
        self.parse_problem_at(pos, ErrorC, 1029, "In [[..]] you shouldn't escape ( or ).");
    }

    fn regex_operator_ahead(&mut self) -> R<bool> {
        self.alt(
            |s| {
                s.look_ahead(|s| {
                    s.alt(
                        |s| s.try_(|s| s.string("=~")),
                        |s| s.try_(|s| s.string("~=")),
                    )?;
                    Ok(true)
                })
            },
            |s| s.ok(false),
        )
    }

    fn read_regex(&mut self) -> R<Token> {
        self.called("regex", |s| {
            let start = s.start_span();
            let parts = s.many1(Self::read_regex_part)?;
            let id = s.end_span(start);
            s.spacing()?;
            Ok(tk(id, T_NormalWord(parts)))
        })
    }

    fn read_regex_part(&mut self) -> R<Token> {
        self.choice(&mut [
            &mut Self::read_regex_group,
            &mut Self::read_single_quoted,
            &mut Self::read_double_quoted,
            &mut Self::read_dollar_expression,
            &mut |s: &mut Self| s.read_literal_for_parser(|s| s.read_normal_literal("( ")),
            &mut |s: &mut Self| s.read_literal_string("|"),
            &mut |s: &mut Self| {
                let start = s.start_span();
                let c = s.alt(Self::extglob_start, |s| s.one_of("{}[]$"))?;
                let id = s.end_span(start);
                Ok(tk(id, T_Literal(c.to_string())))
            },
        ])
    }

    fn read_regex_group(&mut self) -> R<Token> {
        self.called("regex grouping", |s| {
            let start = s.start_span();
            let p1 = s.read_literal_string("(")?;
            let parts = s.many(|s| {
                s.alt(Self::read_regex_part, |s| {
                    let start = s.start_span();
                    let str_ = s.read_generic_literal1(|s| {
                        s.alt(Self::single_quote, |s| {
                            s.alt(Self::double_quotable, |s| s.one_of("()"))
                        })
                    })?;
                    let id = s.end_span(start);
                    Ok(tk(id, T_Literal(str_)))
                })
            })?;
            let p2 = s.read_literal_string(")")?;
            let id = s.end_span(start);
            let mut all = vec![p1];
            all.extend(parts);
            all.push(p2);
            Ok(tk(id, T_NormalWord(all)))
        })
    }

    fn read_literal_string(&mut self, lit: &str) -> R<Token> {
        let start = self.start_span();
        let str_ = self.string(lit)?;
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(str_)))
    }

    fn read_cond_term(&mut self, single: bool) -> R<Token> {
        let term = self.alt(|s| s.read_cond_not(single), |s| s.read_cond_expr(single))?;
        self.cond_spacing(single, false)?;
        Ok(term)
    }

    fn read_cond_not(&mut self, single: bool) -> R<Token> {
        let start = self.start_span();
        self.char_('!')?;
        let id = self.end_span(start);
        self.spacing_or_lf(single)?;
        let expr = self.read_cond_expr(single)?;
        Ok(tk(
            id,
            TC_Unary(Self::cond_type(single), "!".to_string(), bx(expr)),
        ))
    }

    fn read_cond_expr(&mut self, single: bool) -> R<Token> {
        self.alt(
            |s| s.read_cond_group(single),
            |s| {
                s.alt(
                    |s| s.read_cond_unary_exp(single),
                    |s| s.read_cond_nullary_or_binary(single),
                )
            },
        )
    }

    fn read_cond_contents(&mut self, single: bool) -> R<Token> {
        // readCondOr = chainl1 readCondAnd readCondAndOp
        // readCondAnd = chainl1 readCondTerm readCondOrOp
        self.chainl1(
            |s| s.chainl1(|s| s.read_cond_term(single), |s| s.read_cond_or_op(single)),
            |s| s.read_cond_and_op(single),
        )
    }

    // ----- arithmetic

    /// `readArithmeticContents`.
    pub fn read_arithmetic_contents(&mut self) -> R<Token> {
        self.arith_sequence()
    }

    /// The arithmetic `spacing`: whitespace and escaped line feeds.
    fn arith_spacing(&mut self) -> R<String> {
        let v = self.many(|s| {
            s.alt(Self::whitespace, |s| {
                s.try_(|s| s.string("\\\n"))?;
                Ok('\n')
            })
        })?;
        Ok(v.into_iter().collect())
    }

    fn fail_if_incomplete_op(&mut self) -> R<()> {
        self.not_followed_by2(|s| s.one_of("&|<>="))
    }

    fn read_combo_op(&mut self, ops: &[&str], assignment: bool) -> R<Combine> {
        let start = self.start_span();
        let mut alts: Vec<Box<Alternative<'static, String>>> = ops
            .iter()
            .map(|op| {
                let op = (*op).to_string();
                Box::new(move |s: &mut Self| {
                    s.try_(|s| {
                        let v = s.string(&op)?;
                        s.fail_if_incomplete_op()?;
                        Ok(v)
                    })
                }) as Box<Alternative<'static, String>>
            })
            .collect();
        let mut refs: Vec<&mut Alternative<'_, String>> = alts
            .iter_mut()
            .map(|b| b.as_mut() as &mut Alternative<'_, String>)
            .collect();
        let op = self.choice(&mut refs)?;
        let id = self.end_span(start);
        self.arith_spacing()?;
        Ok(if assignment {
            Box::new(move |a, b| tk(id, TA_Assignment(op, bx(a), bx(b))))
        } else {
            Box::new(move |a, b| tk(id, TA_Binary(op, bx(a), bx(b))))
        })
    }

    fn split_by(&mut self, x: fn(&mut Self) -> R<Token>, ops: &[&str]) -> R<Token> {
        self.chainl1(x, |s| s.read_combo_op(ops, false))
    }

    fn read_minus_op(&mut self) -> R<Combine> {
        let start = self.start_span();
        let pos = self.pos;
        self.try_(|s| {
            s.char_('-')?;
            s.fail_if_incomplete_op()
        })?;
        self.optional(|s| {
            let (str_, alt) = s.look_ahead(|s| {
                s.choice(&mut [
                    &mut |s: &mut Self| s.arith_try_op("lt", "<"),
                    &mut |s: &mut Self| s.arith_try_op("gt", ">"),
                    &mut |s: &mut Self| s.arith_try_op("le", "<="),
                    &mut |s: &mut Self| s.arith_try_op("ge", ">="),
                    &mut |s: &mut Self| s.arith_try_op("eq", "=="),
                    &mut |s: &mut Self| s.arith_try_op("ne", "!="),
                ])
            })?;
            s.parse_problem_at(
                pos,
                ErrorC,
                1106,
                &format!("In arithmetic contexts, use {alt} instead of -{str_}"),
            );
            Ok(())
        })?;
        let id = self.end_span(start);
        self.arith_spacing()?;
        Ok(Box::new(move |a, b| {
            tk(id, TA_Binary("-".to_string(), bx(a), bx(b)))
        }))
    }

    fn arith_try_op(
        &mut self,
        str_: &'static str,
        alt: &'static str,
    ) -> R<(&'static str, &'static str)> {
        self.try_(|s| {
            s.string(str_)?;
            s.spacing1()?;
            Ok((str_, alt))
        })
    }

    fn arith_array_index(&mut self) -> R<Token> {
        let start = self.start_span();
        self.char_('[')?;
        let pos = self.pos;
        let middle = self.read_string_for_parser(Self::read_arithmetic_contents)?;
        self.char_(']')?;
        let id = self.end_span(start);
        Ok(tk(id, T_UnparsedIndex(pos, middle)))
    }

    fn arith_literal(&mut self, lit: &str) -> R<Token> {
        let start = self.start_span();
        self.string(lit)?;
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(lit.to_string())))
    }

    fn arith_variable(&mut self) -> R<Token> {
        let start = self.start_span();
        let name = self.read_variable_name()?;
        let indices = self.many(Self::arith_array_index)?;
        let id = self.end_span(start);
        self.arith_spacing()?;
        Ok(tk(id, TA_Variable(name, indices)))
    }

    fn arith_expansion(&mut self) -> R<Token> {
        let start = self.start_span();
        let pieces = self.many1(|s| {
            s.choice(&mut [
                &mut Self::read_single_quoted,
                &mut Self::read_double_quoted,
                &mut Self::read_normal_dollar,
                &mut Self::read_braced,
                &mut Self::read_unquoted_back_ticked,
                &mut |s: &mut Self| s.arith_literal("#"),
                &mut |s: &mut Self| s.read_normal_literal("+-*/=%^,]?:"),
            ])
        })?;
        let id = self.end_span(start);
        self.arith_spacing()?;
        Ok(tk(id, TA_Expansion(pieces)))
    }

    fn arith_group(&mut self) -> R<Token> {
        let start = self.start_span();
        self.char_('(')?;
        let s = self.arith_sequence()?;
        self.char_(')')?;
        let id = self.end_span(start);
        self.arith_spacing()?;
        Ok(tk(id, TA_Parenthesis(bx(s))))
    }

    fn arith_term(&mut self) -> R<Token> {
        self.alt(Self::arith_group, |s| {
            s.alt(Self::arith_variable, Self::arith_expansion)
        })
    }

    fn arith_sequence(&mut self) -> R<Token> {
        self.arith_spacing()?;
        let start = self.start_span();
        let l = self.sep_by(Self::arith_assignment, |s| {
            s.char_(',')?;
            s.arith_spacing()
        })?;
        let id = self.end_span(start);
        Ok(tk(id, TA_Sequence(l)))
    }

    fn arith_assignment(&mut self) -> R<Token> {
        self.chainr1(&mut Self::arith_trinary, &mut |s: &mut Self| {
            s.read_combo_op(
                &[
                    "=", "*=", "/=", "%=", "+=", "-=", "<<=", ">>=", "&=", "^=", "|=",
                ],
                true,
            )
        })
    }

    fn arith_trinary(&mut self) -> R<Token> {
        let x = self.arith_logical_or()?;
        let x2 = x.clone();
        self.alt(
            |s| {
                let start = s.start_span();
                s.string("?")?;
                s.arith_spacing()?;
                let y = s.arith_trinary()?;
                s.string(":")?;
                s.arith_spacing()?;
                let z = s.arith_trinary()?;
                let id = s.end_span(start);
                Ok(tk(id, TA_Trinary(bx(x), bx(y), bx(z))))
            },
            |s| s.ok(x2),
        )
    }

    fn arith_logical_or(&mut self) -> R<Token> {
        self.split_by(Self::arith_logical_and, &["||"])
    }

    fn arith_logical_and(&mut self) -> R<Token> {
        self.split_by(Self::arith_bit_or, &["&&"])
    }

    fn arith_bit_or(&mut self) -> R<Token> {
        self.split_by(Self::arith_bit_xor, &["|"])
    }

    fn arith_bit_xor(&mut self) -> R<Token> {
        self.split_by(Self::arith_bit_and, &["^"])
    }

    fn arith_bit_and(&mut self) -> R<Token> {
        self.split_by(Self::arith_equated, &["&"])
    }

    fn arith_equated(&mut self) -> R<Token> {
        self.split_by(Self::arith_compared, &["==", "!="])
    }

    fn arith_compared(&mut self) -> R<Token> {
        self.split_by(Self::arith_shift, &["<=", ">=", "<", ">"])
    }

    fn arith_shift(&mut self) -> R<Token> {
        self.split_by(Self::arith_addition, &["<<", ">>"])
    }

    fn arith_addition(&mut self) -> R<Token> {
        self.chainl1(Self::arith_multiplication, |s| {
            s.alt(|s| s.read_combo_op(&["+"], false), Self::read_minus_op)
        })
    }

    fn arith_multiplication(&mut self) -> R<Token> {
        self.split_by(Self::arith_exponential, &["*", "/", "%"])
    }

    fn arith_exponential(&mut self) -> R<Token> {
        self.split_by(Self::arith_any_negated, &["**"])
    }

    fn arith_any_negated(&mut self) -> R<Token> {
        self.alt(
            |s| {
                let start = s.start_span();
                let op = s.one_of("!~")?;
                let id = s.end_span(start);
                s.arith_spacing()?;
                let x = s.arith_any_negated()?;
                Ok(tk(id, TA_Unary(op.to_string(), bx(x))))
            },
            Self::arith_any_signed,
        )
    }

    fn arith_any_signed(&mut self) -> R<Token> {
        self.alt(
            |s| {
                let start = s.start_span();
                let op = s.choice(&mut [
                    &mut |s: &mut Self| s.arith_sign_op('+'),
                    &mut |s: &mut Self| s.arith_sign_op('-'),
                ])?;
                let id = s.end_span(start);
                s.arith_spacing()?;
                let x = s.arith_anycremented()?;
                Ok(tk(id, TA_Unary(op.to_string(), bx(x))))
            },
            Self::arith_anycremented,
        )
    }

    fn arith_sign_op(&mut self, c: char) -> R<char> {
        self.try_(|s| {
            s.char_(c)?;
            s.not_followed_by2(|s| s.char_(c))?;
            s.arith_spacing()?;
            Ok(c)
        })
    }

    fn arith_increment_op(&mut self) -> R<String> {
        self.try_(|s| s.alt(|s| s.string("++"), |s| s.string("--")))
    }

    fn arith_anycremented(&mut self) -> R<Token> {
        self.alt(
            |s| {
                let x = s.arith_term()?;
                s.arith_spacing()?;
                let x2 = x.clone();
                s.alt(
                    |s| {
                        let start = s.start_span();
                        let op = s.arith_increment_op()?;
                        let id = s.end_span(start);
                        s.arith_spacing()?;
                        Ok(tk(id, TA_Unary(format!("|{op}"), bx(x))))
                    },
                    |s| s.ok(x2),
                )
            },
            |s| {
                let start = s.start_span();
                let op = s.arith_increment_op()?;
                let id = s.end_span(start);
                s.arith_spacing()?;
                let x = s.arith_term()?;
                Ok(tk(id, TA_Unary(format!("{op}|"), bx(x))))
            },
        )
    }

    // ----- [ ] and [[ ]]

    pub fn read_condition(&mut self) -> R<Token> {
        self.called("test expression", |s| {
            let opos = s.pos;
            let start = s.start_span();
            let open = s.alt(|s| s.try_(|s| s.string("[[")), |s| s.string("["))?;
            let single = open == "[";
            let typ = Self::cond_type(single);
            let pos = s.pos;
            let space = s.allspacing()?;
            if space.is_empty() {
                s.parse_problem_at_with_end(
                    opos,
                    pos,
                    ErrorC,
                    1035,
                    &format!(
                        "You need a space after the {}",
                        if single { "[ and before the ]." } else { "[[ and before the ]]." }
                    ),
                );
            }
            if single && space.contains('\n') {
                s.parse_problem_at(pos, ErrorC, 1080, "You need \\ before line feeds to break lines in [ ].");
            }
            let condition = s.alt(
                |s| s.read_condition_contents(single),
                |s| {
                    s.guard(!space.is_empty())?;
                    s.look_ahead(|s| s.string("]"))?;
                    let id = s.end_span(start);
                    Ok(tk(id, TC_Empty(typ)))
                },
            )?;
            let cpos = s.pos;
            let close = s.alt(
                |s| s.try_(|s| s.string("]]")),
                |s| s.alt(|s| s.string("]"), |s| s.fail("Expected test to end here (don't wrap commands in []/[[]])")),
            )?;
            let id = s.end_span(start);
            if open == "[[" && close != "]]" {
                s.parse_problem_at(
                    cpos,
                    ErrorC,
                    1033,
                    "Test expression was opened with double [[ but closed with single ]. Make sure they match.",
                );
            }
            if open == "[" && close != "]" {
                s.parse_problem_at(
                    opos,
                    ErrorC,
                    1034,
                    "Test expression was opened with single [ but closed with double ]]. Make sure they match.",
                );
            }
            s.spacing()?;
            Ok(tk(id, T_Condition(typ, bx(condition))))
        })
    }
}
