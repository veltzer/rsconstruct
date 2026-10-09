//! The document svglint lints: htmlparser2 3.10.1's `parseDOM` in XML mode,
//! without entity decoding, feeding `DomHandler` 2.4.2 with start indices.
//!
//! This is a lenient parser: an unclosed tag is closed at the end, a stray
//! end tag closes everything back to its match or is ignored, a duplicate
//! attribute keeps its first value, `&amp;` stays as written. The `valid`
//! rule is what reports malformed input; the `elm` and `attr` rules run
//! over whatever tree this builds, exactly as they do in svglint, so the
//! port follows the JavaScript state machine step for step, including how
//! node start positions are chained from one node to the next.
//!
//! Positions are UTF-16 code unit offsets, as JavaScript string indices
//! are, so the columns reported agree with svglint's for any input.

pub type NodeId = usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Tag {
        name: String,
        /// Attributes in document order; the first of a repeated name wins.
        attrs: Vec<(String, String)>,
    },
    Text(String),
    Comment(String),
    Cdata,
    Directive,
}

#[derive(Debug)]
pub struct Node {
    pub kind: Kind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// `startIndex` as htmlparser2 computes it (see `Parser::update_position`).
    pub start_index: i64,
}

#[derive(Debug, Default)]
pub struct Dom {
    pub nodes: Vec<Node>,
    /// The top-level nodes, in document order.
    pub roots: Vec<NodeId>,
    /// The source as UTF-16 code units, for turning indices into positions.
    pub source: Vec<u16>,
}

impl Dom {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn is_tag(&self, id: NodeId) -> bool {
        matches!(self.nodes[id].kind, Kind::Tag { .. })
    }

    pub fn tag_name(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Tag { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn attrs(&self, id: NodeId) -> &[(String, String)] {
        match &self.nodes[id].kind {
            Kind::Tag { attrs, .. } => attrs,
            _ => &[],
        }
    }

    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.attrs(id)
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The siblings list the node belongs to (its parent's children, or the
    /// top level), as css-select's `getSiblings` returns it.
    pub fn siblings(&self, id: NodeId) -> &[NodeId] {
        match self.nodes[id].parent {
            Some(p) => &self.nodes[p].children,
            None => &self.roots,
        }
    }

    /// Every element in document (pre-order) order.
    pub fn elements(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        for &r in &self.roots {
            self.collect_elements(r, &mut out);
        }
        out
    }

    fn collect_elements(&self, id: NodeId, out: &mut Vec<NodeId>) {
        if self.is_tag(id) {
            out.push(id);
        }
        for &c in &self.nodes[id].children {
            self.collect_elements(c, out);
        }
    }

    /// Every element below `id`, in document order.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        for &c in &self.nodes[id].children {
            self.collect_elements(c, &mut out);
        }
        out
    }

    /// domutils' `getText`: the concatenated text of the node, `<br>` being
    /// a newline, comments and directives nothing.
    pub fn text(&self, id: NodeId) -> String {
        let node = &self.nodes[id];
        match &node.kind {
            Kind::Tag { name, .. } if name == "br" => "\n".to_string(),
            Kind::Tag { .. } | Kind::Cdata => node.children.iter().map(|&c| self.text(c)).collect(),
            Kind::Text(data) => data.clone(),
            Kind::Comment(_) | Kind::Directive => String::new(),
        }
    }

    /// 1-based line and column (in UTF-16 code units) of a source index.
    pub fn position(&self, index: i64) -> (usize, usize) {
        let index = usize::try_from(index).unwrap_or(0).min(self.source.len());
        let before = &self.source[..index];
        let newlines = before.iter().filter(|&&u| u == u16::from(b'\n')).count();
        let line_start = before
            .iter()
            .rposition(|&u| u == u16::from(b'\n'))
            .map_or(0, |p| p + 1);
        (newlines + 1, index - line_start + 1)
    }

    fn add(&mut self, kind: Kind, parent: Option<NodeId>, start_index: i64) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent,
            children: Vec::new(),
            start_index,
        });
        match parent {
            Some(p) => self.nodes[p].children.push(id),
            None => self.roots.push(id),
        }
        id
    }
}

