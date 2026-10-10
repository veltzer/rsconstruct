//! `PPI::Lexer`: the tokens into statements and structures, and
//! `PPI::Document::index_locations`.

use super::tokenizer::{Error, Parser};
use super::{Class, DOCUMENT, Doc, Id, Location, count_byte, is_space};

/// `PPI::Document->new`: the document, or PPI's error string.
pub fn parse(source: &[u8], filename: &[u8]) -> Result<Doc, Error> {
    let mut parser = Parser::new(source, filename);
    parser.lex_document()?;
    let mut doc = parser.doc;
    index_locations(&mut doc);
    Ok(doc)
}

/// `%STATEMENT_CLASSES`.
const fn statement_class(word: &[u8]) -> Option<Class> {
    Some(match word {
        b"BEGIN" | b"CHECK" | b"UNITCHECK" | b"INIT" | b"END" => Class::StatementScheduled,
        b"AUTOLOAD" | b"DESTROY" => Class::StatementSub,
        b"package" => Class::StatementPackage,
        b"no" | b"require" => Class::StatementInclude,
        b"my" | b"local" | b"our" | b"state" => Class::StatementVariable,
        b"if" | b"unless" | b"for" | b"foreach" | b"while" | b"until" => Class::StatementCompound,
        b"given" => Class::StatementGiven,
        b"when" | b"default" => Class::StatementWhen,
        b"redo" | b"next" | b"last" | b"return" | b"goto" => Class::StatementBreak,
        b"__DATA__" => Class::StatementData,
        b"__END__" => Class::StatementEnd,
        _ => return None,
    })
}

/// `%CURLY_CLASSES`.
const fn curly_class(content: &[u8]) -> Option<Class> {
    Some(match content {
        b"sub" | b"grep" | b"map" | b"sort" | b"do" => Class::StructureBlock,
        b"scalar" | b"=" | b"||=" | b"&&=" | b"//=" | b"||" | b"&&" | b"//" | b"?" | b":"
        | b"," | b"=>" | b"+" | b"return" | b"bless" => Class::StructureConstructor,
        _ => return None,
    })
}

const fn opposite(brace: &[u8]) -> &'static [u8] {
    match brace {
        b"{" => b"}",
        b"}" => b"{",
        b"[" => b"]",
        b"]" => b"[",
        b"(" => b")",
        _ => b"(",
    }
}

impl Parser {
    fn lex_get_token(&mut self) -> Result<Option<Id>, Error> {
        if let Some(token) = self.buffer.pop_front() {
            return Ok(Some(token));
        }
        self.get_token()
    }

    fn sig(&self, id: Id) -> bool {
        self.doc.significant(id)
    }

    fn text(&self, id: Id) -> &[u8] {
        &self.doc.elems[id].content
    }

    fn is(&self, id: Id, class: Class, content: &[u8]) -> bool {
        self.doc.isa(id, class) && self.text(id) == content
    }

    fn opens(&self, id: Id) -> bool {
        self.doc.class(id) == Class::StructureToken && matches!(self.text(id), b"(" | b"[" | b"{")
    }

    fn closes(&self, id: Id) -> bool {
        self.doc.class(id) == Class::StructureToken && matches!(self.text(id), b")" | b"]" | b"}")
    }

    fn lex_document(&mut self) -> Result<(), Error> {
        while let Some(token) = self.lex_get_token()? {
            if !self.sig(token) {
                self.add_element(DOCUMENT, token)?;
                continue;
            }
            if self.text(token) == b";" {
                let null = self.new_statement(Class::StatementNull, Some(token));
                self.add_element(DOCUMENT, null)?;
                continue;
            }
            if self.doc.class(token) != Class::StructureToken {
                let class = self.statement(DOCUMENT, token)?;
                let statement = self.new_statement(class, Some(token));
                self.add_delayed(DOCUMENT);
                self.add_element(DOCUMENT, statement)?;
                self.lex_statement(statement)?;
                continue;
            }
            if self.opens(token) {
                self.rollback(&[token]);
                let statement = self.new_statement(Class::Statement, None);
                self.add_element(DOCUMENT, statement)?;
                self.lex_statement(statement)?;
                continue;
            }
            if self.closes(token) {
                let statement = self.new_statement(Class::StatementUnmatchedBrace, Some(token));
                self.add_element(DOCUMENT, statement)?;
                continue;
            }
            return Err("Lexer reached an illegal state".to_string());
        }
        self.add_delayed(DOCUMENT);
        if !self.perl6.is_empty() {
            let includes = self.doc.find(DOCUMENT, |d, e| {
                Some(d.class(e) == Class::StatementIncludePerl6)
            });
            if includes.len() > self.perl6.len() {
                return Err("Failed to find a perl6 section".to_string());
            }
        }
        Ok(())
    }

