use super::*;

#[test]
fn truncate_for_display_clips_long_strings() {
    let long = "x".repeat(500);
    let out = truncate_for_display(&long, 200);
    assert!(out.ends_with('…'));
    assert_eq!(out.chars().filter(|c| *c == 'x').count(), 200);
}

#[test]
fn truncate_for_display_passes_short_strings_through() {
    let short = "short";
    let out = truncate_for_display(short, 200);
    assert_eq!(out, "short");
}

#[test]
fn truncate_for_display_handles_multibyte_chars_at_boundary() {
    // 200 multibyte chars — make sure we count chars not bytes
    let s: String = "日".repeat(300);
    let out = truncate_for_display(&s, 200);
    assert!(out.ends_with('…'));
    assert_eq!(out.chars().count(), 201); // 200 + ellipsis
}
