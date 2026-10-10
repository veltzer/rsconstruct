//! PPI 1.284, the Perl parser `Perl::Critic` works on: its element classes,
//! the tree, and the navigation methods the policies use.
//!
//! The tree is an arena: element 0 is the document, every other element a
//! token, statement or structure, linked to its parent and (for nodes) its
//! children by index. A structure's braces are its `start` and `finish`
//! tokens, outside its children, as in PPI. Source is bytes, read as PPI
//! reads a file: each byte a character, matched with ASCII-only `\w`,
//! `\s` and `\d`.
//!
//! `tokenizer` and `lexer` build the tree together, as PPI's do: the lexer
//! pulls tokens one at a time and the tokenizer consults the tree built so
//! far (`presumed_features`, a token's parent).

mod lexer;
mod tokenizer;
pub mod version;

pub use lexer::parse;

/// An element of the arena.
pub type Id = usize;

/// The document: always element 0.
pub const DOCUMENT: Id = 0;

/// A PPI class. The abstract ones (`Element`, `Node`, `Token`, `Statement`
/// as a base, `Structure`, `Quote`, `QuoteLike`, `Regexp`) exist for `isa`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Class {
    Element,
    Node,
    Token,
    Document,
    // Tokens.
    Whitespace,
    Comment,
    Pod,
    Number,
    NumberBinary,
    NumberOctal,
    NumberHex,
    NumberFloat,
    NumberExp,
    NumberVersion,
    Word,
    DashedWord,
    Symbol,
    ArrayIndex,
    Magic,
    Quote,
    QuoteSingle,
    QuoteDouble,
    QuoteLiteral,
    QuoteInterpolate,
    QuoteLike,
    QuoteLikeBacktick,
    QuoteLikeCommand,
    QuoteLikeRegexp,
    QuoteLikeWords,
    QuoteLikeReadline,
    Regexp,
    RegexpMatch,
    RegexpSubstitute,
    RegexpTransliterate,
    Operator,
    Cast,
    StructureToken,
    Label,
    HereDoc,
    Separator,
    Data,
    End,
    Prototype,
    Attribute,
    Unknown,
    Bom,
    // Statements.
    Statement,
    StatementBreak,
    StatementCompound,
    StatementData,
    StatementEnd,
    StatementExpression,
    StatementGiven,
    StatementInclude,
    StatementIncludePerl6,
    StatementNull,
    StatementPackage,
    StatementScheduled,
    StatementSub,
    StatementUnmatchedBrace,
    StatementVariable,
    StatementWhen,
    // Structures.
    Structure,
    StructureBlock,
    StructureCondition,
    StructureConstructor,
    StructureFor,
    StructureGiven,
    StructureList,
    StructureSignature,
    StructureSubscript,
    StructureWhen,
}

impl Class {
    /// The class this one inherits from (`@ISA`, the branch PPI's methods
    /// and `isa` follow).
    // The class tables name every class: the glob keeps them readable.
    #[allow(clippy::enum_glob_use)]
    pub const fn superclass(self) -> Option<Self> {
        use Class::*;
        Some(match self {
            Element => return None,
            Node | Token => Element,
            Document | Statement | Structure => Node,
            NumberBinary | NumberOctal | NumberHex | NumberFloat | NumberVersion => Number,
            NumberExp => NumberFloat,
            Magic => Symbol,
            QuoteSingle | QuoteDouble | QuoteLiteral | QuoteInterpolate => Quote,
            QuoteLikeBacktick | QuoteLikeCommand | QuoteLikeRegexp | QuoteLikeWords
            | QuoteLikeReadline => QuoteLike,
            RegexpMatch | RegexpSubstitute | RegexpTransliterate => Regexp,
            Separator => Word,
            Prototype => QuoteLiteral,
            StatementScheduled => StatementSub,
            StatementVariable => StatementExpression,
            StatementIncludePerl6 => StatementInclude,
            StatementBreak
            | StatementCompound
            | StatementData
            | StatementEnd
            | StatementExpression
            | StatementGiven
            | StatementInclude
            | StatementNull
            | StatementPackage
            | StatementSub
            | StatementUnmatchedBrace
            | StatementWhen => Statement,
            StructureSignature => StructureList,
            StructureBlock | StructureCondition | StructureConstructor | StructureFor
            | StructureGiven | StructureList | StructureSubscript | StructureWhen => Structure,
            _ => Token,
        })
    }

    pub fn isa(self, other: Self) -> bool {
        let mut class = Some(self);
        while let Some(c) = class {
            if c == other {
                return true;
            }
            class = c.superclass();
        }
        false
    }

