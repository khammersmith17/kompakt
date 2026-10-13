//! Helpers shared by the test modules of this crate.

use crate::core::GermanString;

pub(crate) fn gs(s: &str) -> GermanString {
    GermanString::try_from(s.to_string()).unwrap()
}

/// Strings covering every boundary: empty, inside the prefix, exactly the prefix,
/// spilling into the inline bytes, exactly the inline limit, and heap-allocated.
/// Many share prefixes so comparisons have to look past the first 4 bytes.
pub(crate) fn boundary_strings() -> Vec<&'static str> {
    vec![
        "",
        "a",
        "ab",
        "abc",
        "abcd",
        "abce",
        "abcde",
        "abcdf",
        "abcdefgh",
        "abcdefghijk",
        "abcdefghijkl",
        "abcdefghijkm",
        "abcdefghijklm",
        "abcdefghijkln",
        "abcdefghijklmnopqrst",
        "abcdefghijklmnopqrsu",
        "abd",
        "abdzzzzzzzzzzzzz",
        "b",
        "bbbbbbbbbbbbbbbbbbbb",
        "zzzz",
        "a\0",
        "a\0\0\0",
    ]
}

/// Strings that exercise multi-byte UTF-8 of every width, some of it across the prefix (byte 4)
/// and inline (byte 12) boundaries, in addition to the ASCII boundary strings.
pub(crate) fn all_strings() -> Vec<&'static str> {
    let mut v = boundary_strings();
    v.extend([
        "é",
        "héllo",
        "abcé",
        "abcdefghijké",
        "日本語",
        "日本語テキスト",
        "🦀",
        "🦀🦀🦀🦀",
        "aé日🦀",
        "🦀日éa",
        "a\0b",
        "quote \" and \\ backslash and \n newline",
    ]);
    v
}
