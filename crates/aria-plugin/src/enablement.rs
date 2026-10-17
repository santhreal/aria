//! Enablement conditions: the editor state a command needs before it runs.
//!
//! A condition is a boolean expression over [`ContextKey`]s, such as
//! `selection && !playing`. The host publishes the keys that hold with
//! [`PluginContext::set_context_keys`](crate::PluginContext::set_context_keys);
//! a command whose condition is false is drawn disabled and refused by
//! [`PluginContext::run_command`](crate::PluginContext::run_command).

use std::fmt;

/// One fact about the editor a condition reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextKey {
    /// The sequence holds at least one clip.
    Clips,
    /// At least one clip is selected.
    Selection,
    /// Exactly one clip is selected.
    SingleSelection,
    /// The sequence has an in point.
    MarkIn,
    /// The sequence has an out point.
    MarkOut,
    /// The sequence has both an in point and an out point.
    InOut,
    /// Playback is running.
    Playing,
    /// The Project panel has focus.
    FocusMedia,
    /// The Viewer has focus.
    FocusViewer,
    /// The Timeline has focus.
    FocusTimeline,
    /// Effect Controls has focus.
    FocusInspector,
    /// The Assistant has focus.
    FocusAssistant,
    /// The Audio Track Mixer has focus.
    FocusMixer,
}

impl ContextKey {
    /// Every key, in the order the documentation lists them.
    pub const ALL: [Self; 13] = [
        Self::Clips,
        Self::Selection,
        Self::SingleSelection,
        Self::MarkIn,
        Self::MarkOut,
        Self::InOut,
        Self::Playing,
        Self::FocusMedia,
        Self::FocusViewer,
        Self::FocusTimeline,
        Self::FocusInspector,
        Self::FocusAssistant,
        Self::FocusMixer,
    ];

    /// The name a condition spells the key with.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Clips => "clips",
            Self::Selection => "selection",
            Self::SingleSelection => "single-selection",
            Self::MarkIn => "mark-in",
            Self::MarkOut => "mark-out",
            Self::InOut => "in-out",
            Self::Playing => "playing",
            Self::FocusMedia => "focus.media",
            Self::FocusViewer => "focus.viewer",
            Self::FocusTimeline => "focus.timeline",
            Self::FocusInspector => "focus.inspector",
            Self::FocusAssistant => "focus.assistant",
            Self::FocusMixer => "focus.mixer",
        }
    }

    /// The key spelled `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.name() == name)
    }

    const fn bit(self) -> u32 {
        1 << self as u32
    }
}

impl fmt::Display for ContextKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The set of [`ContextKey`]s that hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ContextKeys(u32);

impl ContextKeys {
    /// No key holds: the state of a context no host has published to.
    pub const NONE: Self = Self(0);

    /// These keys, with `key` set to `on`.
    #[must_use]
    pub const fn with(self, key: ContextKey, on: bool) -> Self {
        if on {
            Self(self.0 | key.bit())
        } else {
            Self(self.0 & !key.bit())
        }
    }

    /// Whether `key` holds.
    pub const fn has(self, key: ContextKey) -> bool {
        self.0 & key.bit() != 0
    }

    pub(crate) const fn bits(self) -> u32 {
        self.0
    }

    pub(crate) const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}

/// A parsed enablement condition.
///
/// The grammar, loosest binding first:
///
/// ```text
/// any  = all ( "||" all )*
/// all  = one ( "&&" one )*
/// one  = "!" one | "(" any ")" | key
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enablement {
    source: Box<str>,
    root: Node,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Key(ContextKey),
    Not(Box<Node>),
    All(Box<[Node]>),
    Any(Box<[Node]>),
}

impl Node {
    fn holds(&self, keys: ContextKeys) -> bool {
        match self {
            Self::Key(key) => keys.has(*key),
            Self::Not(inner) => !inner.holds(keys),
            Self::All(nodes) => nodes.iter().all(|node| node.holds(keys)),
            Self::Any(nodes) => nodes.iter().any(|node| node.holds(keys)),
        }
    }
}