    /// The Perl package name.
    #[allow(clippy::enum_glob_use)]
    pub const fn name(self) -> &'static str {
        use Class::*;
        match self {
            Element => "PPI::Element",
            Node => "PPI::Node",
            Token => "PPI::Token",
            Document => "PPI::Document",
            Whitespace => "PPI::Token::Whitespace",
            Comment => "PPI::Token::Comment",
            Pod => "PPI::Token::Pod",
            Number => "PPI::Token::Number",
            NumberBinary => "PPI::Token::Number::Binary",
            NumberOctal => "PPI::Token::Number::Octal",
            NumberHex => "PPI::Token::Number::Hex",
            NumberFloat => "PPI::Token::Number::Float",
            NumberExp => "PPI::Token::Number::Exp",
            NumberVersion => "PPI::Token::Number::Version",
            Word => "PPI::Token::Word",
            DashedWord => "PPI::Token::DashedWord",
            Symbol => "PPI::Token::Symbol",
            ArrayIndex => "PPI::Token::ArrayIndex",
            Magic => "PPI::Token::Magic",
            Quote => "PPI::Token::Quote",
            QuoteSingle => "PPI::Token::Quote::Single",
            QuoteDouble => "PPI::Token::Quote::Double",
            QuoteLiteral => "PPI::Token::Quote::Literal",
            QuoteInterpolate => "PPI::Token::Quote::Interpolate",
            QuoteLike => "PPI::Token::QuoteLike",
            QuoteLikeBacktick => "PPI::Token::QuoteLike::Backtick",
            QuoteLikeCommand => "PPI::Token::QuoteLike::Command",
            QuoteLikeRegexp => "PPI::Token::QuoteLike::Regexp",
            QuoteLikeWords => "PPI::Token::QuoteLike::Words",
            QuoteLikeReadline => "PPI::Token::QuoteLike::Readline",
            Regexp => "PPI::Token::Regexp",
            RegexpMatch => "PPI::Token::Regexp::Match",
            RegexpSubstitute => "PPI::Token::Regexp::Substitute",
            RegexpTransliterate => "PPI::Token::Regexp::Transliterate",
            Operator => "PPI::Token::Operator",
            Cast => "PPI::Token::Cast",
            StructureToken => "PPI::Token::Structure",
            Label => "PPI::Token::Label",
            HereDoc => "PPI::Token::HereDoc",
            Separator => "PPI::Token::Separator",
            Data => "PPI::Token::Data",
            End => "PPI::Token::End",
            Prototype => "PPI::Token::Prototype",
            Attribute => "PPI::Token::Attribute",
            Unknown => "PPI::Token::Unknown",
            Bom => "PPI::Token::BOM",
            Statement => "PPI::Statement",
            StatementBreak => "PPI::Statement::Break",
            StatementCompound => "PPI::Statement::Compound",
            StatementData => "PPI::Statement::Data",
            StatementEnd => "PPI::Statement::End",
            StatementExpression => "PPI::Statement::Expression",
            StatementGiven => "PPI::Statement::Given",
            StatementInclude => "PPI::Statement::Include",
            StatementIncludePerl6 => "PPI::Statement::Include::Perl6",
            StatementNull => "PPI::Statement::Null",
            StatementPackage => "PPI::Statement::Package",
            StatementScheduled => "PPI::Statement::Scheduled",
            StatementSub => "PPI::Statement::Sub",
            StatementUnmatchedBrace => "PPI::Statement::UnmatchedBrace",
            StatementVariable => "PPI::Statement::Variable",
            StatementWhen => "PPI::Statement::When",
            Structure => "PPI::Structure",
            StructureBlock => "PPI::Structure::Block",
            StructureCondition => "PPI::Structure::Condition",
            StructureConstructor => "PPI::Structure::Constructor",
            StructureFor => "PPI::Structure::For",
            StructureGiven => "PPI::Structure::Given",
            StructureList => "PPI::Structure::List",
            StructureSignature => "PPI::Structure::Signature",
            StructureSubscript => "PPI::Structure::Subscript",
            StructureWhen => "PPI::Structure::When",
        }
    }

    pub fn is_token(self) -> bool {
        self.isa(Self::Token)
    }

    pub fn is_node(self) -> bool {
        self.isa(Self::Node)
    }

    pub fn is_structure(self) -> bool {
        self.isa(Self::Structure)
    }

    pub fn is_statement(self) -> bool {
        self.isa(Self::Statement)
    }

    /// The classes built on PPI's quote engines (`_QuoteEngine::Simple`
    /// or `::Full`), whose tokens the engine fills.
    pub const fn is_quote_like_class(self) -> bool {
        matches!(
            self,
            Self::QuoteSingle
                | Self::QuoteDouble
                | Self::QuoteLiteral
                | Self::QuoteInterpolate
                | Self::QuoteLikeBacktick
                | Self::QuoteLikeCommand
                | Self::QuoteLikeRegexp
                | Self::QuoteLikeWords
                | Self::QuoteLikeReadline
                | Self::RegexpMatch
                | Self::RegexpSubstitute
                | Self::RegexpTransliterate
                | Self::Prototype
        )
    }

    /// PPI's `significant`.
    pub const fn significant(self) -> bool {
        !matches!(
            self,
            Self::Whitespace
                | Self::Comment
                | Self::Pod
                | Self::End
                | Self::Bom
                | Self::StatementNull
        )
    }
}