    fn new_statement(&mut self, class: Class, token: Option<Id>) -> Id {
        let statement = self.doc.alloc(class, b"");
        if let Some(token) = token {
            self.doc.push_child(statement, token);
        }
        statement
    }

    fn new_structure(&mut self, class: Class, token: Id) -> Id {
        let structure = self.doc.alloc(class, b"");
        self.doc.elems[structure].start = Some(token);
        self.doc.elems[token].parent = Some(structure);
        structure
    }

    /// `_statement`: the class of the statement `token` starts.
    fn statement(&mut self, parent: Id, token: Id) -> Result<Class, Error> {
        let pclass = self.doc.class(parent);
        if (pclass.isa(Class::StructureList) || pclass.isa(Class::StructureConstructor))
            && self.doc.isa(token, Class::Word)
        {
            let mut next = None;
            while let Some(n) = self.lex_get_token()? {
                next = Some(n);
                if !self.sig(n) {
                    self.delayed.push(n);
                    next = None;
                    continue;
                }
                if self.is(n, Class::Operator, b"=>") {
                    self.rollback(&[n]);
                    return Ok(Class::StatementExpression);
                }
                break;
            }
            self.rollback_opt(next);
        }

        let mut is_lexsub = false;
        let content = self.text(token).to_vec();
        let mut class = statement_class(&content);
        // PPI builds its class table, `presumed_features` included, for
        // every statement.
        let anchor = self.doc.schild(parent, -1).unwrap_or(parent);
        if self.doc.presumed_features(anchor)?.try_ && content == b"try" {
            class = Some(Class::StatementCompound);
        }
        if let Some(c) = class {
            let mut next = None;
            while let Some(n) = self.lex_get_token()? {
                next = Some(n);
                if !self.sig(n) {
                    self.delayed.push(n);
                    next = None;
                    continue;
                }
                if c == Class::StatementScheduled
                    && !(self.doc.isa(n, Class::StructureToken)
                        && matches!(self.text(n), b"{" | b";"))
                {
                    class = None;
                    break;
                }
                if matches!(content.as_slice(), b"my" | b"our" | b"state")
                    && self.is(n, Class::Word, b"sub")
                {
                    class = None;
                    is_lexsub = true;
                    break;
                }
                if !self.doc.isa(n, Class::Operator) || self.text(n) != b"=>" {
                    break;
                }
                self.rollback(&[n]);
                return Ok(Class::Statement);
            }
            self.rollback_opt(next);
        }

        if pclass.isa(Class::StructureSubscript) {
            if !class.is_some_and(|c| c.isa(Class::StatementExpression)) {
                return Ok(Class::StatementExpression);
            }
            while let Some(n) = self.lex_get_token()? {
                if !self.sig(n) {
                    self.delayed.push(n);
                    continue;
                }
                self.rollback(&[n]);
                if self.text(n) == b"}" {
                    return Ok(Class::StatementExpression);
                }
                return Ok(class.unwrap_or(Class::StatementExpression));
            }
            self.rollback(&[]);
            return Ok(Class::StatementExpression);
        }

        if let Some(c) = class {
            return Ok(c);
        }

        if is_lexsub || content == b"sub" {
            while let Some(n) = self.lex_get_token()? {
                if !self.sig(n) {
                    self.delayed.push(n);
                    continue;
                }
                self.rollback(&[n]);
                if statement_class(self.text(n)) == Some(Class::StatementScheduled) {
                    return Ok(Class::StatementScheduled);
                }
                if self.doc.isa(n, Class::Word) {
                    return Ok(Class::StatementSub);
                }
                return Ok(Class::Statement);
            }
            self.rollback(&[]);
            return Ok(Class::StatementSub);
        }

        if content == b"use" {
            while let Some(n) = self.lex_get_token()? {
                if !self.sig(n) {
                    self.delayed.push(n);
                    continue;
                }
                self.rollback(&[n]);
                if self.is(n, Class::Operator, b"=>") {
                    return Ok(Class::Statement);
                }
                if self.text(n) == b"v6" {
                    return Ok(Class::StatementIncludePerl6);
                }
                return Ok(Class::StatementInclude);
            }
            self.rollback(&[]);
            return Ok(Class::StatementInclude);
        }

        if pclass.isa(Class::StructureCondition)
            || pclass.isa(Class::StructureList)
            || pclass.isa(Class::StructureGiven)
            || pclass.isa(Class::StructureWhen)
        {
            return Ok(Class::StatementExpression);
        }
        if self.doc.class(token) == Class::Label {
            return Ok(Class::StatementCompound);
        }
        Ok(Class::Statement)
    }