/// Parse a source string the way svglint does.
pub fn parse(source: &str) -> Dom {
    let units: Vec<u16> = source.encode_utf16().collect();
    let mut parser = Parser::new(units.clone());
    parser.run();
    let mut dom = parser.handler.dom;
    dom.source = units;
    dom
}

fn utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

const fn is_whitespace(c: u16) -> bool {
    c == b' ' as u16 || c == b'\n' as u16 || c == b'\t' as u16 || c == 0x0c || c == b'\r' as u16
}

// ---- DomHandler 2.4.2 -------------------------------------------------------

#[derive(Default)]
struct Handler {
    dom: Dom,
    tag_stack: Vec<NodeId>,
}

impl Handler {
    fn add(&mut self, kind: Kind, start_index: i64) -> NodeId {
        let parent = self.tag_stack.last().copied();
        self.dom.add(kind, parent, start_index)
    }

    fn onopentag(&mut self, name: String, attrs: Vec<(String, String)>, start_index: i64) {
        let id = self.add(
            Kind::Tag {
                name,
                attrs: js_object_order(attrs),
            },
            start_index,
        );
        self.tag_stack.push(id);
    }

    fn onclosetag(&mut self) {
        self.tag_stack.pop();
    }

    fn ontext(&mut self, data: &[u16], start_index: i64) {
        let text = utf16(data);
        // Consecutive text runs join the previous text node.
        let last = match self.tag_stack.last() {
            None => self.dom.roots.last().copied(),
            Some(&top) => self.dom.nodes[top].children.last().copied(),
        };
        if let Some(last) = last
            && let Kind::Text(existing) = &mut self.dom.nodes[last].kind
        {
            existing.push_str(&text);
            return;
        }
        self.add(Kind::Text(text), start_index);
    }

    fn oncomment(&mut self, data: &[u16], start_index: i64) {
        if let Some(&top) = self.tag_stack.last()
            && let Kind::Comment(existing) = &mut self.dom.nodes[top].kind
        {
            existing.push_str(&utf16(data));
            return;
        }
        let id = self.add(Kind::Comment(utf16(data)), start_index);
        self.tag_stack.push(id);
    }

    fn oncdatastart(&mut self, start_index: i64) {
        let id = self.add(Kind::Cdata, start_index);
        // The CDATA node starts with an empty text child the data joins.
        let text = self.dom.nodes.len();
        self.dom.nodes.push(Node {
            kind: Kind::Text(String::new()),
            parent: Some(id),
            children: Vec::new(),
            start_index,
        });
        self.dom.nodes[id].children.push(text);
        self.tag_stack.push(id);
    }

    fn onsectionend(&mut self) {
        self.tag_stack.pop();
    }

    fn onprocessinginstruction(&mut self, start_index: i64) {
        self.add(Kind::Directive, start_index);
    }
}

/// Attributes live in a JavaScript object (`attribs`), whose keys come out
/// with array-index-like names (`"0"`, `"2000"`) first in numeric order,
/// then the rest in insertion order. Such names only arise from malformed
/// markup (an unquoted value with spaces), but the `attr` rule's messages
/// list them, so the order is reproduced.
fn js_object_order(attrs: Vec<(String, String)>) -> Vec<(String, String)> {
    let is_index = |name: &str| -> Option<u64> {
        if name.is_empty() || name.len() > 10 || !name.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if name.len() > 1 && name.starts_with('0') {
            return None;
        }
        let n: u64 = name.parse().ok()?;
        (n < 4_294_967_295).then_some(n)
    };
    let mut indexed: Vec<(u64, (String, String))> = Vec::new();
    let mut named = Vec::new();
    for attr in attrs {
        match is_index(&attr.0) {
            Some(n) => indexed.push((n, attr)),
            None => named.push(attr),
        }
    }
    indexed.sort_by_key(|(n, _)| *n);
    indexed.into_iter().map(|(_, a)| a).chain(named).collect()
}