/// One section of a quote-like token: where it is in the content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub position: usize,
    pub size: usize,
    /// `type`: the delimiter pair, absent on some sections as in PPI.
    pub kind: Option<Vec<u8>>,
    /// `_close`, while a braced section is being scanned.
    pub close: Option<u8>,
}

/// `_QuoteEngine::Full`'s fields.
#[derive(Clone, Debug, Default)]
pub struct QuoteData {
    pub operator: Option<Vec<u8>>,
    pub braced: Option<bool>,
    pub separator: Option<u8>,
    /// `_sections`: how many sections the operator takes (and, after an
    /// unterminated scan, how many it got).
    pub n_sections: usize,
    /// `modifiers`, for the operators that take them (`None` otherwise).
    pub modifiers: Option<Vec<u8>>,
    pub sections: Vec<Section>,
}

/// `PPI::Token::HereDoc`'s body lines (`_heredoc`).
#[derive(Clone, Debug, Default)]
pub struct HereDocData {
    pub heredoc: Vec<Vec<u8>>,
}

/// `[line, character, column, logical line, logical file]`; the file is an
/// index into `Doc::files`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub character: usize,
    pub column: usize,
    pub logical_line: usize,
    pub logical_file: usize,
}

#[derive(Clone, Debug)]
pub struct Elem {
    pub class: Class,
    pub parent: Option<Id>,
    /// The element's index among its parent's children (none for a
    /// structure's braces, which are not children).
    pub index: Option<usize>,
    /// A token's content.
    pub content: Vec<u8>,
    /// A node's children.
    pub children: Vec<Id>,
    /// A structure's braces.
    pub start: Option<Id>,
    pub finish: Option<Id>,
    pub location: Option<Location>,
    pub quote: Option<Box<QuoteData>>,
    pub heredoc: Option<Box<HereDocData>>,
    /// `_attribute`: an Operator `:` the tokenizer took for the start of a
    /// subroutine attribute.
    pub attribute_hint: bool,
}

impl Elem {
    const fn new(class: Class) -> Self {
        Self {
            class,
            parent: None,
            index: None,
            content: Vec::new(),
            children: Vec::new(),
            start: None,
            finish: None,
            location: None,
            quote: None,
            heredoc: None,
            attribute_hint: false,
        }
    }
}

/// A parsed document.
#[derive(Clone, Debug)]
pub struct Doc {
    pub elems: Vec<Elem>,
    /// Logical file names (`#line` directives); 0 is the document's name.
    pub files: Vec<Vec<u8>>,
    /// Each node's include statements among its children, in order: what
    /// `presumed_features` looks for among an element's earlier siblings.
    includes: std::collections::HashMap<Id, Vec<Id>>,
}

impl Doc {
    pub fn new(filename: &[u8]) -> Self {
        Self {
            elems: vec![Elem::new(Class::Document)],
            files: vec![filename.to_vec()],
            includes: std::collections::HashMap::new(),
        }
    }

    pub fn alloc(&mut self, class: Class, content: &[u8]) -> Id {
        let mut elem = Elem::new(class);
        elem.content = content.to_vec();
        self.elems.push(elem);
        self.elems.len() - 1
    }

    /// Append `child` to `parent`'s children (PPI's lexer only ever
    /// appends, so a child's index never changes).
    pub fn push_child(&mut self, parent: Id, child: Id) {
        let index = self.elems[parent].children.len();
        let elem = &mut self.elems[child];
        elem.parent = Some(parent);
        elem.index = Some(index);
        // A statement's class is settled when it is added (the lexer's
        // one later change, a plain statement to a compound one, never
        // makes or unmakes an include).
        if elem.class.isa(Class::StatementInclude) {
            self.includes.entry(parent).or_default().push(child);
        }
        self.elems[parent].children.push(child);
    }

    pub fn class(&self, id: Id) -> Class {
        self.elems[id].class
    }

    pub fn isa(&self, id: Id, class: Class) -> bool {
        self.elems[id].class.isa(class)
    }

    pub fn significant(&self, id: Id) -> bool {
        self.elems[id].class.significant()
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        self.elems[id].parent
    }

    pub fn children(&self, id: Id) -> &[Id] {
        &self.elems[id].children
    }

    /// A token's content (empty for a node: see `content`).
    pub fn text(&self, id: Id) -> &[u8] {
        &self.elems[id].content
    }

    /// `content`: a token's text, a node's children's concatenated, a
    /// structure's with its braces.
    pub fn content(&self, id: Id) -> Vec<u8> {
        let mut out = Vec::new();
        self.content_into(id, &mut out);
        out
    }

    fn content_into(&self, id: Id, out: &mut Vec<u8>) {
        let elem = &self.elems[id];
        if elem.class.is_token() {
            out.extend_from_slice(&elem.content);
            return;
        }
        if let Some(start) = elem.start {
            out.extend_from_slice(&self.elems[start].content);
        }
        for &child in &elem.children {
            self.content_into(child, out);
        }
        if let Some(finish) = elem.finish {
            out.extend_from_slice(&self.elems[finish].content);
        }
    }