    /// `__LEXER__normal`.
    fn normal(&self, statement: Id) -> bool {
        !matches!(
            self.doc.class(statement),
            Class::StatementCompound
                | Class::StatementGiven
                | Class::StatementWhen
                | Class::StatementPackage
                | Class::StatementScheduled
                | Class::StatementSub
        )
    }

    fn lex_statement(&mut self, statement: Id) -> Result<(), Error> {
        if self.doc.isa(statement, Class::StatementEnd) {
            return self.lex_end(statement);
        }
        while let Some(token) = self.lex_get_token()? {
            if !self.sig(token) {
                self.delayed.push(token);
                continue;
            }
            if self.closes(token) || self.doc.isa(token, Class::Separator) {
                self.rollback(&[token]);
                return Ok(());
            }
            if !self.normal(statement) && !self.continues(statement, token)? {
                self.rollback(&[token]);
                return Ok(());
            }
            if !self.doc.isa(token, Class::StructureToken) {
                self.add_element(statement, token)?;
                continue;
            }
            if self.text(token) == b";" {
                self.add_element(statement, token)?;
                return Ok(());
            }
            let class = match self.text(token) {
                b"(" => self.round(statement)?,
                b"[" => self.square(statement),
                _ => self.curly(statement)?,
            };
            let structure = self.new_structure(class, token);
            self.add_delayed(statement);
            self.add_element(statement, structure)?;
            self.lex_structure(structure)?;
        }
        self.rollback(&[]);
        Ok(())
    }

    fn lex_end(&mut self, statement: Id) -> Result<(), Error> {
        while let Some(token) = self.lex_get_token()? {
            self.doc.push_child(statement, token);
        }
        self.rollback(&[]);
        Ok(())
    }

