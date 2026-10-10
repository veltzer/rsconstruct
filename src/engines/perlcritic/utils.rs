//! The parts of `Perl::Critic::Utils` the ported policies call.

use super::ppi::{Class, Doc, Id, perl_substr, split_whitespace};

/// An element of an argument list: a tree element, or one of the
/// `PPI::Token::Quote::Single` tokens `split_nodes_on_comma` makes of a
/// `qw()` list's words (which are not in the tree).
#[derive(Clone, Debug)]
pub enum Node {
    Elem(Id),
    QwWord { content: Vec<u8> },
}

impl Node {
    pub fn content(&self, doc: &Doc) -> Vec<u8> {
        match self {
            Self::Elem(id) => doc.content(*id),
            Self::QwWord { content, .. } => content.clone(),
        }
    }

    pub fn isa(&self, doc: &Doc, class: Class) -> bool {
        match self {
            Self::Elem(id) => doc.isa(*id, class),
            Self::QwWord { .. } => Class::QuoteSingle.isa(class),
        }
    }

    /// `string`, for a Quote.
    pub fn string(&self, doc: &Doc) -> Vec<u8> {
        match self {
            Self::Elem(id) => doc.quote_string(*id),
            Self::QwWord { content, .. } => {
                perl_substr(content, 1, Some(content.len() as isize - 2))
            }
        }
    }

    pub const fn elem(&self) -> Option<Id> {
        match self {
            Self::Elem(id) => Some(*id),
            Self::QwWord { .. } => None,
        }
    }
}

/// One argument from `parse_arg_list`: a list of nodes (an array
/// reference in Perl, `None` for a hole the comma splitting leaves), or a
/// statement that is not an expression, which Perl returns bare.
#[derive(Clone, Debug)]
pub enum Arg {
    Nodes(Option<Vec<Node>>),
    Elem(Id),
}

/// What a policy does with an argument that Perl would die on.
pub type Fatal = String;

impl Arg {
    /// `$arg->[0]`: Perl dies on a bare element ("Not an ARRAY
    /// reference"), and a hole autovivifies to undef.
    pub fn first(&self) -> Result<Option<&Node>, Fatal> {
        match self {
            Self::Nodes(Some(nodes)) => Ok(nodes.first()),
            Self::Nodes(None) => Ok(None),
            Self::Elem(_) => Err("Not an ARRAY reference".to_string()),
        }
    }

    /// `$arg->[1]`.
    pub fn second(&self) -> Result<Option<&Node>, Fatal> {
        match self {
            Self::Nodes(Some(nodes)) => Ok(nodes.get(1)),
            Self::Nodes(None) => Ok(None),
            Self::Elem(_) => Err("Not an ARRAY reference".to_string()),
        }
    }

    /// Whether Perl sees the argument as true (a hole is undef).
    pub const fn defined(&self) -> bool {
        !matches!(self, Self::Nodes(None))
    }
}

/// `%PRECEDENCE_OF`.
pub const fn precedence_of(op: &[u8]) -> Option<u32> {
    Some(match op {
        b"->" => 1,
        b"++" | b"--" => 2,
        b"**" => 3,
        b"!" | b"~" | b"\\" => 4,
        b"=~" | b"!~" => 5,
        b"*" | b"/" | b"%" | b"x" => 6,
        b"+" | b"-" | b"." => 7,
        b"<<" | b">>" => 8,
        b"-R" | b"-W" | b"-X" | b"-r" | b"-w" | b"-x" | b"-e" | b"-O" | b"-o" | b"-z" | b"-s"
        | b"-M" | b"-A" | b"-C" | b"-S" | b"-c" | b"-b" | b"-f" | b"-d" | b"-p" | b"-l" | b"-u"
        | b"-g" | b"-k" | b"-t" | b"-T" | b"-B" => 9,
        b"<" | b">" | b"<=" | b">=" | b"lt" | b"gt" | b"le" | b"ge" => 10,
        b"==" | b"!=" | b"<=>" | b"eq" | b"ne" | b"cmp" | b"~~" => 11,
        b"&" => 12,
        b"|" | b"^" => 13,
        b"&&" => 14,
        b"//" | b"||" => 15,
        b".." => 16,
        b"..." => 17,
        b"?" | b":" => 18,
        b"=" | b"+=" | b"-=" | b"*=" | b"/=" | b"%=" | b"||=" | b"&&=" | b"|=" | b"&=" | b"**="
        | b"x=" | b".=" | b"^=" | b"<<=" | b">>=" | b"//=" => 19,
        b"," | b"=>" => 20,
        b"not" => 22,
        b"and" => 23,
        b"or" | b"xor" => 24,
        _ => return None,
    })
}

