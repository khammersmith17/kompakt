mod core;
mod iter;

#[cfg(feature = "serde")]
pub mod serde;

#[cfg(test)]
mod test_util;

pub use core::GermanString;
pub use iter::{GermanStringIntoIter, GermanStringIter};
