use std::fmt;

fn length_panic_message(size: u64) -> String {
    format!(
        "Length {} is too large. GermanString size if limited to strings less than u32::MAX in length",
        size,
    )
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct GermanStringLengthError(pub u64);

impl fmt::Display for GermanStringLengthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", length_panic_message(self.0))
    }
}

impl fmt::Debug for GermanStringLengthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", length_panic_message(self.0))
    }
}

impl std::error::Error for GermanStringLengthError {}