// ---- htmlparser2 Parser + Tokenizer (xmlMode, decodeEntities off) ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Text,
    BeforeTagName,
    InTagName,
    InSelfClosingTag,
    BeforeClosingTagName,
    InClosingTagName,
    AfterClosingTagName,
    BeforeAttributeName,
    InAttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    InAttributeValueDq,
    InAttributeValueSq,
    InAttributeValueNq,
    BeforeDeclaration,
    InDeclaration,
    InProcessingInstruction,
    BeforeComment,
    InComment,
    AfterComment1,
    AfterComment2,
    BeforeCdata1,
    BeforeCdata2,
    BeforeCdata3,
    BeforeCdata4,
    BeforeCdata5,
    BeforeCdata6,
    InCdata,
    AfterCdata1,
    AfterCdata2,
}

struct Parser {
    // Tokenizer
    state: State,
    buffer: Vec<u16>,
    section_start: i64,
    index: i64,
    buffer_offset: i64,
    // Parser
    tagname: String,
    attribname: String,
    attribvalue: String,
    attribs: Option<Vec<(String, String)>>,
    stack: Vec<String>,
    start_index: i64,
    end_index: Option<i64>,
    handler: Handler,
}

impl Parser {
    fn new(buffer: Vec<u16>) -> Self {
        Self {
            state: State::Text,
            buffer,
            section_start: 0,
            index: 0,
            buffer_offset: 0,
            tagname: String::new(),
            attribname: String::new(),
            attribvalue: String::new(),
            attribs: None,
            stack: Vec::new(),
            start_index: 0,
            end_index: None,
            handler: Handler::default(),
        }
    }

    // -- tokenizer helpers