    /// `_continues`: whether `token` continues a compound-like statement.
    fn continues(&self, statement: Id, token: Id) -> Result<bool, Error> {
        let doc = &self.doc;
        let part = doc.schildren(statement);
        if part.len() == 1 && doc.isa(part[0], Class::StructureBlock) {
            return Ok(false);
        }
        let Some(&last) = part.last() else {
            return Ok(true);
        };
        if !doc.isa(statement, Class::StatementCompound) {
            return Ok(!doc.isa(last, Class::StructureBlock));
        }
        let token_is =
            |class: Class, content: &[u8]| doc.isa(token, class) && doc.text(token) == content;
        let open_curly = token_is(Class::StructureToken, b"{");
        let mut kind = doc.compound_type(statement)?;
        if kind == Some("if") {
            if !doc.isa(last, Class::StructureBlock) {
                return Ok(true);
            }
            if let Some(next_last) = doc.schild(statement, -2)
                && doc.isa(next_last, Class::Word)
                && doc.text(next_last) == b"else"
            {
                return Ok(false);
            }
            return Ok(token_is(Class::Word, b"else") || token_is(Class::Word, b"elsif"));
        }
        if kind == Some("label") {
            if doc.isa(token, Class::Word)
                && matches!(doc.text(token), b"while" | b"until" | b"for" | b"foreach")
            {
                return Ok(true);
            }
            return Ok(open_curly);
        }
        if doc.isa(last, Class::Structure) && doc.braces(last) == Some("()") {
            return Ok(open_curly);
        }
        if kind == Some("for") {
            if doc.isa(last, Class::Word) && matches!(doc.text(last), b"for" | b"foreach") {
                if token_is(Class::StructureToken, b"(") || doc.isa(token, Class::QuoteLikeWords) {
                    return Ok(true);
                }
                kind = Some("foreach");
            } else if doc.isa(last, Class::StructureBlock) {
                return Ok(false);
            } else if doc.isa(last, Class::QuoteLikeWords) {
                return Ok(open_curly);
            }
        }
        if doc.isa(last, Class::Word) && doc.text(last) == b"continue" {
            return Ok(open_curly);
        }
        if kind == Some("try") && doc.presumed_features(last)?.try_ {
            if !doc.isa(last, Class::StructureBlock) {
                return Ok(true);
            }
            if let Some(next_last) = doc.schild(statement, -2)
                && doc.isa(next_last, Class::Word)
                && doc.text(next_last) == b"catch"
            {
                return Ok(false);
            }
            return Ok(token_is(Class::Word, b"catch"));
        }
        if doc.isa(last, Class::StructureBlock) {
            if part.len() >= 2
                && doc.isa(part[part.len() - 2], Class::Word)
                && doc.text(part[part.len() - 2]) == b"continue"
            {
                return Ok(false);
            }
            return Ok(token_is(Class::Word, b"continue"));
        }
        if kind == Some("while")
            && doc.isa(last, Class::Word)
            && matches!(doc.text(last), b"while" | b"until")
        {
            return Ok(token_is(Class::StructureToken, b"("));
        }
        if kind == Some("foreach") {
            if doc.isa(last, Class::Symbol) {
                return Ok(
                    token_is(Class::StructureToken, b"(") || doc.isa(token, Class::QuoteLikeWords)
                );
            }
            let last_content = doc.content(last);
            if last_content == b"foreach" || last_content == b"for" {
                if doc.isa(token, Class::Word)
                    && statement_class(doc.text(token)) == Some(Class::StatementVariable)
                {
                    return Ok(true);
                }
                return Ok(doc.content(token).first() == Some(&b'$')
                    || token_is(Class::StructureToken, b"(")
                    || doc.isa(token, Class::QuoteLikeWords));
            }
            if statement_class(&last_content) == Some(Class::StatementVariable) {
                return Ok(doc.content(token).first() == Some(&b'$'));
            }
            if doc.isa(last, Class::QuoteLikeWords) {
                return Ok(open_curly);
            }
        }
        Err(format!(
            "Illegal state in '{}' compound statement",
            kind.unwrap_or("")
        ))
    }

    /// `_round`.
    fn round(&self, parent: Id) -> Result<Class, Error> {
        if let Some(e) = self.doc.schild(parent, -1)
            && self.doc.isa(e, Class::Word)
        {
            match self.text(e) {
                b"if" | b"elsif" | b"unless" | b"while" | b"until" => {
                    return Ok(Class::StructureCondition);
                }
                b"for" | b"foreach" => return Ok(Class::StructureFor),
                _ => {}
            }
        }
        let pclass = self.doc.class(parent);
        if pclass.isa(Class::StatementCompound) {
            if matches!(self.doc.compound_type(parent)?, Some("for" | "foreach")) {
                return Ok(Class::StructureFor);
            }
        } else if pclass.isa(Class::StatementGiven) {
            return Ok(Class::StructureGiven);
        } else if pclass.isa(Class::StatementWhen) {
            return Ok(Class::StructureWhen);
        } else if pclass.isa(Class::StatementSub) {
            return Ok(Class::StructureSignature);
        }
        Ok(Class::StructureList)
    }

