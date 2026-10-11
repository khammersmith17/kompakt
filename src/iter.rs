use crate::core::GermanString;
use std::{
    iter::{DoubleEndedIterator, FusedIterator, IntoIterator, Iterator},
    str::Chars,
};

pub struct GermanStringIter<'a> {
    iter: Chars<'a>,
}

impl Iterator for GermanStringIter<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl DoubleEndedIterator for GermanStringIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

impl FusedIterator for GermanStringIter<'_> {}

pub struct GermanStringIntoIter {
    concrete: GermanString,
    front: usize,
    back: usize,
}

impl Iterator for GermanStringIntoIter {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self
            .concrete
            .as_str_slice(self.front..self.back)
            .chars()
            .next()?;
        self.front += item.len_utf8();
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let bytes_remaining = self.back - self.front;
        (bytes_remaining.div_ceil(4), Some(bytes_remaining))
    }
}

impl DoubleEndedIterator for GermanStringIntoIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = self
            .concrete
            .as_str_slice(self.front..self.back)
            .chars()
            .next_back()?;
        self.back -= item.len_utf8();
        Some(item)
    }
}

impl FusedIterator for GermanStringIntoIter {}

impl IntoIterator for GermanString {
    type Item = char;
    type IntoIter = GermanStringIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        let back = self.size();
        GermanStringIntoIter {
            concrete: self,
            front: 0_usize,
            back,
        }
    }
}

impl<'a> IntoIterator for &'a GermanString {
    type Item = char;
    type IntoIter = GermanStringIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl GermanString {
    pub fn iter(&self) -> GermanStringIter<'_> {
        let iter = self.as_str().chars();

        GermanStringIter { iter }
    }
}

#[cfg(test)]
mod test {
    use super::GermanString;

    /// Strings on both sides of the prefix (4 byte) and inline (12 byte) limits, with multi-byte
    /// characters of every width, some of them straddling those limits.
    fn strings() -> Vec<&'static str> {
        vec![
            "",
            "a",
            "abcd",
            "abcde",
            "abcdefghijkl",
            "abcdefghijklm",
            "abcdefghijklmnopqrstuvwxyz",
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
        ]
    }

    fn gs(s: &str) -> GermanString {
        GermanString::new_from_str(s)
    }

    /// Runs `check` against the borrowing and the owning iterator of every test string, together
    /// with the `str::chars` iterator they must agree with.
    fn for_each_iterator(
        check: impl Fn(&mut dyn DoubleEndedIterator<Item = char>, std::str::Chars),
    ) {
        for s in strings() {
            let g = gs(s);
            check(&mut g.iter(), s.chars());
            check(&mut (&g).into_iter(), s.chars());
            check(&mut g.into_iter(), s.chars());
        }
    }

    #[test]
    fn forward_matches_chars() {
        for_each_iterator(|iter, expected| {
            assert_eq!(iter.collect::<Vec<_>>(), expected.collect::<Vec<_>>());
        });
    }

    #[test]
    fn reversed_matches_chars() {
        for_each_iterator(|iter, expected| {
            assert_eq!(
                iter.rev().collect::<Vec<_>>(),
                expected.rev().collect::<Vec<_>>()
            );
        });
    }

    /// Alternating ends must meet in the middle without skipping or repeating a character.
    #[test]
    fn alternating_ends_match_chars() {
        for_each_iterator(|iter, mut expected| {
            for step in 0.. {
                let (got, want) = if step % 2 == 0 {
                    (iter.next(), expected.next())
                } else {
                    (iter.next_back(), expected.next_back())
                };
                assert_eq!(got, want, "step {step}");
                if want.is_none() {
                    break;
                }
            }
        });
    }

    #[test]
    fn stays_exhausted_from_both_ends() {
        for_each_iterator(|iter, _| {
            while iter.next().is_some() {}
            for _ in 0..3 {
                assert_eq!(iter.next(), None);
                assert_eq!(iter.next_back(), None);
            }
        });
    }

    #[test]
    fn size_hint_bounds_the_remaining_count() {
        for_each_iterator(|iter, expected| {
            let mut remaining = expected.count();
            loop {
                let (lower, upper) = iter.size_hint();
                assert!(lower <= remaining, "lower {lower} > remaining {remaining}");
                assert!(
                    upper.is_some_and(|upper| remaining <= upper),
                    "upper {upper:?} < remaining {remaining}"
                );
                // Consume from alternating ends so both indices are exercised.
                let item = if remaining % 2 == 0 {
                    iter.next()
                } else {
                    iter.next_back()
                };
                if item.is_none() {
                    break;
                }
                remaining -= 1;
            }
            assert_eq!(remaining, 0);
            assert_eq!(iter.size_hint(), (0, Some(0)));
        });
    }

    #[test]
    fn empty_string_yields_nothing() {
        assert_eq!(gs("").iter().next(), None);
        assert_eq!(gs("").iter().next_back(), None);
        assert_eq!(gs("").into_iter().next(), None);
        assert_eq!(gs("").into_iter().next_back(), None);
    }

    #[test]
    fn for_loops_work_by_reference_and_by_value() {
        for s in strings() {
            let g = gs(s);

            let mut by_ref = String::new();
            for c in &g {
                by_ref.push(c);
            }
            assert_eq!(by_ref, s);

            let mut by_value = String::new();
            for c in g {
                by_value.push(c);
            }
            assert_eq!(by_value, s);
        }
    }

    #[test]
    fn iter_leaves_the_string_usable() {
        for s in strings() {
            let g = gs(s);
            assert_eq!(g.iter().count(), s.chars().count());
            assert_eq!(g.as_str(), s);
        }
    }

    /// The owning iterator holds the heap allocation, so dropping it part-way must be clean.
    #[test]
    fn partially_consumed_owning_iterator_drops() {
        for s in strings() {
            let mut iter = gs(s).into_iter();
            let mut expected = s.chars();
            assert_eq!(iter.next(), expected.next());
            assert_eq!(iter.next_back(), expected.next_back());
            drop(iter);
        }
    }
}