    /// Whether the content equals `s` (PPI's overloaded `eq`).
    pub fn is(&self, id: Id, s: &str) -> bool {
        if self.elems[id].class.is_token() {
            self.elems[id].content == s.as_bytes()
        } else {
            self.content(id) == s.as_bytes()
        }
    }

    /// `tokens`: every token under a node, in order, braces included.
    pub fn tokens(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        self.tokens_into(id, &mut out);
        out
    }

    fn tokens_into(&self, id: Id, out: &mut Vec<Id>) {
        let elem = &self.elems[id];
        if elem.class.is_token() {
            out.push(id);
            return;
        }
        if let Some(start) = elem.start {
            out.push(start);
        }
        for &child in &elem.children {
            self.tokens_into(child, out);
        }
        if let Some(finish) = elem.finish {
            out.push(finish);
        }
    }

    /// `schild`: the index'th significant child (negative from the end).
    pub fn schild(&self, id: Id, index: isize) -> Option<Id> {
        let children = &self.elems[id].children;
        if index < 0 {
            children
                .iter()
                .rev()
                .filter(|&&c| self.significant(c))
                .nth((-index - 1) as usize)
                .copied()
        } else {
            children
                .iter()
                .filter(|&&c| self.significant(c))
                .nth(index as usize)
                .copied()
        }
    }

    pub fn schildren(&self, id: Id) -> Vec<Id> {
        self.elems[id]
            .children
            .iter()
            .copied()
            .filter(|&c| self.significant(c))
            .collect()
    }

    /// The position of `id` among its parent's children (`None` for a
    /// structure's brace, which is not a child).
    fn position(&self, id: Id) -> Option<(Id, usize)> {
        Some((self.parent(id)?, self.elems[id].index?))
    }

    /// `Token::Structure`'s sibling methods: a brace has no siblings, a `;`
    /// does.
    fn is_brace(&self, id: Id) -> bool {
        self.class(id) == Class::StructureToken && self.text(id) != b";"
    }

    pub fn next_sibling(&self, id: Id) -> Option<Id> {
        if self.is_brace(id) {
            return None;
        }
        let parent = self.parent(id)?;
        let siblings = &self.elems[parent].children;
        match self.position(id) {
            Some((_, index)) => siblings.get(index + 1).copied(),
            // PPI's `__position` is undef: `$elements->[undef + 1]`.
            None => siblings.get(1).copied(),
        }
    }

    pub fn previous_sibling(&self, id: Id) -> Option<Id> {
        if self.is_brace(id) {
            return None;
        }
        let parent = self.parent(id)?;
        match self.position(id) {
            Some((_, index)) if index > 0 => Some(self.elems[parent].children[index - 1]),
            _ => None,
        }
    }

    pub fn snext_sibling(&self, id: Id) -> Option<Id> {
        if self.is_brace(id) {
            return None;
        }
        let parent = self.parent(id)?;
        let siblings = &self.elems[parent].children;
        let start = self.position(id).map_or(0, |(_, i)| i);
        siblings[start + 1..]
            .iter()
            .copied()
            .find(|&c| self.significant(c))
    }

    pub fn sprevious_sibling(&self, id: Id) -> Option<Id> {
        if self.is_brace(id) {
            return None;
        }
        let parent = self.parent(id)?;
        let index = self.position(id)?.1;
        self.elems[parent].children[..index]
            .iter()
            .rev()
            .copied()
            .find(|&c| self.significant(c))
    }

    /// `first_element`: a structure's opening brace, else the first child.
    pub fn first_element(&self, id: Id) -> Option<Id> {
        let elem = &self.elems[id];
        elem.start
            .or_else(|| elem.children.first().copied())
            .or(elem.finish)
    }

    /// `last_element`: a structure's closing brace, else the last child.
    pub fn last_element(&self, id: Id) -> Option<Id> {
        let elem = &self.elems[id];
        elem.finish
            .or_else(|| elem.children.last().copied())
            .or(elem.start)
    }

    pub fn last_token(&self, id: Id) -> Option<Id> {
        let mut cursor = id;
        while self.class(cursor).is_node() {
            cursor = self.last_element(cursor)?;
        }
        Some(cursor)
    }

    /// `previous_token`, with `Token::Structure`'s version for braces.
    pub fn previous_token(&self, id: Id) -> Option<Id> {
        if self.is_brace(id) {
            let structure = self.parent(id)?;
            if matches!(self.text(id), b")" | b"]" | b"}") {
                if let Some(&child) = self.elems[structure].children.last() {
                    return if self.class(child).is_token() {
                        Some(child)
                    } else {
                        self.last_token(child)
                    };
                } else if let Some(start) = self.elems[structure].start {
                    return Some(start);
                }
            }
            return self.element_previous_token(structure);
        }
        self.element_previous_token(id)
    }