    /// `_square`.
    fn square(&self, parent: Id) -> Class {
        let doc = &self.doc;
        if let Some(e) = doc.schild(parent, -1) {
            if self.is(e, Class::Operator, b"->") || doc.isa(e, Class::StructureSubscript) {
                return Class::StructureSubscript;
            }
            if doc.isa(e, Class::Symbol) && matches!(doc.text(e).first(), Some(b'$' | b'@')) {
                return Class::StructureSubscript;
            }
            if doc.isa(e, Class::Cast)
                && matches!(doc.text(e).first(), Some(b'@' | b'%'))
                && doc
                    .schild(parent, -2)
                    .is_some_and(|prior| self.is(prior, Class::Operator, b"->"))
            {
                return Class::StructureSubscript;
            }
        }
        Class::StructureConstructor
    }

    /// `_curly`.
    fn curly(&mut self, parent: Id) -> Result<Class, Error> {
        let element = self.doc.schild(parent, -1);
        let content = element.map(|e| self.doc.content(e)).unwrap_or_default();
        let pclass = self.doc.class(parent);
        if let Some(e) = element {
            let doc = &self.doc;
            if content == b"->" && doc.isa(e, Class::Operator) {
                return Ok(Class::StructureSubscript);
            }
            if doc.isa(e, Class::StructureSubscript) {
                return Ok(Class::StructureSubscript);
            }
            if matches!(content.first(), Some(b'$' | b'@')) && doc.isa(e, Class::Symbol) {
                return Ok(Class::StructureSubscript);
            }
            if doc.isa(e, Class::Cast)
                && matches!(doc.text(e).first(), Some(b'@' | b'%' | b'*'))
                && doc
                    .schild(parent, -2)
                    .is_some_and(|prior| self.is(prior, Class::Operator, b"->"))
            {
                return Ok(Class::StructureSubscript);
            }
            if doc.isa(e, Class::StructureBlock)
                && doc.schild(parent, -2).is_some_and(|prior| {
                    let prior_content = doc.content(prior);
                    doc.isa(prior, Class::Cast) && (prior_content == b"@" || prior_content == b"$")
                })
            {
                return Ok(Class::StructureSubscript);
            }
            if pclass.isa(Class::StatementSub) || pclass.isa(Class::StatementPackage) {
                return Ok(Class::StructureBlock);
            }
            if let Some(class) = curly_class(&content) {
                return Ok(class);
            }
        }
        if pclass.isa(Class::StatementCompound) {
            return Ok(Class::StructureBlock);
        }
        if pclass.isa(Class::StatementInclude) {
            let n = self.doc.schildren(parent).len();
            if n == 2
                || (n == 3
                    && self
                        .doc
                        .schild(parent, 2)
                        .is_some_and(|c| self.doc.isa(c, Class::Number)))
            {
                return Ok(Class::StructureConstructor);
            }
        }
        if element.is_some() {
            return Ok(Class::StructureBlock);
        }
        if let Some(grand) = self.doc.parent(parent)
            && self.doc.isa(grand, Class::StructureList)
        {
            let function = self.doc.parent(grand).and_then(|g| self.doc.schild(g, -2));
            let fcontent = function.map(|f| self.doc.content(f));
            if fcontent
                .as_deref()
                .is_some_and(|f| matches!(f, b"map" | b"grep" | b"sort" | b"eval" | b"do"))
            {
                return Ok(Class::StructureBlock);
            }
            if !fcontent
                .as_deref()
                .is_some_and(|f| matches!(f, b"print" | b"say"))
            {
                return Ok(Class::StructureConstructor);
            }
        }
        let mut position = 0;
        let mut delayed = Vec::new();
        while let Some(next) = self.lex_get_token()? {
            if !self.sig(next) {
                delayed.push(next);
                continue;
            }
            position += 1;
            if position >= 3 {
                delayed.push(next);
                self.buffer_front(delayed);
                return Ok(self.curly_default(parent));
            }
            let class = match (position, self.text(next)) {
                (1, b";") => Some(Class::StructureBlock),
                (1, b"}") | (2, b"=>") => Some(Class::StructureConstructor),
                _ => None,
            };
            if let Some(class) = class {
                delayed.push(next);
                self.buffer_front(delayed);
                return Ok(class);
            }
            delayed.push(next);
        }
        self.buffer_front(delayed);
        Ok(self.curly_default(parent))
    }

