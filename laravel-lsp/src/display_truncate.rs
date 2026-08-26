//! Char-safe truncation for values shown in hover cards and completion docs.
//!
//! `s[..limit]` truncation panics whenever a multibyte character straddles
//! the byte index — reachable via any translation or config value with a
//! multibyte character at the cut point. Every display-truncation call site
//! shares this one implementation so that safety property can't regress by
//! being hand-copied somewhere byte-based again.

/// Truncate strings longer than `limit` chars with a `…` ellipsis. Operates
/// on chars (not bytes) so it never splits a multibyte character.
///
/// Used by config/translation dispatch code to clip long resolved values
/// before stuffing them into a code block.
pub fn truncate_for_display(s: &str, limit: usize) -> String {
    if s.chars().count() <= limit {
        return s.to_string();
    }
    let head: String = s.chars().take(limit).collect();
    format!("{}…", head)
}

#[cfg(test)]
mod tests;