/// `@B::Keywords::Barewords` and `BarewordsExtra`.
const fn is_perl_bareword(name: &[u8]) -> bool {
    matches!(
        name,
        b"__FILE__"
            | b"__LINE__"
            | b"__PACKAGE__"
            | b"__DATA__"
            | b"__END__"
            | b"NULL"
            | b"and"
            | b"cmp"
            | b"continue"
            | b"do"
            | b"else"
            | b"elsif"
            | b"eq"
            | b"for"
            | b"foreach"
            | b"ge"
            | b"gt"
            | b"if"
            | b"le"
            | b"lock"
            | b"lt"
            | b"m"
            | b"ne"
            | b"no"
            | b"or"
            | b"package"
            | b"q"
            | b"qq"
            | b"qr"
            | b"qw"
            | b"qx"
            | b"s"
            | b"sub"
            | b"tr"
            | b"unless"
            | b"until"
            | b"while"
            | b"x"
            | b"xor"
            | b"y"
            | b"break"
            | b"given"
            | b"when"
            | b"default"
            | b"state"
            | b"try"
            | b"catch"
            | b"defer"
            | b"finally"
            | b"ADJUST"
            | b"class"
            | b"field"
            | b"method"
            | b"__CLASS__"
            | b"import"
            | b"unimport"
    )
}

/// `@B::Keywords::Filehandles`.
const fn is_perl_filehandle(name: &[u8]) -> bool {
    matches!(
        name,
        b"*ARGV" | b"ARGV" | b"*_" | b"_" | b"ARGVOUT" | b"DATA" | b"STDIN" | b"STDOUT" | b"STDERR"
    )
}

/// `_name_for_sub_or_stringified_element`.
fn name_for(doc: &Doc, elem: Id) -> Vec<u8> {
    if doc.isa(elem, Class::StatementSub) {
        doc.sub_name(elem)
    } else {
        doc.content(elem)
    }
}

fn is_op(doc: &Doc, elem: Option<Id>, op: &[u8]) -> bool {
    elem.is_some_and(|e| doc.isa(e, Class::Operator) && doc.text(e) == op)
}

/// `is_hash_key`.
pub fn is_hash_key(doc: &Doc, elem: Id) -> bool {
    let sib = doc.snext_sibling(elem);
    if sib.is_some_and(|s| doc.isa(s, Class::StructureList)) {
        return false;
    }
    let Some(parent) = doc.parent(elem) else {
        return false;
    };
    let Some(grandparent) = doc.parent(parent) else {
        return false;
    };
    if doc.isa(grandparent, Class::StructureSubscript) {
        if sib.is_some() && !(is_op(doc, sib, b",") || is_op(doc, sib, b"=>")) {
            return false;
        }
        return true;
    }
    is_op(doc, sib, b"=>")
}

/// `is_included_module_name`.
fn is_included_module_name(doc: &Doc, elem: Id) -> bool {
    let Some(statement) = doc.statement(elem) else {
        return false;
    };
    doc.isa(statement, Class::StatementInclude) && doc.schild(statement, 1) == Some(elem)
}

/// `is_label_pointer`.
fn is_label_pointer(doc: &Doc, elem: Id) -> bool {
    let Some(statement) = doc.statement(elem) else {
        return false;
    };
    if !doc.isa(statement, Class::StatementBreak) {
        return false;
    }
    doc.sprevious_sibling(elem).is_some_and(|p| {
        matches!(
            doc.content(p).as_slice(),
            b"redo" | b"goto" | b"next" | b"last"
        )
    })
}

/// `is_method_call`.
pub fn is_method_call(doc: &Doc, elem: Id) -> bool {
    is_op(doc, doc.sprevious_sibling(elem), b"->")
}

/// `is_class_name`.
fn is_class_name(doc: &Doc, elem: Id) -> bool {
    is_op(doc, doc.snext_sibling(elem), b"->") && !is_op(doc, doc.sprevious_sibling(elem), b"->")
}

/// `is_package_declaration`.
fn is_package_declaration(doc: &Doc, elem: Id) -> bool {
    let Some(statement) = doc.statement(elem) else {
        return false;
    };
    doc.isa(statement, Class::StatementPackage) && doc.schild(statement, 1) == Some(elem)
}