    fn element_previous_token(&self, id: Id) -> Option<Id> {
        let mut cursor = id;
        loop {
            if let Some(element) = self.previous_sibling(cursor) {
                return if self.class(element).is_token() {
                    Some(element)
                } else {
                    self.last_token(element)
                };
            }
            cursor = self.parent(cursor)?;
            if self.class(cursor).is_structure()
                && let Some(start) = self.elems[cursor].start
            {
                return Some(start);
            }
        }
    }

    /// `statement`: the element itself or its nearest statement ancestor.
    pub fn statement(&self, id: Id) -> Option<Id> {
        let mut cursor = id;
        loop {
            if self.class(cursor).is_statement() {
                return Some(cursor);
            }
            cursor = self.parent(cursor)?;
        }
    }

    /// `location`: a token's, or a node's first element's.
    pub fn location(&self, id: Id) -> Option<Location> {
        let mut cursor = id;
        loop {
            let elem = &self.elems[cursor];
            if elem.class.is_token() {
                return elem.location;
            }
            cursor = if elem.class.is_structure() {
                self.first_element(cursor)?
            } else {
                *elem.children.first()?
            };
        }
    }

    /// `find`: every element under `id` the predicate accepts, in PPI's
    /// order (a structure's opening brace, its children, its closing
    /// brace). The predicate returns `None` to skip an element's subtree.
    pub fn find(&self, id: Id, mut wanted: impl FnMut(&Self, Id) -> Option<bool>) -> Vec<Id> {
        let mut found = Vec::new();
        let mut queue: Vec<Id> = self.elems[id].children.iter().rev().copied().collect();
        while let Some(element) = queue.pop() {
            let rv = wanted(self, element);
            if rv == Some(true) {
                found.push(element);
            }
            if rv.is_none() || !self.class(element).is_node() {
                continue;
            }
            let elem = &self.elems[element];
            if let Some(finish) = elem.finish {
                queue.push(finish);
            }
            queue.extend(elem.children.iter().rev().copied());
            if let Some(start) = elem.start {
                queue.push(start);
            }
        }
        found
    }

    /// `find_first`.
    pub fn find_first(
        &self,
        id: Id,
        mut wanted: impl FnMut(&Self, Id) -> Option<bool>,
    ) -> Option<Id> {
        let mut queue: Vec<Id> = self.elems[id].children.iter().rev().copied().collect();
        while let Some(element) = queue.pop() {
            let rv = wanted(self, element);
            if rv == Some(true) {
                return Some(element);
            }
            if rv.is_none() || !self.class(element).is_node() {
                continue;
            }
            let elem = &self.elems[element];
            if let Some(finish) = elem.finish {
                queue.push(finish);
            }
            queue.extend(elem.children.iter().rev().copied());
            if let Some(start) = elem.start {
                queue.push(start);
            }
        }
        None
    }

