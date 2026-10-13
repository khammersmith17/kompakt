//! This crate is an implementation of whats often called a German String. The idea came from the
//! following paper out of TU Munich: [Umbra: A Disk-Based System with In-Memory Performance](https://www.cidrdb.org/cidr2020/papers/p29-neumann-cidr20.pdf)
//! (Neumann and Freitag, CIDR 2020).
//!
//! The idea is that strings are often rather small and they do not change often. [String] from the
//! standard library is designed from growable strings, and just wraps a [Vec<u8>].
//!
//! Given the property of immutability, and that most strings are small, a string can be
//! represented in 16 bytes, and only needs to store data indirect on the heap when needed.
//!
//! This representation stores the size as a [u32], a 4 byte prefix, and either an inline 8 byte
//! array, or a 8 byte pointer to the heap. This compact representation can store strings whose
//! underlying bytes are less than or equal to length can be stored on the stack.
//!
//! An allocation of [GermanString] can be stored largely contiguously.
//!
//! Ordering also becomes extremely cheap in many cases, especially when strings do not share a
//! prefix.
//!
//! [GermanString] includes common iterator implementations and opt in serde with the "serde"
//! feature.
mod core;
mod error;
mod iter;

#[cfg(feature = "serde")]
pub mod serde;

#[cfg(test)]
mod test_util;

pub use core::GermanString;
pub use error::GermanStringLengthError;
pub use iter::{GermanStringIntoIter, GermanStringIter};