/// The longest condition accepted, in bytes.
pub const ENABLEMENT_MAX_LEN: usize = 256;
/// The deepest nesting of `!` and parentheses accepted.
pub const ENABLEMENT_MAX_DEPTH: usize = 16;

impl Enablement {
    /// Parse `source`. The error states the column and what was expected.
    pub fn parse(source: &str) -> Result<Self, String> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err("the enablement condition is empty".into());
        }
        if trimmed.len() > ENABLEMENT_MAX_LEN {
            return Err(format!(
                "the enablement condition is {} bytes; the limit is {ENABLEMENT_MAX_LEN}",
                trimmed.len()
            ));
        }
        let mut parser = Parser {
            text: trimmed,
            at: 0,
            depth: 0,
        };
        let root = parser.any()?;
        parser.skip_space();
        if parser.at < trimmed.len() {
            return Err(parser.expected("`&&`, `||`, or the end"));
        }
        Ok(Self {
            source: trimmed.into(),
            root,
        })
    }

    /// Whether the condition holds under `keys`.
    pub fn holds(&self, keys: ContextKeys) -> bool {
        self.root.holds(keys)
    }

    /// The condition as written, without surrounding space.
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl fmt::Display for Enablement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn any(&mut self) -> Result<Node, String> {
        let mut nodes = vec![self.all()?];
        while self.eat("||") {
            nodes.push(self.all()?);
        }
        Ok(collapse(nodes, Node::Any))
    }

    fn all(&mut self) -> Result<Node, String> {
        let mut nodes = vec![self.one()?];
        while self.eat("&&") {
            nodes.push(self.one()?);
        }
        Ok(collapse(nodes, Node::All))
    }

    fn one(&mut self) -> Result<Node, String> {
        self.skip_space();
        if self.eat("!") {
            let inner = self.nested(Self::one)?;
            return Ok(Node::Not(Box::new(inner)));
        }
        if self.eat("(") {
            let inner = self.nested(Self::any)?;
            if !self.eat(")") {
                return Err(self.expected("`)`"));
            }
            return Ok(inner);
        }
        self.key()
    }

    fn nested(&mut self, parse: fn(&mut Self) -> Result<Node, String>) -> Result<Node, String> {
        if self.depth == ENABLEMENT_MAX_DEPTH {
            // The `!` or `(` just eaten opens the level past the limit.
            return Err(format!(
                "the enablement condition nests deeper than {ENABLEMENT_MAX_DEPTH} at column {}",
                self.column() - 1
            ));
        }
        self.depth += 1;
        let node = parse(self);
        self.depth -= 1;
        node
    }

    fn key(&mut self) -> Result<Node, String> {
        let rest = &self.text[self.at..];
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
            .unwrap_or(rest.len());
        if len == 0 {
            return Err(self.expected("a key, `!`, or `(`"));
        }
        let name = &rest[..len];
        let Some(key) = ContextKey::from_name(name) else {
            return Err(format!(
                "the enablement condition names an unknown key `{name}` at column {}; the keys are {}",
                self.column(),
                key_list()
            ));
        };
        self.at += len;
        Ok(Node::Key(key))
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_space();
        if self.text[self.at..].starts_with(token) {
            self.at += token.len();
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        let rest = &self.text[self.at..];
        self.at += rest.len() - rest.trim_start().len();
    }

    /// The 1-based character column of the cursor.
    fn column(&self) -> usize {
        self.text[..self.at].chars().count() + 1
    }

    fn expected(&self, what: &str) -> String {
        format!(
            "the enablement condition `{}` expects {what} at column {}",
            self.text,
            self.column()
        )
    }
}

fn collapse(mut nodes: Vec<Node>, join: fn(Box<[Node]>) -> Node) -> Node {
    if nodes.len() == 1
        && let Some(node) = nodes.pop()
    {
        return node;
    }
    join(nodes.into_boxed_slice())
}

fn key_list() -> String {
    let names: Vec<&str> = ContextKey::ALL.iter().map(|key| key.name()).collect();
    names.join(", ")
}

#[cfg(test)]
#[path = "enablement_tests.rs"]
mod tests;
