//! `ShellCheck`'s regular expressions (`ShellCheck.Regex`, on `regex-tdfa`).
//!
//! `regex-tdfa` is a POSIX extended regex engine, so a match is the leftmost
//! one and, among those, the longest; subgroups follow the POSIX
//! subexpression rule (earlier groups first, each as early and then as long
//! as the overall match allows). `ShellCheck` compiles with the defaults:
//! multiline, so `^` and `$` also match at line breaks and neither `.` nor a
//! negated bracket matches a newline; an escaped character outside brackets
//! is that character literally (`\d` is `d`).
//!
//! The patterns `ShellCheck` uses are small and run on short strings, so this
//! is a backtracking matcher that enumerates the candidate matches at a
//! start position and picks the POSIX one.

#[derive(Clone, Debug)]
enum ClassItem {
    Char(char),
    Range(char, char),
    Named(String),
}

#[derive(Clone, Debug)]
enum Node {
    Char(char),
    Any,
    Class(bool, Vec<ClassItem>),
    Start,
    End,
    Group(Box<Self>, usize),
    Alt(Vec<Self>),
    Concat(Vec<Self>),
    Repeat(Box<Self>, usize, Option<usize>),
}

#[derive(Clone, Debug)]
pub struct Regex {
    node: Node,
    groups: usize,
}

type Caps = Vec<Option<(usize, usize)>>;

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    groups: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn alt(&mut self) -> Node {
        let mut branches = vec![self.concat()];
        while self.peek() == Some('|') {
            self.pos += 1;
            branches.push(self.concat());
        }
        if branches.len() == 1 {
            branches.pop().expect("one branch")
        } else {
            Node::Alt(branches)
        }
    }

    fn concat(&mut self) -> Node {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom();
            items.push(self.repeats(atom));
        }
        Node::Concat(items)
    }

    fn repeats(&mut self, mut atom: Node) -> Node {
        loop {
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    atom = Node::Repeat(Box::new(atom), 0, None);
                }
                Some('+') => {
                    self.pos += 1;
                    atom = Node::Repeat(Box::new(atom), 1, None);
                }
                Some('?') => {
                    self.pos += 1;
                    atom = Node::Repeat(Box::new(atom), 0, Some(1));
                }
                Some('{') => {
                    let save = self.pos;
                    self.pos += 1;
                    let min = self.number();
                    let (min, max) = match (min, self.peek()) {
                        (Some(n), Some('}')) => {
                            self.pos += 1;
                            (n, Some(n))
                        }
                        (Some(n), Some(',')) => {
                            self.pos += 1;
                            let max = self.number();
                            if self.peek() == Some('}') {
                                self.pos += 1;
                                (n, max)
                            } else {
                                self.pos = save;
                                return atom;
                            }
                        }
                        _ => {
                            self.pos = save;
                            return atom;
                        }
                    };
                    atom = Node::Repeat(Box::new(atom), min, max);
                }
                _ => return atom,
            }
        }
    }

    fn number(&mut self) -> Option<usize> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if start == self.pos {
            None
        } else {
            self.chars[start..self.pos]
                .iter()
                .collect::<String>()
                .parse()
                .ok()
        }
    }

    fn atom(&mut self) -> Node {
        let c = self.peek().expect("atom at end of pattern");
        self.pos += 1;
        match c {
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '(' => {
                self.groups += 1;
                let idx = self.groups;
                let inner = self.alt();
                if self.peek() == Some(')') {
                    self.pos += 1;
                }
                Node::Group(Box::new(inner), idx)
            }
            '[' => self.bracket(),
            '\\' => match self.peek() {
                Some(e) => {
                    self.pos += 1;
                    Node::Char(e)
                }
                None => Node::Char('\\'),
            },
            _ => Node::Char(c),
        }
    }

    fn bracket(&mut self) -> Node {
        let mut negated = false;
        if self.peek() == Some('^') {
            negated = true;
            self.pos += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        while let Some(c) = self.peek() {
            if c == ']' && !first {
                self.pos += 1;
                break;
            }
            first = false;
            if c == '[' && self.chars.get(self.pos + 1) == Some(&':') {
                let rest: String = self.chars[self.pos + 2..].iter().collect();
                if let Some(end) = rest.find(":]") {
                    let name = rest[..end].to_string();
                    self.pos += 2 + name.chars().count() + 2;
                    items.push(ClassItem::Named(name));
                    continue;
                }
            }
            self.pos += 1;
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).is_some_and(|&n| n != ']') {
                let hi = self.chars[self.pos + 1];
                self.pos += 2;
                items.push(ClassItem::Range(c, hi));
            } else {
                items.push(ClassItem::Char(c));
            }
        }
        Node::Class(negated, items)
    }
}