/// `is_subroutine_name`.
fn is_subroutine_name(doc: &Doc, elem: Id) -> bool {
    let Some(sib) = doc.sprevious_sibling(elem) else {
        return false;
    };
    if !doc.is(sib, "sub") {
        return false;
    }
    doc.statement(elem)
        .is_some_and(|s| doc.isa(s, Class::StatementSub))
}

/// `is_function_call`.
pub fn is_function_call(doc: &Doc, elem: Id) -> bool {
    let name = name_for(doc, elem);
    !(is_perl_bareword(&name)
        || is_perl_filehandle(&name)
        || is_package_declaration(doc, elem)
        || is_included_module_name(doc, elem)
        || is_method_call(doc, elem)
        || is_class_name(doc, elem)
        || is_subroutine_name(doc, elem)
        || is_label_pointer(doc, elem)
        || is_hash_key(doc, elem))
}

/// `first_arg`.
pub fn first_arg(doc: &Doc, elem: Id) -> Option<Id> {
    let sib = doc.snext_sibling(elem)?;
    if doc.isa(sib, Class::StructureList) {
        let expr = doc.schild(sib, 0)?;
        return if doc.isa(expr, Class::Statement) {
            doc.schild(expr, 0)
        } else {
            Some(expr)
        };
    }
    Some(sib)
}

/// `is_ppi_expression_or_generic_statement`.
fn is_expression_or_generic_statement(doc: &Doc, elem: Id) -> bool {
    let class = doc.class(elem);
    class.isa(Class::StatementExpression) || class == Class::Statement
}

/// `parse_arg_list`.
pub fn parse_arg_list(doc: &Doc, elem: Id) -> Vec<Arg> {
    let Some(sib) = doc.snext_sibling(elem) else {
        return Vec::new();
    };
    if doc.isa(sib, Class::StructureList) {
        let mut out = Vec::new();
        for item in doc.schildren(sib) {
            if is_expression_or_generic_statement(doc, item) {
                let nodes: Vec<Node> = doc.schildren(item).into_iter().map(Node::Elem).collect();
                out.extend(split_nodes_on_comma(doc, &nodes));
            } else {
                out.push(Arg::Elem(item));
            }
        }
        return out;
    }
    let mut args = Vec::new();
    let mut iter = elem;
    while let Some(next) = doc.snext_sibling(iter) {
        iter = next;
        if doc.isa(next, Class::StructureToken) && doc.text(next) == b";" {
            break;
        }
        if doc.isa(next, Class::Operator) && precedence_of(doc.text(next)).is_some_and(|p| p >= 22)
        {
            break;
        }
        args.push(Node::Elem(next));
    }
    split_nodes_on_comma(doc, &args)
}

/// `split_nodes_on_comma`.
pub fn split_nodes_on_comma(doc: &Doc, nodes: &[Node]) -> Vec<Arg> {
    let mut i = 0;
    let mut stacks: Vec<Option<Vec<Node>>> = Vec::new();
    let push = |stacks: &mut Vec<Option<Vec<Node>>>, i: usize, node: Node| {
        while stacks.len() <= i {
            stacks.push(None);
        }
        stacks[i].get_or_insert_with(Vec::new).push(node);
    };
    for node in nodes {
        let content = node.content(doc);
        if node.isa(doc, Class::Operator) && (content == b"," || content == b"=>") {
            if !stacks.is_empty() {
                i += 1;
            }
            continue;
        }
        if let Node::Elem(id) = node
            && doc.isa(*id, Class::QuoteLikeWords)
        {
            let words = doc.elems[*id]
                .quote
                .as_ref()
                .and_then(|q| q.sections.first())
                .map(|s| perl_substr(&content, s.position as isize, Some(s.size as isize)))
                .unwrap_or_default();
            for word in split_whitespace(&words) {
                let mut quoted = vec![b'\''];
                quoted.extend(word);
                quoted.push(b'\'');
                push(&mut stacks, i, Node::QwWord { content: quoted });
                i += 1;
            }
            continue;
        }
        push(&mut stacks, i, node.clone());
    }
    stacks.into_iter().map(Arg::Nodes).collect()
}

/// `shebang_line`.
pub fn shebang_line(doc: &Doc) -> Option<Vec<u8>> {
    let first = *doc.children(super::ppi::DOCUMENT).first()?;
    if !doc.isa(first, Class::Comment) {
        return None;
    }
    let location = doc.location(first)?;
    if location.line != 1 || location.character != 1 {
        return None;
    }
    let content = doc.text(first);
    content.starts_with(b"#!").then(|| content.to_vec())
}