    /// Structure `braces`: `()`, `[]` or `{}`.
    pub fn braces(&self, id: Id) -> Option<&'static str> {
        let start = self.elems[id].start?;
        match self.text(start) {
            b"[" => Some("[]"),
            b"(" => Some("()"),
            b"{" => Some("{}"),
            _ => None,
        }
    }

    /// A quote-like token's section `i`.
    pub fn section_content(&self, id: Id, i: usize) -> Option<Vec<u8>> {
        let quote = self.elems[id].quote.as_ref()?;
        let section = quote.sections.get(i)?;
        Some(perl_substr(
            &self.elems[id].content,
            section.position as isize,
            Some(section.size as isize),
        ))
    }

    /// The modifiers of a quote-like token (`get_modifiers`).
    pub fn modifiers(&self, id: Id) -> Vec<u8> {
        self.elems[id]
            .quote
            .as_ref()
            .and_then(|q| q.modifiers.clone())
            .unwrap_or_default()
    }

    /// `string`, for the Quote classes.
    pub fn quote_string(&self, id: Id) -> Vec<u8> {
        match self.class(id) {
            Class::QuoteSingle | Class::QuoteDouble => {
                let s = self.text(id);
                perl_substr(s, 1, Some(s.len() as isize - 2))
            }
            _ => self.section_content(id, 0).unwrap_or_default(),
        }
    }

    /// `PPI::Statement::Include::type`.
    pub fn include_type(&self, id: Id) -> Option<Vec<u8>> {
        let keyword = self.schild(id, 0)?;
        self.isa(keyword, Class::Word)
            .then(|| self.text(keyword).to_vec())
    }

    /// `PPI::Statement::Include::module` (empty when not a word).
    pub fn include_module(&self, id: Id) -> Vec<u8> {
        match self.schild(id, 1) {
            Some(module) if self.isa(module, Class::Word) => self.text(module).to_vec(),
            _ => Vec::new(),
        }
    }

    /// `PPI::Statement::Include::pragma`.
    pub fn include_pragma(&self, id: Id) -> Vec<u8> {
        let module = self.include_module(id);
        let is_pragma = module.first().is_some_and(u8::is_ascii_lowercase)
            && module
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if is_pragma { module } else { Vec::new() }
    }

    /// `PPI::Statement::Include::version`: a Number second child's content.
    pub fn include_version(&self, id: Id) -> Vec<u8> {
        match self.schild(id, 1) {
            Some(version) if self.isa(version, Class::Number) => self.text(version).to_vec(),
            _ => Vec::new(),
        }
    }

    /// `PPI::Statement::Package::namespace`.
    pub fn package_namespace(&self, id: Id) -> Vec<u8> {
        match self.schild(id, 1) {
            Some(ns) if self.isa(ns, Class::Word) => self.text(ns).to_vec(),
            _ => Vec::new(),
        }
    }

    /// `PPI::Statement::Sub::type` and `Variable::type`: a leading
    /// `my`/`our`/`state` (and `local` for a variable), after any label.
    pub fn declaration_type(&self, id: Id, with_local: bool) -> Option<Vec<u8>> {
        let mut schildren = self.schildren(id).into_iter();
        let mut first = schildren.next();
        if first.is_some_and(|f| self.class(f) == Class::Label) {
            first = schildren.next();
        }
        let first = first?;
        let word = self.text(first);
        (self.isa(first, Class::Word)
            && (matches!(word, b"my" | b"our" | b"state") || (with_local && word == b"local")))
            .then(|| word.to_vec())
    }

    /// `PPI::Statement::Compound::type`; `Err` where `presumed_features`
    /// dies.
    pub fn compound_type(&self, id: Id) -> Result<Option<&'static str>, String> {
        let mut p = 0;
        let Some(mut element) = self.schild(id, p) else {
            return Ok(None);
        };
        if self.class(element) == Class::Label {
            p += 1;
            match self.schild(id, p) {
                Some(e) => element = e,
                None => return Ok(Some("label")),
            }
        }
        let content = self.content(element);
        if content == b"for" || content == b"foreach" {
            let for_word = if content == b"for" { "for" } else { "foreach" };
            p += 1;
            let Some(next) = self.schild(id, p) else {
                return Ok(Some(for_word));
            };
            if self.class(next).is_token()
                && (matches!(self.text(next), b"my" | b"our" | b"state")
                    || self.isa(next, Class::Symbol)
                    || self.isa(next, Class::QuoteLikeWords))
            {
                return Ok(Some("foreach"));
            }
            if self.isa(next, Class::StructureList) {
                return Ok(Some("foreach"));
            }
            return Ok(Some("for"));
        }
        if self.isa(element, Class::Word) {
            // PPI builds its type table, `presumed_features` included,
            // before looking the word up.
            let has_try = self.presumed_features(id)?.try_;
            return Ok(match content.as_slice() {
                b"if" | b"unless" => Some("if"),
                b"while" | b"until" => Some("while"),
                b"try" if has_try => Some("try"),
                _ => None,
            });
        }
        if self.isa(element, Class::StructureBlock) {
            return Ok(Some("continue"));
        }
        Ok(None)
    }

    /// `PPI::Statement::Sub::name`.
    pub fn sub_name(&self, id: Id) -> Vec<u8> {
        let index = if self.declaration_type(id, false).is_some() {
            2
        } else {
            1
        };
        [self.schild(id, index), self.schild(id, 0)]
            .into_iter()
            .flatten()
            .find(|&token| self.isa(token, Class::Word))
            .map(|token| self.text(token).to_vec())
            .unwrap_or_default()
    }

    /// `presumed_features`: the features the `use`/`no` statements before
    /// an element (its earlier siblings and its ancestors' earlier
    /// siblings) switch on or off. PPI evaluates them on the tree as built
    /// so far, and dies where an include's version or arguments make
    /// version.pm or PPI's own code die: that is the `Err`.
    pub fn presumed_features(&self, id: Id) -> Result<Features, String> {
        // PPI walks each ancestor's earlier significant siblings, nearest
        // first; the includes among them are all it keeps, so the walk
        // takes them from the parent's list of includes.
        let mut includes = Vec::new();
        let mut walker = Some(id);
        while let Some(w) = walker {
            match (self.parent(w), self.elems[w].index) {
                (Some(parent), Some(index)) => {
                    if let Some(list) = self.includes.get(&parent) {
                        let before = list
                            .partition_point(|&s| self.elems[s].index.is_some_and(|i| i <= index));
                        includes.extend(list[..before].iter().rev().copied());
                    }
                }
                _ => {
                    // A structure's brace, or the document: no siblings.
                    if self.class(w).isa(Class::StatementInclude) {
                        includes.push(w);
                    }
                }
            }
            walker = self.parent(w);
        }
        // Nearest first; the nearest one that mentions a feature wins.
        let mut features = Features::default();
        let (mut signatures, mut try_) = (None, None);
        for include in includes {
            let mods = self.feature_mods(include)?;
            signatures = signatures.or(mods.signatures);
            try_ = try_.or(mods.try_);
        }
        features.signatures = signatures.unwrap_or(false);
        features.try_ = try_.unwrap_or(false);
        Ok(features)
    }

    /// `PPI::Statement::Include::feature_mods`: the features the statement
    /// turns on (`Some(true)`) or off, and those it does not mention.
    fn feature_mods(&self, id: Id) -> Result<FeatureMods, String> {
        let mut mods = FeatureMods::default();
        let kind = self.include_type(id);
        if kind.as_deref() == Some(b"require") {
            return Ok(mods);
        }
        let version = self.include_version(id);
        if !version.is_empty() && version != b"0" {
            let parsed = version::Version::parse_checked(&version)
                .map_err(|e| format!("{e} at {INCLUDE_PM} line 272.\n"))?;
            if parsed >= version::Version::decimal(5, 35) {
                mods.signatures = Some(true);
                return Ok(mods);
            }
        }
        let on = kind.as_deref() == Some(b"use");
        match self.include_module(id).as_slice() {
            b"feature" => {
                for arg in self.decompose_arguments(id)? {
                    match arg.as_slice() {
                        b"signatures" => mods.signatures = Some(on),
                        b"try" => mods.try_ = Some(on),
                        _ => {}
                    }
                }
            }
            b"Mojolicious::Lite" => {
                let wants = self
                    .decompose_arguments(id)?
                    .iter()
                    .any(|a| contains(a, b"-signatures"));
                mods.signatures = Some(wants);
            }
            b"Modern::Perl" => {
                let v = self
                    .include_module_version(id)
                    .and_then(|v| version::numeric_literal(self.text(v)))
                    .unwrap_or(0.0);
                mods.signatures = Some(v >= 2023.0);
            }
            b"experimental" => {
                let wants = self
                    .decompose_arguments(id)?
                    .iter()
                    .any(|a| contains(a, b"signatures"));
                mods.signatures = Some(wants);
            }
            b"Syntax::Keyword::Try" => mods.try_ = Some(on),
            _ => {}
        }
        Ok(mods)
    }

    /// `PPI::Statement::Include::module_version`.
    pub fn include_module_version(&self, id: Id) -> Option<Id> {
        if self
            .schild(id, 3)
            .is_some_and(|argument| self.isa(argument, Class::Operator))
        {
            return None;
        }
        let version = self.schild(id, 2)?;
        self.isa(version, Class::Number).then_some(version)
    }

    /// `PPI::Statement::Include::arguments`.
    pub fn include_arguments(&self, id: Id) -> Vec<Id> {
        let mut args = self.schildren(id);
        if args.is_empty() {
            return args;
        }
        args.remove(0);
        if args.last().is_some_and(|&last| {
            self.class(last) == Class::StructureToken && self.text(last) == b";"
        }) {
            args.pop();
        }
        if args.is_empty() {
            return args;
        }
        args.remove(0);
        if args.is_empty() {
            return args;
        }
        if self.isa(args[0], Class::Number) {
            let Some(&after) = args.get(1) else {
                return Vec::new();
            };
            if !self.isa(after, Class::Operator) {
                args.remove(0);
            }
        }
        args
    }

    /// `_decompose_arguments`: the arguments' literal strings. PPI maps
    /// every argument again for as long as any is still an element, the
    /// strings of earlier rounds included, and a string there is a class
    /// name to Perl: calling methods on it dies.
    fn decompose_arguments(&self, id: Id) -> Result<Vec<Vec<u8>>, String> {
        let mut args: Vec<Arg> = self
            .include_arguments(id)
            .into_iter()
            .map(Arg::Elem)
            .collect();
        while args.iter().any(|a| matches!(a, Arg::Elem(_))) {
            let mut next = Vec::new();
            for arg in args {
                next.extend(self.decompose_argument(arg)?);
            }
            args = next;
        }
        Ok(args
            .into_iter()
            .filter_map(|a| match a {
                Arg::Str(s) => Some(s),
                _ => None,
            })
            .collect())
    }

    /// `_decompose_argument`.
    fn decompose_argument(&self, arg: Arg) -> Result<Vec<Arg>, String> {
        let id = match arg {
            Arg::Elem(id) => id,
            Arg::Str(s) if s.is_empty() => {
                return Err(format!(
                    "Can't call method \"isa\" without a package or object reference at {INCLUDE_PM} line 317.\n"
                ));
            }
            Arg::Str(s) => {
                let name = String::from_utf8_lossy(&s);
                return Err(format!(
                    "Can't locate object method \"content\" via package \"{name}\" (perhaps you forgot to load \"{name}\"?) at {INCLUDE_PM} line 322.\n"
                ));
            }
            Arg::Undef => {
                return Err(format!(
                    "Can't call method \"isa\" on an undefined value at {INCLUDE_PM} line 317.\n"
                ));
            }
        };
        let class = self.class(id);
        if class.isa(Class::StructureList) || class.isa(Class::StatementExpression) {
            return Ok(self.children(id).iter().map(|&c| Arg::Elem(c)).collect());
        }
        if let Some(values) = self.literal(id) {
            return Ok(values);
        }
        Ok(Vec::new())
    }

    /// The `literal` (or else `string`) of the token classes that have one,
    /// as list values; `None` for the rest.
    fn literal(&self, id: Id) -> Option<Vec<Arg>> {
        let class = self.class(id);
        let text = self.text(id);
        if class.isa(Class::QuoteLikeWords) {
            return Some(self.words_literal(id).into_iter().map(Arg::Str).collect());
        }
        if class.isa(Class::Word) || class == Class::DashedWord {
            let mut word = Vec::new();
            for &c in text {
                if c == b'\'' {
                    word.extend_from_slice(b"::");
                } else {
                    word.push(c);
                }
            }
            return Some(vec![Arg::Str(word)]);
        }
        if class.isa(Class::Number) {
            return Some(
                version::number_literal(class, text)
                    .map(Arg::Str)
                    .into_iter()
                    .collect(),
            );
        }
        if class == Class::QuoteSingle {
            return Some(vec![Arg::Str(single_quote_literal(&self.quote_string(id)))]);
        }
        if class.isa(Class::QuoteLiteral) {
            // `string` returns nothing without a section: `literal`'s
            // `$str` is undef.
            return Some(vec![match self.section_content(id, 0) {
                Some(s) => Arg::Str(single_quote_literal(&s)),
                None => Arg::Undef,
            }]);
        }
        if class.isa(Class::Quote) {
            return Some(vec![Arg::Str(self.quote_string(id))]);
        }
        None
    }

    /// `PPI::Token::QuoteLike::Words::literal`.
    pub fn words_literal(&self, id: Id) -> Vec<Vec<u8>> {
        let Some(content) = self.section_content(id, 0) else {
            return Vec::new();
        };
        // `s/\\([\Q$left$right\\\E])/$1/g`, `$left` being the first
        // section's delimiter pair.
        let delimiters = self.delimiters(id);
        let (left, right) = delimiters
            .first()
            .map_or((None, None), |d| (d.first().copied(), d.get(1).copied()));
        let mut unescaped = Vec::new();
        let mut i = 0;
        while i < content.len() {
            if content[i] == b'\\'
                && let Some(&next) = content.get(i + 1)
                && (Some(next) == left || Some(next) == right || next == b'\\')
            {
                unescaped.push(next);
                i += 2;
                continue;
            }
            unescaped.push(content[i]);
            i += 1;
        }
        split_whitespace(&unescaped)
    }

    /// `_delimiters`.
    fn delimiters(&self, id: Id) -> Vec<Vec<u8>> {
        let Some(quote) = self.elems[id].quote.as_ref() else {
            return Vec::new();
        };
        let content = &self.elems[id].content;
        quote
            .sections
            .iter()
            .map(|s| {
                if let Some(kind) = &s.kind {
                    return kind.clone();
                }
                let mut d = perl_substr(content, s.position as isize - 1, Some(1));
                d.extend(perl_substr(
                    content,
                    (s.position + s.size) as isize,
                    Some(1),
                ));
                d
            })
            .collect()
    }
}