fn named_class(name: &str, c: char) -> bool {
    match name {
        "alpha" => c.is_alphabetic(),
        "digit" => c.is_ascii_digit(),
        "alnum" => c.is_alphanumeric(),
        "upper" => c.is_uppercase(),
        "lower" => c.is_lowercase(),
        "space" => c.is_whitespace(),
        "blank" => c == ' ' || c == '\t',
        "punct" => c.is_ascii_punctuation(),
        "xdigit" => c.is_ascii_hexdigit(),
        "cntrl" => c.is_control(),
        "print" => !c.is_control(),
        "graph" => !c.is_control() && !c.is_whitespace(),
        "word" => c.is_alphanumeric() || c == '_',
        _ => false,
    }
}

impl Regex {
    pub fn new(pattern: &str) -> Self {
        let chars: Vec<char> = pattern.chars().collect();
        let mut p = Parser {
            chars: &chars,
            pos: 0,
            groups: 0,
        };
        let node = p.alt();
        Self {
            node,
            groups: p.groups,
        }
    }

    fn m(
        &self,
        node: &Node,
        s: &[char],
        pos: usize,
        caps: &mut Caps,
        k: &mut dyn FnMut(usize, &mut Caps) -> bool,
    ) -> bool {
        match node {
            Node::Char(c) => s.get(pos) == Some(c) && k(pos + 1, caps),
            Node::Any => s.get(pos).is_some_and(|&c| c != '\n') && k(pos + 1, caps),
            Node::Class(neg, items) => match s.get(pos) {
                Some(&c) => {
                    let inside = items.iter().any(|item| match item {
                        ClassItem::Char(x) => *x == c,
                        ClassItem::Range(lo, hi) => *lo <= c && c <= *hi,
                        ClassItem::Named(name) => named_class(name, c),
                    });
                    let ok = if *neg { !inside && c != '\n' } else { inside };
                    ok && k(pos + 1, caps)
                }
                None => false,
            },
            Node::Start => (pos == 0 || s[pos - 1] == '\n') && k(pos, caps),
            Node::End => (pos == s.len() || s[pos] == '\n') && k(pos, caps),
            Node::Group(inner, idx) => {
                let idx = *idx;
                self.m(inner, s, pos, caps, &mut |end, caps: &mut Caps| {
                    let old = caps[idx];
                    caps[idx] = Some((pos, end));
                    let r = k(end, caps);
                    caps[idx] = old;
                    r
                })
            }
            Node::Alt(branches) => branches.iter().any(|b| self.m(b, s, pos, caps, k)),
            Node::Concat(items) => self.seq(items, s, pos, caps, k),
            Node::Repeat(inner, min, max) => self.rep(inner, *min, *max, 0, s, pos, caps, k),
        }
    }

    fn seq(
        &self,
        items: &[Node],
        s: &[char],
        pos: usize,
        caps: &mut Caps,
        k: &mut dyn FnMut(usize, &mut Caps) -> bool,
    ) -> bool {
        match items.split_first() {
            None => k(pos, caps),
            Some((first, rest)) => self.m(first, s, pos, caps, &mut |p, caps: &mut Caps| {
                self.seq(rest, s, p, caps, k)
            }),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the repetition state is the recursion's arguments"
    )]
    fn rep(
        &self,
        inner: &Node,
        min: usize,
        max: Option<usize>,
        count: usize,
        s: &[char],
        pos: usize,
        caps: &mut Caps,
        k: &mut dyn FnMut(usize, &mut Caps) -> bool,
    ) -> bool {
        // Greedy: try one more iteration first. An iteration that matched
        // nothing ends the repetition (it could repeat forever).
        if max.is_none_or(|m| count < m)
            && self.m(inner, s, pos, caps, &mut |p, caps: &mut Caps| {
                if p == pos && count >= min {
                    return false;
                }
                self.rep(inner, min, max, count + 1, s, p, caps, k)
            })
        {
            return true;
        }
        count >= min && k(pos, caps)
    }

