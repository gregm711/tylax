//! Utility functions for token stream parsing

use super::token::{TexToken, TokenList};

/// Maximum number of tokens to read in a single argument.
/// This prevents runaway parsing when input contains unclosed braces.
const MAX_ARG_TOKENS: usize = 10000;

/// Skip space and comment tokens
pub fn skip_spaces<I>(iter: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = TexToken>,
{
    while matches!(
        iter.peek(),
        Some(TexToken::Space) | Some(TexToken::Comment(_))
    ) {
        iter.next();
    }
}

/// Read a control sequence name from the iterator
#[allow(clippy::result_unit_err)]
pub fn read_control_seq_name<I>(iter: &mut std::iter::Peekable<I>) -> Result<String, ()>
where
    I: Iterator<Item = TexToken>,
{
    skip_spaces(iter);
    match iter.next() {
        Some(TexToken::ControlSeq(name)) => Ok(name),
        _ => Err(()),
    }
}

/// Read a number from the iterator
#[allow(clippy::result_unit_err)]
pub fn read_number<I>(iter: &mut std::iter::Peekable<I>) -> Result<u8, ()>
where
    I: Iterator<Item = TexToken>,
{
    skip_spaces(iter);
    let mut num_str = String::new();
    while let Some(TexToken::Char(c)) = iter.peek() {
        if c.is_ascii_digit() {
            num_str.push(*c);
            iter.next();
        } else {
            break;
        }
    }
    num_str.parse().map_err(|_| ())
}

/// Skip tokens until a specific character is encountered
pub fn skip_until_char<I>(iter: &mut I, end_char: char)
where
    I: Iterator<Item = TexToken>,
{
    for token in iter {
        if matches!(token, TexToken::Char(c) if c == end_char) {
            break;
        }
    }
}

/// Read tokens until a specific character is encountered, respecting braces.
///
/// Safety: Limited to `MAX_ARG_TOKENS` to prevent runaway parsing.
pub fn read_until_char<I>(iter: &mut I, end_char: char) -> TokenList
where
    I: Iterator<Item = TexToken>,
{
    let mut result = Vec::new();
    let mut depth = 0;

    for token in iter {
        if result.len() >= MAX_ARG_TOKENS {
            // Safety limit reached - return what we have
            break;
        }
        match &token {
            TexToken::BeginGroup => {
                depth += 1;
                result.push(token);
            }
            TexToken::EndGroup => {
                if depth > 0 {
                    depth -= 1;
                }
                result.push(token);
            }
            TexToken::Char(c) if *c == end_char && depth == 0 => {
                break;
            }
            _ => {
                result.push(token);
            }
        }
    }

    TokenList::from_vec(result)
}

/// Read tokens inside a balanced group { ... }
///
/// Safety: Limited to `MAX_ARG_TOKENS` to prevent runaway parsing
/// when input contains unclosed braces.
pub fn read_balanced_group<I>(iter: &mut I) -> TokenList
where
    I: Iterator<Item = TexToken>,
{
    let mut result = Vec::new();
    let mut depth = 1;

    for token in iter {
        if result.len() >= MAX_ARG_TOKENS {
            // Safety limit reached - return what we have
            break;
        }
        match &token {
            TexToken::BeginGroup => {
                depth += 1;
                result.push(token);
            }
            TexToken::EndGroup => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                result.push(token);
            }
            _ => {
                result.push(token);
            }
        }
    }

    TokenList::from_vec(result)
}

/// Read a single macro argument (braced group or single token)
pub fn read_argument<I>(iter: &mut std::iter::Peekable<I>) -> TokenList
where
    I: Iterator<Item = TexToken>,
{
    skip_spaces(iter);

    match iter.peek() {
        Some(TexToken::BeginGroup) => {
            iter.next();
            read_balanced_group(iter)
        }
        Some(_) => {
            // SAFETY: peek() returned Some, so next() is guaranteed to return Some
            let token = iter.next().expect("peek succeeded");
            TokenList::from_vec(vec![token])
        }
        None => TokenList::new(),
    }
}