    fn curly_default(&mut self, parent: Id) -> Class {
        if self.doc.class(parent) == Class::Statement {
            self.doc.elems[parent].class = Class::StatementCompound;
        }
        Class::StructureBlock
    }

    fn lex_structure(&mut self, structure: Id) -> Result<(), Error> {
        while let Some(token) = self.lex_get_token()? {
            if !self.sig(token) {
                self.delayed.push(token);
                continue;
            }
            if !self.doc.isa(token, Class::StructureToken) {
                self.add_delayed(structure);
                let class = self.statement(structure, token)?;
                let statement = self.new_statement(class, Some(token));
                self.add_element(structure, statement)?;
                self.lex_statement(statement)?;
                continue;
            }
            if self.opens(token) {
                self.rollback(&[token]);
                let statement = self.new_statement(Class::Statement, None);
                self.add_element(structure, statement)?;
                self.lex_statement(statement)?;
                continue;
            }
            if self.closes(token) {
                let start = self.doc.elems[structure]
                    .start
                    .expect("a structure has an opening brace");
                if self.text(token) == opposite(self.text(start)) {
                    self.add_delayed(structure);
                    self.doc.elems[structure].finish = Some(token);
                    self.doc.elems[token].parent = Some(structure);
                    if self.doc.class(structure) == Class::StructureFor {
                        let statements = self.doc.elems[structure]
                            .children
                            .iter()
                            .filter(|&&c| self.doc.isa(c, Class::Statement))
                            .count();
                        if statements < 2 {
                            self.doc.elems[structure].class = Class::StructureList;
                        }
                    }
                    return Ok(());
                }
                self.rollback(&[token]);
                return Ok(());
            }
            let null = self.new_statement(Class::StatementNull, Some(token));
            self.add_element(structure, null)?;
        }
        self.add_delayed(structure);
        Ok(())
    }

    fn add_element(&mut self, parent: Id, element: Id) -> Result<(), Error> {
        if self.doc.class(parent) == Class::Statement
            && let Some(first) = self.doc.schild(parent, 0)
            && self.doc.class(first) == Class::Label
            && self.doc.schild(parent, 1).is_none()
        {
            return Err("Can't call method \"content\" on an undefined value".to_string());
        }
        self.add_delayed(parent);
        self.doc.push_child(parent, element);
        Ok(())
    }

    fn add_delayed(&mut self, parent: Id) {
        for el in std::mem::take(&mut self.delayed) {
            self.doc.push_child(parent, el);
        }
    }

    /// `_rollback(@tokens)`: the tokens, then the delayed ones before them,
    /// back to the front of the buffer.
    fn rollback(&mut self, tokens: &[Id]) {
        for &t in tokens.iter().rev() {
            self.buffer.push_front(t);
        }
        let delayed = std::mem::take(&mut self.delayed);
        for &t in delayed.iter().rev() {
            self.buffer.push_front(t);
        }
    }

    /// `_rollback($Next)` where `$Next` may be the end of input (which
    /// PPI puts in the buffer as 0, the same as asking the tokenizer).
    fn rollback_opt(&mut self, token: Option<Id>) {
        match token {
            Some(t) => self.rollback(&[t]),
            None => self.rollback(&[]),
        }
    }

    /// `_buffer(@tokens)`.
    fn buffer_front(&mut self, tokens: Vec<Id>) {
        for &t in tokens.iter().rev() {
            self.buffer.push_front(t);
        }
    }
}

/// `PPI::Document::index_locations` (tab width 1, as `Perl::Critic`'s
/// documents have).
fn index_locations(doc: &mut Doc) {
    let tokens = doc.tokens(DOCUMENT);
    let mut heredoc = 0;
    let mut location = Location {
        line: 1,
        character: 1,
        column: 1,
        logical_line: 1,
        logical_file: 0,
    };
    for token in tokens {
        doc.elems[token].location = Some(location);
        location = add_location(doc, location, token, &mut heredoc);
        if let Some(h) = &doc.elems[token].heredoc {
            heredoc += h.heredoc.len() + 1;
        }
    }
}