    /// All the matches starting at `start`: (end, groups).
    fn matches_at(&self, s: &[char], start: usize) -> Vec<(usize, Caps)> {
        let mut out = Vec::new();
        let mut caps: Caps = vec![None; self.groups + 1];
        self.m(
            &self.node,
            s,
            start,
            &mut caps,
            &mut |end, caps: &mut Caps| {
                out.push((end, caps.clone()));
                false
            },
        );
        out
    }

    /// The POSIX match starting at `start`.
    fn best_at(&self, s: &[char], start: usize) -> Option<(usize, Caps)> {
        let all = self.matches_at(s, start);
        let longest = all.iter().map(|(e, _)| *e).max()?;
        let mut best: Option<(usize, Caps)> = None;
        for (end, caps) in all {
            if end != longest {
                continue;
            }
            let better = match &best {
                None => true,
                Some((_, b)) => posix_better(&caps, b),
            };
            if better {
                best = Some((end, caps));
            }
        }
        best
    }

    /// The first match: (start, end, groups).
    fn find(&self, s: &[char]) -> Option<(usize, usize, Caps)> {
        (0..=s.len()).find_map(|start| self.best_at(s, start).map(|(end, caps)| (start, end, caps)))
    }

    /// `matches`.
    pub fn is_match(&self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        (0..=chars.len()).any(|start| {
            let mut caps: Caps = vec![None; self.groups + 1];
            self.m(&self.node, &chars, start, &mut caps, &mut |_, _| true)
        })
    }

    fn groups_of(caps: &Caps, s: &[char]) -> Vec<String> {
        caps[1..]
            .iter()
            .map(|c| c.map_or_else(String::new, |(a, b)| s[a..b].iter().collect()))
            .collect()
    }

    /// `matchRegex`: the subgroups of the first match.
    pub fn match_groups(&self, s: &str) -> Option<Vec<String>> {
        let chars: Vec<char> = s.chars().collect();
        let (_, _, caps) = self.find(&chars)?;
        Some(Self::groups_of(&caps, &chars))
    }

    /// `matchM`: (before, match, after, groups).
    pub fn match_parts(&self, s: &str) -> Option<(String, String, String, Vec<String>)> {
        let chars: Vec<char> = s.chars().collect();
        let (start, end, caps) = self.find(&chars)?;
        Some((
            chars[..start].iter().collect(),
            chars[start..end].iter().collect(),
            chars[end..].iter().collect(),
            Self::groups_of(&caps, &chars),
        ))
    }

    /// `matchAllStrings`.
    pub fn match_all_strings(&self, s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = s.to_string();
        while let Some((_, m, after, _)) = self.match_parts(&rest) {
            out.push(m);
            rest = after;
        }
        out
    }

    /// `matchAllSubgroups`.
    pub fn match_all_subgroups(&self, s: &str) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        let mut rest = s.to_string();
        while let Some((_, _, after, groups)) = self.match_parts(&rest) {
            out.push(groups);
            rest = after;
        }
        out
    }

    /// `subRegex`.
    pub fn replace_all(&self, input: &str, replacement: &str) -> String {
        match self.match_parts(input) {
            Some((before, m, after, _)) => {
                assert!(
                    !m.is_empty(),
                    "Internal error: substituted empty in {input}"
                );
                format!(
                    "{before}{replacement}{}",
                    self.replace_all(&after, replacement)
                )
            }
            None => input.to_string(),
        }
    }
}

/// The POSIX subexpression rule: earlier groups first, each preferring to
/// participate, then the earlier start, then the longer span.
fn posix_better(a: &Caps, b: &Caps) -> bool {
    for (x, y) in a.iter().zip(b.iter()).skip(1) {
        match (x, y) {
            (Some(_), None) => return true,
            (None, Some(_)) => return false,
            (Some((xs, xe)), Some((ys, ye))) => {
                if xs != ys {
                    return xs < ys;
                }
                if xe != ye {
                    return xe > ye;
                }
            }
            (None, None) => {}
        }
    }
    false
}

/// `mkRegex`.
pub fn mk_regex(pattern: &str) -> Regex {
    Regex::new(pattern)
}
