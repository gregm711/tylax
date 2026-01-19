//! TeX Token definitions for the macro expansion engine.
//!
//! This module defines the fundamental token types that mirror how TeX
//! actually processes input - as a stream of categorized tokens.

use std::fmt;

/// A TeX token representing the smallest unit of TeX processing.
///
/// Unlike character-based processing, tokens preserve semantic meaning
/// and allow for correct handling of nested braces and macro arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TexToken {
    /// A control sequence like `\frac`, `\newcommand`, etc.
    /// The string does NOT include the leading backslash.
    ControlSeq(String),

    /// Begin group token `{`
    BeginGroup,

    /// End group token `}`
    EndGroup,

    /// A parameter token like `#1`, `#2`, etc.
    /// The u8 value is the parameter number (1-9).
    Param(u8),

    /// A deferred parameter token like `##1`, `##2`, etc.
    /// Used in nested macro definitions. When the outer macro expands,
    /// this becomes `Param(n)` for the inner macro.
    DeferredParam(u8),

    /// A regular character (letters, digits, punctuation, etc.)
    Char(char),

    /// Whitespace (space, tab, newline normalized to single space)
    Space,

    /// A comment (everything from `%` to end of line)
    /// We preserve comments for potential reconstruction
    Comment(String),

    /// Math shift `$` - for detecting math mode boundaries
    MathShift,

    /// Alignment tab `&` - for tables and alignments
    AlignTab,

    /// Superscript `^`
    Superscript,

    /// Subscript `_`
    Subscript,

    /// Active char `~` (non-breaking space in LaTeX)
    ActiveChar(char),

    /// End of input marker
    EndOfInput,
}

impl TexToken {
    /// Returns true if this token is a begin group `{`
    pub fn is_begin_group(&self) -> bool {
        matches!(self, TexToken::BeginGroup)
    }

    /// Returns true if this token is an end group `}`
    pub fn is_end_group(&self) -> bool {
        matches!(self, TexToken::EndGroup)
    }

    /// Returns true if this token is a control sequence
    pub fn is_control_seq(&self) -> bool {
        matches!(self, TexToken::ControlSeq(_))
    }

    /// Returns true if this token is whitespace
    pub fn is_space(&self) -> bool {
        matches!(self, TexToken::Space)
    }

    /// Returns the control sequence name if this is a ControlSeq token
    pub fn as_control_seq(&self) -> Option<&str> {
        match self {
            TexToken::ControlSeq(name) => Some(name),
            _ => None,
        }
    }

    /// Check if this is a specific control sequence
    pub fn is_cs(&self, name: &str) -> bool {
        matches!(self, TexToken::ControlSeq(n) if n == name)
    }
}

impl fmt::Display for TexToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TexToken::ControlSeq(name) => write!(f, "\\{}", name),
            TexToken::BeginGroup => write!(f, "{{"),
            TexToken::EndGroup => write!(f, "}}"),
            TexToken::Param(n) => write!(f, "#{}", n),
            TexToken::DeferredParam(n) => write!(f, "##{}", n),
            TexToken::Char(c) => write!(f, "{}", c),
            TexToken::Space => write!(f, " "),
            TexToken::Comment(text) => write!(f, "%{}", text),
            TexToken::MathShift => write!(f, "$"),
            TexToken::AlignTab => write!(f, "&"),
            TexToken::Superscript => write!(f, "^"),
            TexToken::Subscript => write!(f, "_"),
            TexToken::ActiveChar(c) => write!(f, "{}", c),
            TexToken::EndOfInput => Ok(()),
        }
    }
}

/// A list of tokens, used for macro bodies and arguments
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenList(pub Vec<TexToken>);

impl TokenList {
    /// Create a new empty token list
    pub fn new() -> Self {
        TokenList(Vec::new())
    }

    /// Create from a vector of tokens
    pub fn from_vec(tokens: Vec<TexToken>) -> Self {
        TokenList(tokens)
    }

    /// Push a token to the list
    pub fn push(&mut self, token: TexToken) {
        self.0.push(token);
    }

    /// Get the inner vector
    pub fn into_inner(self) -> Vec<TexToken> {
        self.0
    }

    /// Get a reference to the inner vector
    pub fn as_slice(&self) -> &[TexToken] {
        &self.0
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get length
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for TokenList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for token in &self.0 {
            write!(f, "{}", token)?;
        }
        Ok(())
    }
}

impl IntoIterator for TokenList {
    type Item = TexToken;
    type IntoIter = std::vec::IntoIter<TexToken>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a TokenList {
    type Item = &'a TexToken;
    type IntoIter = std::slice::Iter<'a, TexToken>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_display() {
        assert_eq!(format!("{}", TexToken::ControlSeq("frac".into())), "\\frac");
        assert_eq!(format!("{}", TexToken::BeginGroup), "{");
        assert_eq!(format!("{}", TexToken::EndGroup), "}");
        assert_eq!(format!("{}", TexToken::Param(1)), "#1");
        assert_eq!(format!("{}", TexToken::Char('x')), "x");
        assert_eq!(format!("{}", TexToken::Space), " ");
    }

    #[test]
    fn test_token_list_display() {
        let tokens = TokenList::from_vec(vec![
            TexToken::ControlSeq("frac".into()),
            TexToken::BeginGroup,
            TexToken::Char('a'),
            TexToken::EndGroup,
            TexToken::BeginGroup,
            TexToken::Char('b'),
            TexToken::EndGroup,
        ]);
        assert_eq!(format!("{}", tokens), "\\frac{a}{b}");
    }
}