/// Where PPI's `feature_mods` lives, as Perl's die messages name it.
const INCLUDE_PM: &str = "/usr/share/perl5/PPI/Statement/Include.pm";

/// The features `presumed_features` tracks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Features {
    pub signatures: bool,
    pub try_: bool,
}

/// What one include says about each feature.
#[derive(Clone, Copy, Debug, Default)]
struct FeatureMods {
    signatures: Option<bool>,
    try_: Option<bool>,
}

/// A value of `_decompose_arguments`' list.
enum Arg {
    Elem(Id),
    Str(Vec<u8>),
    Undef,
}

/// Perl's `substr($s, $offset, $length)` (no warnings: out of range is
/// empty).
pub fn perl_substr(s: &[u8], offset: isize, length: Option<isize>) -> Vec<u8> {
    let len = s.len() as isize;
    let start = if offset < 0 {
        (len + offset).max(0)
    } else {
        offset
    };
    if start > len {
        return Vec::new();
    }
    let end = match length {
        None => len,
        Some(l) if l < 0 => (len + l).max(start),
        Some(l) => (start + l).min(len),
    };
    if end <= start {
        return Vec::new();
    }
    s[start as usize..end as usize].to_vec()
}

/// `split ' ', $s`.
pub fn split_whitespace(s: &[u8]) -> Vec<Vec<u8>> {
    s.split(|c| is_space(*c))
        .filter(|w| !w.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// `PPI::Token::Quote::Single::literal`'s unescaping.
pub fn single_quote_literal(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' && i + 1 < s.len() && s[i + 1] != b'\n' {
            if s[i + 1] == b'\'' || s[i + 1] == b'\\' {
                out.push(s[i + 1]);
            } else {
                out.extend_from_slice(&s[i..i + 2]);
            }
            i += 2;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// How many times `byte` occurs in `s`. Clippy suggests the `bytecount`
/// crate; these are short tokens and versions, not worth a dependency.
#[allow(clippy::naive_bytecount)]
pub fn count_byte(s: &[u8], byte: u8) -> usize {
    s.iter().filter(|&&c| c == byte).count()
}

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// `\w` on a byte (ASCII rules, as Perl applies them to a byte string).
pub const fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `\s` on a byte.
pub const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}