fn add_location(doc: &mut Doc, start: Location, token: Id, heredoc: &mut usize) -> Location {
    let content = doc.elems[token].content.clone();
    let newlines = count_byte(&content, b'\n');
    let (logical_line, logical_file) = logical_line_and_file(doc, start, token, newlines);
    if newlines == 0 {
        return Location {
            line: start.line,
            character: start.character + content.len(),
            column: start.column + content.len(),
            logical_line,
            logical_file,
        };
    }
    let mut location = Location {
        line: start.line + newlines,
        character: 1,
        column: 1,
        logical_line,
        logical_file,
    };
    if *heredoc > 0 {
        location.line += *heredoc;
        location.logical_line += *heredoc;
        *heredoc = 0;
    }
    if let Some(last_nl) = content.iter().rposition(|&c| c == b'\n') {
        let tail = content.len() - last_nl - 1;
        location.character += tail;
        location.column += tail;
    }
    location
}

fn logical_line_and_file(
    doc: &mut Doc,
    start: Location,
    token: Id,
    newlines: usize,
) -> (usize, usize) {
    if start.character == 1 {
        let class = doc.class(token);
        let content = doc.elems[token].content.clone();
        if class == Class::Comment {
            if let Some((line, file)) = line_directive(&content) {
                let file = file.map_or(start.logical_file, |f| file_index(doc, f));
                return (line, file);
            }
        } else if class == Class::Pod {
            // The last directive in the POD wins.
            let mut found = None;
            let mut offset = 0;
            for line in content.split_inclusive(|&c| c == b'\n') {
                if let Some((n, f)) = pod_line_directive(line) {
                    found = Some((n, f, offset + line.len()));
                }
                offset += line.len();
            }
            if let Some((n, f, end)) = found {
                let file = f.map_or(start.logical_file, |f| file_index(doc, f));
                let after = count_byte(&content[end..], b'\n');
                // `$line + $post_directive_newlines - 1`, the newline ending
                // the directive's own line counted by the regex.
                let post = after + usize::from(content[..end].ends_with(b"\n"));
                return ((n + post).saturating_sub(1), file);
            }
        }
    }
    (start.logical_line + newlines, start.logical_file)
}

fn file_index(doc: &mut Doc, name: Vec<u8>) -> usize {
    if let Some(i) = doc.files.iter().position(|f| *f == name) {
        return i;
    }
    doc.files.push(name);
    doc.files.len() - 1
}

/// A comment `# line N "file"`: `\A\#\s*line\s+(\d+)\s*(?:(\"?)([^\"]*[^\s\"])\2)?\s*\z`.
fn line_directive(content: &[u8]) -> Option<(usize, Option<Vec<u8>>)> {
    let rest = content.strip_prefix(b"#")?;
    let rest = &rest[rest.iter().take_while(|&&c| is_space(c)).count()..];
    let rest = rest.strip_prefix(b"line")?;
    let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
    if spaces == 0 {
        return None;
    }
    let rest = &rest[spaces..];
    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let line: usize = std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()?;
    // `\s* (?: (\"?) ([^\"]*[^\s\"]) \2 )? \s* \z`.
    let rest = &rest[digits..];
    let body = &rest[rest.iter().take_while(|&&c| is_space(c)).count()..];
    let body = &body[..body.len() - body.iter().rev().take_while(|&&c| is_space(c)).count()];
    if body.is_empty() {
        return Some((line, None));
    }
    let name = match body.strip_prefix(b"\"") {
        Some(quoted) => quoted.strip_suffix(b"\"")?,
        None => body,
    };
    if name.is_empty() || name.contains(&b'"') {
        return None;
    }
    // A quoted name may not end in whitespace (`[^\s\"]` before the quote).
    if is_space(name[name.len() - 1]) {
        return None;
    }
    // `$3 || $logical_file`: a name of "0" is false.
    if name == b"0" {
        return Some((line, None));
    }
    Some((line, Some(name.to_vec())))
}

/// A `#line` directive line inside POD (`^\#\s*?line\s+?(\d+)...$`, with
/// `/m`): the line number and file.
fn pod_line_directive(line: &[u8]) -> Option<(usize, Option<Vec<u8>>)> {
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    line_directive(body)
}