    fn at(&self, i: i64) -> u16 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.buffer.get(i).copied())
            .unwrap_or(0)
    }

    fn section(&self) -> Vec<u16> {
        self.slice(self.section_start, self.index)
    }

    fn slice(&self, from: i64, to: i64) -> Vec<u16> {
        let len = i64::try_from(self.buffer.len()).unwrap_or(i64::MAX);
        let from = from.clamp(0, len);
        let to = to.clamp(from, len);
        let (from, to) = (
            usize::try_from(from).unwrap_or(0),
            usize::try_from(to).unwrap_or(0),
        );
        self.buffer[from..to].to_vec()
    }

    const fn absolute_index(&self) -> i64 {
        self.buffer_offset + self.index
    }

    // -- Parser position bookkeeping (`_updatePosition`)

    const fn update_position(&mut self, initial_offset: i64) {
        match self.end_index {
            None => {
                self.start_index = if self.section_start <= initial_offset {
                    0
                } else {
                    self.section_start - initial_offset
                };
            }
            Some(end) => self.start_index = end + 1,
        }
        self.end_index = Some(self.absolute_index());
    }

    // -- Parser callbacks

    fn ontext(&mut self, data: &[u16]) {
        self.update_position(1);
        self.end_index = self.end_index.map(|e| e - 1);
        let start = self.start_index;
        self.handler.ontext(data, start);
    }

    fn onopentagname(&mut self, name: &[u16]) {
        self.tagname = utf16(name);
        self.stack.push(self.tagname.clone());
        self.attribs = Some(Vec::new());
    }

    fn onopentagend(&mut self) {
        self.update_position(1);
        if let Some(attribs) = self.attribs.take() {
            let name = std::mem::take(&mut self.tagname);
            self.handler.onopentag(name, attribs, self.start_index);
        }
        self.tagname.clear();
    }

    fn onclosetag(&mut self, name: &[u16]) {
        self.update_position(1);
        let name = utf16(name);
        if !self.stack.is_empty()
            && let Some(pos) = self.stack.iter().rposition(|n| *n == name)
        {
            let count = self.stack.len() - pos;
            for _ in 0..count {
                self.stack.pop();
                self.handler.onclosetag();
            }
        }
    }

    fn onselfclosingtag(&mut self) {
        let name = self.tagname.clone();
        self.onopentagend();
        if self.stack.last() == Some(&name) {
            self.handler.onclosetag();
            self.stack.pop();
        }
    }

    fn onattribname(&mut self, name: &[u16]) {
        self.attribname = utf16(name);
    }

    fn onattribdata(&mut self, value: &[u16]) {
        self.attribvalue.push_str(&utf16(value));
    }

    fn onattribend(&mut self) {
        if let Some(attribs) = &mut self.attribs
            && !attribs.iter().any(|(n, _)| *n == self.attribname)
        {
            attribs.push((self.attribname.clone(), self.attribvalue.clone()));
        }
        self.attribname.clear();
        self.attribvalue.clear();
    }

    /// Declarations (`<!DOCTYPE ...>`) and processing instructions both
    /// become directive nodes; neither updates the position bookkeeping.
    fn oninstruction(&mut self) {
        self.handler.onprocessinginstruction(self.start_index);
    }

    fn oncomment(&mut self, data: &[u16]) {
        self.update_position(4);
        let start = self.start_index;
        self.handler.oncomment(data, start);
        self.handler.onsectionend();
    }

    fn oncdata(&mut self, data: &[u16]) {
        self.update_position(1);
        let start = self.start_index;
        self.handler.oncdatastart(start);
        self.handler.ontext(data, start);
        self.handler.onsectionend();
    }

    fn onend(&mut self) {
        while self.stack.pop().is_some() {
            self.handler.onclosetag();
        }
    }

    // -- tokenizer

    fn emit_token(&mut self, kind: Token) {
        let section = self.section();
        match kind {
            Token::OpenTagName => self.onopentagname(&section),
            Token::CloseTag => self.onclosetag(&section),
            Token::AttribData => self.onattribdata(&section),
        }
        self.section_start = -1;
    }

    fn run(&mut self) {
        self.parse();
        self.finish();
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one state machine, one function, as in the original"
    )]
    fn parse(&mut self) {
        while usize::try_from(self.index).is_ok_and(|i| i < self.buffer.len()) {
            let c = self.at(self.index);
            match self.state {
                State::Text => {
                    if c == u16::from(b'<') {
                        if self.index > self.section_start {
                            let s = self.section();
                            self.ontext(&s);
                        }
                        self.state = State::BeforeTagName;
                        self.section_start = self.index;
                    }
                }
                State::BeforeTagName => {
                    if c == u16::from(b'/') {
                        self.state = State::BeforeClosingTagName;
                    } else if c == u16::from(b'<') {
                        let s = self.section();
                        self.ontext(&s);
                        self.section_start = self.index;
                    } else if c == u16::from(b'>') || is_whitespace(c) {
                        self.state = State::Text;
                    } else if c == u16::from(b'!') {
                        self.state = State::BeforeDeclaration;
                        self.section_start = self.index + 1;
                    } else if c == u16::from(b'?') {
                        self.state = State::InProcessingInstruction;
                        self.section_start = self.index + 1;
                    } else {
                        self.state = State::InTagName;
                        self.section_start = self.index;
                    }
                }
                State::InTagName => {
                    if c == u16::from(b'/') || c == u16::from(b'>') || is_whitespace(c) {
                        self.emit_token(Token::OpenTagName);
                        self.state = State::BeforeAttributeName;
                        self.index -= 1;
                    }
                }
                State::BeforeClosingTagName => {
                    if is_whitespace(c) {
                        // skip
                    } else if c == u16::from(b'>') {
                        self.state = State::Text;
                    } else {
                        self.state = State::InClosingTagName;
                        self.section_start = self.index;
                    }
                }
                State::InClosingTagName => {
                    if c == u16::from(b'>') || is_whitespace(c) {
                        self.emit_token(Token::CloseTag);
                        self.state = State::AfterClosingTagName;
                        self.index -= 1;
                    }
                }
                State::AfterClosingTagName => {
                    if c == u16::from(b'>') {
                        self.state = State::Text;
                        self.section_start = self.index + 1;
                    }
                }
                State::BeforeAttributeName => {
                    if c == u16::from(b'>') {
                        self.onopentagend();
                        self.state = State::Text;
                        self.section_start = self.index + 1;
                    } else if c == u16::from(b'/') {
                        self.state = State::InSelfClosingTag;
                    } else if !is_whitespace(c) {
                        self.state = State::InAttributeName;
                        self.section_start = self.index;
                    }
                }
                State::InSelfClosingTag => {
                    if c == u16::from(b'>') {
                        self.onselfclosingtag();
                        self.state = State::Text;
                        self.section_start = self.index + 1;
                    } else if !is_whitespace(c) {
                        self.state = State::BeforeAttributeName;
                        self.index -= 1;
                    }
                }
                State::InAttributeName => {
                    if c == u16::from(b'=')
                        || c == u16::from(b'/')
                        || c == u16::from(b'>')
                        || is_whitespace(c)
                    {
                        let s = self.section();
                        self.onattribname(&s);
                        self.section_start = -1;
                        self.state = State::AfterAttributeName;
                        self.index -= 1;
                    }
                }
                State::AfterAttributeName => {
                    if c == u16::from(b'=') {
                        self.state = State::BeforeAttributeValue;
                    } else if c == u16::from(b'/') || c == u16::from(b'>') {
                        self.onattribend();
                        self.state = State::BeforeAttributeName;
                        self.index -= 1;
                    } else if !is_whitespace(c) {
                        self.onattribend();
                        self.state = State::InAttributeName;
                        self.section_start = self.index;
                    }
                }
                State::BeforeAttributeValue => {
                    if c == u16::from(b'"') {
                        self.state = State::InAttributeValueDq;
                        self.section_start = self.index + 1;
                    } else if c == u16::from(b'\'') {
                        self.state = State::InAttributeValueSq;
                        self.section_start = self.index + 1;
                    } else if !is_whitespace(c) {
                        self.state = State::InAttributeValueNq;
                        self.section_start = self.index;
                        self.index -= 1;
                    }
                }
                State::InAttributeValueDq => {
                    if c == u16::from(b'"') {
                        self.emit_token(Token::AttribData);
                        self.onattribend();
                        self.state = State::BeforeAttributeName;
                    }
                }
                State::InAttributeValueSq => {
                    if c == u16::from(b'\'') {
                        self.emit_token(Token::AttribData);
                        self.onattribend();
                        self.state = State::BeforeAttributeName;
                    }
                }
                State::InAttributeValueNq => {
                    if is_whitespace(c) || c == u16::from(b'>') {
                        self.emit_token(Token::AttribData);
                        self.onattribend();
                        self.state = State::BeforeAttributeName;
                        self.index -= 1;
                    }
                }
                State::BeforeDeclaration => {
                    self.state = if c == u16::from(b'[') {
                        State::BeforeCdata1
                    } else if c == u16::from(b'-') {
                        State::BeforeComment
                    } else {
                        State::InDeclaration
                    };
                }
                State::InDeclaration | State::InProcessingInstruction => {
                    if c == u16::from(b'>') {
                        self.oninstruction();
                        self.state = State::Text;
                        self.section_start = self.index + 1;
                    }
                }
                State::BeforeComment => {
                    if c == u16::from(b'-') {
                        self.state = State::InComment;
                        self.section_start = self.index + 1;
                    } else {
                        self.state = State::InDeclaration;
                    }
                }
                State::InComment => {
                    if c == u16::from(b'-') {
                        self.state = State::AfterComment1;
                    }
                }
                State::AfterComment1 => {
                    self.state = if c == u16::from(b'-') {
                        State::AfterComment2
                    } else {
                        State::InComment
                    };
                }
                State::AfterComment2 => {
                    if c == u16::from(b'>') {
                        let data = self.slice(self.section_start, self.index - 2);
                        self.oncomment(&data);
                        self.state = State::Text;
                        self.section_start = self.index + 1;
                    } else if c != u16::from(b'-') {
                        self.state = State::InComment;
                    }
                }
                State::BeforeCdata1 => self.expect_cdata_char(c, b'C', State::BeforeCdata2),
                State::BeforeCdata2 => self.expect_cdata_char(c, b'D', State::BeforeCdata3),
                State::BeforeCdata3 => self.expect_cdata_char(c, b'A', State::BeforeCdata4),
                State::BeforeCdata4 => self.expect_cdata_char(c, b'T', State::BeforeCdata5),
                State::BeforeCdata5 => self.expect_cdata_char(c, b'A', State::BeforeCdata6),
                State::BeforeCdata6 => {
                    if c == u16::from(b'[') {
                        self.state = State::InCdata;
                        self.section_start = self.index + 1;
                    } else {
                        self.state = State::InDeclaration;
                        self.index -= 1;
                    }
                }
                State::InCdata => {
                    if c == u16::from(b']') {
                        self.state = State::AfterCdata1;
                    }
                }
                State::AfterCdata1 => {
                    self.state = if c == u16::from(b']') {
                        State::AfterCdata2
                    } else {
                        State::InCdata
                    };
                }
                State::AfterCdata2 => {
                    if c == u16::from(b'>') {
                        let data = self.slice(self.section_start, self.index - 2);
                        self.oncdata(&data);
                        self.state = State::Text;
                        self.section_start = self.index + 1;
                    } else if c != u16::from(b']') {
                        self.state = State::InCdata;
                    }
                }
            }
            self.index += 1;
        }
        self.cleanup();
    }

    /// The `ifElseState` helpers for `<![CDATA[`: either case of the letter
    /// advances, anything else turns the section into a declaration.
    fn expect_cdata_char(&mut self, c: u16, upper: u8, next: State) {
        let lower = upper.to_ascii_lowercase();
        if c == u16::from(lower) || c == u16::from(upper) {
            self.state = next;
        } else {
            self.state = State::InDeclaration;
            self.index -= 1;
        }
    }

    fn cleanup(&mut self) {
        if self.section_start < 0 {
            self.buffer.clear();
            self.buffer_offset += self.index;
            self.index = 0;
        } else {
            if self.state == State::Text {
                if self.section_start != self.index {
                    let data = self.slice(self.section_start, i64::MAX);
                    self.ontext(&data);
                }
                self.buffer.clear();
                self.buffer_offset += self.index;
                self.index = 0;
            } else if self.section_start == self.index {
                self.buffer.clear();
                self.buffer_offset += self.index;
                self.index = 0;
            } else {
                let keep = self.slice(self.section_start, i64::MAX);
                self.buffer = keep;
                self.index -= self.section_start;
                self.buffer_offset += self.section_start;
            }
            self.section_start = 0;
        }
    }

    fn finish(&mut self) {
        if self.section_start < self.index {
            self.handle_trailing_data();
        }
        self.onend();
    }

    fn handle_trailing_data(&mut self) {
        let data = self.slice(self.section_start, i64::MAX);
        match self.state {
            State::InCdata | State::AfterCdata1 | State::AfterCdata2 => self.oncdata(&data),
            State::InComment | State::AfterComment1 | State::AfterComment2 => self.oncomment(&data),
            State::InTagName
            | State::BeforeAttributeName
            | State::BeforeAttributeValue
            | State::AfterAttributeName
            | State::InAttributeName
            | State::InAttributeValueSq
            | State::InAttributeValueDq
            | State::InAttributeValueNq
            | State::InClosingTagName => {}
            _ => self.ontext(&data),
        }
    }
}

#[derive(Clone, Copy)]
enum Token {
    OpenTagName,
    CloseTag,
    AttribData,
}
