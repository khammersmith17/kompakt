use std::{
    cmp::Ordering,
    fmt,
    ops::{Index, Range},
    ptr::{self, NonNull},
};

const PREFIX_SIZE: usize = 4_usize;
const INLINE_SIZE: usize = 8_usize;
const SMALL_STRING_SIZE: usize = PREFIX_SIZE + INLINE_SIZE;

fn index_panic_message(idx: usize, size: u32) -> String {
    format!("Index {} is out of bounds for string length {}", idx, size)
}

fn length_panic_message(size: usize) -> String {
    format!(
        "Length {} is too large. GermanString size if limited to strings less than u32::MAX in length",
        size,
    )
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[repr(u8)]
pub enum GermanStringType {
    Small = 0,
    Large = 1,
}

impl GermanStringType {
    fn from_is_large(is_large: bool) -> GermanStringType {
        unsafe { std::mem::transmute(u8::from(is_large)) }
    }
}

#[repr(C)]
union Variant {
    small: [u8; INLINE_SIZE],
    large: NonNull<u8>,
}

/// A compact immutable string representation. This is inspired by research at TU Munich:
/// [Umbra: A Disk-Based System with In-Memory Performance](https://www.cidrdb.org/cidr2020/papers/p29-neumann-cidr20.pdf)
///
/// (Neumann and Freitag, CIDR 2020).
///
/// The idea is that many strings are small, and often do not change. Thus, they do not need to be
/// mutable/growable in many cases. Given the typical String implementation contains a pointer,
/// length and capacity to some heap allocation, there is some wasted space.
///
/// This string representation is 16 bytes: a 32 bit size, a 4 byte prefix, and 8 more bytes that
/// hold either the rest of the string or a pointer to a fixed sized allocation on the heap.
///
/// Thus, this representation can store strings <= 12 bytes entirely inline, without allocating.
///
/// There are two variants, small and large. The small case is when the entire string fits in the
/// 4 byte prefix plus the 8 inline bytes. Large is the variant that requires a separate heap
/// allocation, which holds the whole string; the prefix is kept inline as well so that many
/// comparisons are decided without following the pointer.
#[repr(C)]
pub struct GermanString {
    size: u32,
    prefix: [u8; 4],
    variant: Variant,
}

// SAFETY: NonNull ppointer disables safety.
// This type is safe to send/share across threads as it is immutable.
unsafe impl Send for GermanString {}
unsafe impl Sync for GermanString {}

impl AsRef<str> for GermanString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::hash::Hash for GermanString {
    // Delegate to str.
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        self.as_str().hash(hasher);
    }
}

impl fmt::Debug for GermanString {
    // Diverge from String for Debug as layout can be important for debugging.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = self.suffix();
        f.debug_struct("GermanString")
            .field("size", &self.size)
            .field("prefix", &self.prefix)
            .field("variant", &variant)
            .finish()
    }
}

impl fmt::Display for GermanString {
    // Consistent with Sting's Display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Index<usize> for GermanString {
    type Output = u8;

    #[inline]
    fn index(&self, idx: usize) -> &Self::Output {
        assert!(
            idx < self.size as usize,
            "{}",
            index_panic_message(idx, self.size)
        );

        // Use prefix if we can.
        if idx < PREFIX_SIZE {
            return &self.prefix[idx];
        }

        // Delegate to variant specific method.
        match self.variant_type() {
            GermanStringType::Large => self.index_overflow(idx),
            GermanStringType::Small => self.index_inline(idx),
        }
    }
}

impl PartialEq for GermanString {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        // First check header.
        if self.header() != other.header() {
            return false;
        }

        // Otherwise delegate the &[u8] ordering.
        match self.variant_type() {
            GermanStringType::Small => unsafe { self.variant.small == other.variant.small },
            GermanStringType::Large => self.overflow_ref() == other.overflow_ref(),
        }
    }
}

impl PartialEq<str> for GermanString {
    fn eq(&self, other: &str) -> bool {
        self.as_str().eq(other)
    }
}

impl PartialEq<&str> for GermanString {
    fn eq(&self, other: &&str) -> bool {
        self.as_str().eq(*other)
    }
}

impl PartialEq<String> for GermanString {
    fn eq(&self, other: &String) -> bool {
        self.as_str().eq(other.as_str())
    }
}

impl Eq for GermanString {}

impl PartialOrd for GermanString {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // First try prefix order.
        let prefix_ord = self.cmp_prefix(other);

        if !matches!(prefix_ord, Ordering::Equal) {
            return Some(prefix_ord);
        }

        // Determine where the rest lives, and use built in u8 ordering.
        let ord = match (self.variant_type(), other.variant_type()) {
            (GermanStringType::Small, GermanStringType::Small) => {
                // Best path, both inline.
                let (mine, theirs) = unsafe {
                    (
                        u64::from_be_bytes(self.variant.small),
                        u64::from_be_bytes(other.variant.small),
                    )
                };
                mine.cmp(&theirs).then(self.size.cmp(&other.size))
            }
            _ => {
                // Otherwise, at least one is indirect.
                let mine = self.suffix();
                let theirs = other.suffix();
                mine.cmp(&theirs)
            }
        };

        Some(ord)
    }
}

impl PartialOrd<str> for GermanString {
    /// Delegate to str ordering.
    #[inline]
    fn partial_cmp(&self, other: &str) -> Option<Ordering> {
        self.as_str().partial_cmp(other)
    }
}

impl PartialOrd<&str> for GermanString {
    /// Delegate to str ordering.
    #[inline]
    fn partial_cmp(&self, other: &&str) -> Option<Ordering> {
        self.as_str().partial_cmp(*other)
    }
}

impl PartialOrd<String> for GermanString {
    /// Delegate to str ordering.
    #[inline]
    fn partial_cmp(&self, other: &String) -> Option<Ordering> {
        self.as_str().partial_cmp(other.as_str())
    }
}

impl Ord for GermanString {
    /// Delegate to str ordering.
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl GermanString {
    /// Build a new [GermanString] from a [&str].
    /// The only fallible condition is when the [&str] is larger than u32::MAX in size.
    #[inline]
    pub fn new_from_str(s: &str) -> GermanString {
        let len = s.len();
        GermanString::new_from_str_fallible(s).expect(&length_panic_message(len))
    }

    /// Build a new [GermanString] from a [String]. This is most performant when the string is
    /// large.
    #[inline]
    pub fn new_from_string(s: String) -> GermanString {
        let len = s.len();
        GermanString::new_fallible(s).expect(&length_panic_message(len))
    }

    /// Safe get [char].
    pub fn get(&self, idx: usize) -> Option<char> {
        self.as_str().get(idx..)?.chars().next()
    }

    /// Get a view into the underlying &[u8] slice.
    pub fn as_bytes(&self) -> &[u8] {
        self.as_ref()
    }

    /// Get a view of the [GermanString] as a &str.
    pub fn as_str(&self) -> &str {
        self.as_str_slice(0..self.size())
    }

    /// Build a [String] without transfering ownership. If you no longer need ownership, the
    /// [Into<String] implementation is zero copy.
    pub fn as_string(&self) -> String {
        self.string_from_ref()
    }

    /// Get the total size of a [GermanString].
    pub fn len(&self) -> usize {
        self.size as usize
    }
}

/// Private methods.
impl GermanString {
    pub(crate) fn as_str_slice(&self, rng: Range<usize>) -> &str {
        // SAFETY: It is impossible to construct a non-utf8 encoded string.
        unsafe { std::str::from_utf8_unchecked(self.as_ref().get_unchecked(rng)) }
    }

    #[inline]
    pub(crate) fn size(&self) -> usize {
        self.size as usize
    }

    #[inline]
    pub(crate) fn new_fallible(s: String) -> Result<GermanString, ()> {
        if s.as_bytes().len() > u32::MAX as usize {
            return Err(());
        }
        if s.len() <= SMALL_STRING_SIZE {
            Ok(GermanString::new_small(s))
        } else {
            Ok(GermanString::new_large(s))
        }
    }

    #[inline]
    pub(crate) fn new_from_str_fallible(s: &str) -> Result<GermanString, ()> {
        if s.as_bytes().len() > u32::MAX as usize {
            return Err(());
        }
        if s.len() <= SMALL_STRING_SIZE {
            Ok(GermanString::new_small_from_str(&s))
        } else {
            Ok(GermanString::new_large_from_str(&s))
        }
    }

    /// Determine the variant from the size.
    #[inline]
    fn variant_type(&self) -> GermanStringType {
        GermanStringType::from_is_large(self.size as usize > SMALL_STRING_SIZE)
    }

    #[inline]
    fn as_ref(&self) -> &[u8] {
        let size = self.size as usize;
        match self.variant_type() {
            GermanStringType::Small => unsafe {
                // Get the offset of prefix.
                // Fusing the inline portion with the small inline array is UB from taking the
                // prefix pointer directly, as flagged by miri.
                // This should always be 4 given repr(C).
                let prefix_offset = std::mem::offset_of!(Self, prefix);
                let ptr = (self as *const Self as *const u8).add(prefix_offset);
                std::slice::from_raw_parts(ptr, size)
            },
            GermanStringType::Large => unsafe {
                let ptr = self.variant.large.as_ptr();
                std::slice::from_raw_parts(ptr, size)
            },
        }
    }

    /// Consruct a new small [GermanString] from a standard String.
    #[inline]
    fn new_small_from_str(s: &str) -> GermanString {
        // Assert length is valid.
        debug_assert!(s.len() <= SMALL_STRING_SIZE);
        let size = s.len();
        let src = s.as_ptr();

        // Register based approach to reading the prefix + inline portion.
        // Read the first "half" using the largest primitive possible(u64, u32, u16).
        // Then read a possible overlapping portion to avoid UB and only read data that s
        // owns. Both reads in little endian order.
        //
        // After both reads, shift the overlapping portion to zero pad the second read by shifting right, given le.
        //
        // Widen both reads, padding the LSB given LE order. When the string is less than 4 in length,
        // then inline read is a no-op.
        let (low_bytes, high_bytes) = unsafe {
            if size >= 8 {
                // head: [0, 8)
                // tail [size - 4, size)
                // head will the [first 4 bytes of inline portion][preifx] in little endian order.
                let head = u64::from_le(ptr::read_unaligned(src as *const u64));
                // Read the last 4 bytes that s owns.
                let tail = u32::from_le(ptr::read_unaligned(src.add(size - 4) as *const u32));

                // Widen last 4 bytes of inline out, and shift to 4 LSB in little endian order.
                let lo = u64::from(tail) >> ((SMALL_STRING_SIZE - size) * 8);
                (head, lo)
            } else if size >= 4 {
                // u32 reads to avoid UB.
                // Entire prefix + inline fits in a single 8 byte unsigned integer.
                let head = u32::from_le(ptr::read_unaligned(src as *const u32));
                let tail = u32::from_le(ptr::read_unaligned(src.add(size - 4) as *const u32));

                // Widen head + OR the inline bits to the MSB in little endidan order.
                let lo = u64::from(head) | (u64::from(tail) << (size - 4) * 8);
                (u64::from(lo), 0)
            } else if size >= 2 {
                // u16 reads
                // No inline portion here.
                let head = u16::from_le(ptr::read_unaligned(src as *const u16));
                let tail = u16::from_le(ptr::read_unaligned(src.add(size - 2) as *const u16));

                let lo = u64::from(head) | (u64::from(tail) << (size - 2) * 8);
                (u64::from(lo), 0)
            } else if size == 1 {
                (u64::from(*src), 0)
            } else {
                (0, 0)
            }
        };

        GermanString {
            size: size as u32,
            // Given little endian, the prefix is stored in the LSBs.
            prefix: (low_bytes as u32).to_le_bytes(),
            // Shift out the prefix, and swap the first half on the inline to the MSG of the little
            // endian layout.
            variant: Variant {
                small: ((low_bytes >> 32) | (high_bytes << 32)).to_le_bytes(),
            },
        }
    }

    /// Consruct a new large [GermanString] from a standard String.
    fn new_large_from_str(s: &str) -> GermanString {
        // Assert length is valid.
        debug_assert!(s.len() > PREFIX_SIZE);

        // Copy prefix into prefix array.
        let size = s.len() as u32;
        let mut prefix = [0_u8; 4];
        let s_bytes = s.as_bytes();
        prefix[..].copy_from_slice(&s_bytes[..PREFIX_SIZE]);

        // Copy the rest into a heap allocated slice.
        let rest_alloc = s_bytes.to_vec().into_boxed_slice();
        let rest_ptr = Box::into_raw(rest_alloc) as *mut u8;
        let rest = unsafe { NonNull::new_unchecked(rest_ptr) };

        GermanString {
            size,
            prefix,
            variant: Variant { large: rest },
        }
    }

    /// Consruct a new small [GermanString] from a standard String.
    #[inline]
    fn new_small(s: String) -> GermanString {
        GermanString::new_small_from_str(&s)
    }

    /// Consruct a new large [GermanString] from a standard String.
    ///
    /// When ownership of the source string is transfered, the heap allocation can be directly
    /// used.
    fn new_large(s: String) -> GermanString {
        // Assert length is valid.
        debug_assert!(s.len() > PREFIX_SIZE);

        // Copy prefix into prefix array.
        let size = s.len() as u32;
        let mut prefix = [0_u8; 4];
        let s_bytes = s.as_bytes();
        prefix[..].copy_from_slice(&s_bytes[..PREFIX_SIZE]);

        // Resue string allocation. Adds 4 bytes of overhead on the heap, does not
        // require a copy, and makes comparisons much faster.
        let heap = s.into_boxed_str();
        let rest_ptr = Box::into_raw(heap) as *mut u8;
        let rest = unsafe { NonNull::new_unchecked(rest_ptr) };

        GermanString {
            size,
            prefix,
            variant: Variant { large: rest },
        }
    }

    /// Suffix is data stored in the variant.
    #[inline]
    fn suffix(&self) -> &[u8] {
        match self.variant_type() {
            GermanStringType::Small => self.inline_ref(),
            GermanStringType::Large => self.overflow_ref(),
        }
    }

    /// Get a reference to the inline portion of a small string.
    #[inline]
    fn inline_ref(&self) -> &[u8] {
        debug_assert_eq!(self.variant_type(), GermanStringType::Small);
        unsafe { &self.variant.small }
    }

    /// Get a referene to the overflow heap allocation for a large string.
    #[inline]
    fn overflow_ref(&self) -> &[u8] {
        debug_assert_eq!(self.variant_type(), GermanStringType::Large);
        // Overflow stores the entire allocation, but this method only provides the overflow
        // portion.
        unsafe {
            let ptr = self.variant.large.as_ptr().add(PREFIX_SIZE);
            std::slice::from_raw_parts(ptr, (self.size as usize) - PREFIX_SIZE)
        }
    }

    /// Index into the inline portion when the string is small.
    #[inline]
    fn index_inline(&self, idx: usize) -> &u8 {
        debug_assert_eq!(self.variant_type(), GermanStringType::Small);
        let inline = self.inline_ref();
        let offset = idx - PREFIX_SIZE;
        &inline[offset]
    }

    /// Index into the overflow portion when the string is large.
    #[inline]
    fn index_overflow(&self, idx: usize) -> &u8 {
        debug_assert_eq!(self.variant_type(), GermanStringType::Large);
        unsafe {
            let ptr = self.variant.large.as_ptr();
            ptr.add(idx).as_ref().unwrap()
        }
    }

    /// Header consists of size + prefix fused into a u64.
    /// Cheap equality check before delegating to the union.
    #[inline]
    fn header(&self) -> u64 {
        (u64::from(self.size) << 32) | u64::from(u32::from_ne_bytes(self.prefix))
    }

    /// Compare the prefix using u32 Ord implementation.
    /// The ordering result will be the same as [u8, 4].
    #[inline]
    fn cmp_prefix(&self, other: &GermanString) -> Ordering {
        let mine = u32::from_be_bytes(self.prefix);
        let theirs = u32::from_be_bytes(other.prefix);
        mine.cmp(&theirs)
    }

    /// Coerce the heap allocated portion into a String.
    fn to_string_large(self) -> String {
        // Manually drop self so the String allocation is not dropped.
        let this = std::mem::ManuallyDrop::new(self);
        let size = this.size as usize;

        // Reuse the overflow allocation.
        let blob = unsafe {
            let ptr = this.variant.large.as_ptr();
            Vec::from_raw_parts(ptr, size, size)
        };
        // Hand it back.
        // SAFETY: Construction validates utf8.
        unsafe { String::from_utf8_unchecked(blob) }
    }

    /// Build a string from the prefix + inline portion.
    fn to_string_small(self) -> String {
        // SAFETY: Construction validates utf8.
        unsafe { String::from_utf8_unchecked(self.as_ref().to_vec()) }
    }

    /// Build a string from a large german string and maintain ownership.
    fn string_from_ref(&self) -> String {
        // SAFETY: Construction validates utf8.
        unsafe { String::from_utf8_unchecked(self.as_ref().to_vec()) }
    }
}

impl Drop for GermanString {
    fn drop(&mut self) {
        // Explicitly drop heap allocation if needed.
        if self.variant_type() == GermanStringType::Large {
            let boxed_overflow = unsafe {
                let ptr = self.variant.large.as_ptr();
                let rest = std::ptr::slice_from_raw_parts_mut(ptr, self.size as usize);
                Box::from_raw(rest)
            };
            drop(boxed_overflow);
        }
    }
}

impl From<String> for GermanString {
    #[inline]
    fn from(s: String) -> GermanString {
        GermanString::new_from_string(s)
    }
}

impl Into<String> for GermanString {
    // Into given it is impossible to construct a non utf8 string here.
    fn into(self) -> String {
        match self.variant_type() {
            GermanStringType::Small => self.to_string_small(),
            GermanStringType::Large => self.to_string_large(),
        }
    }
}

#[cfg(test)]
mod test {
    use std::cmp::Ordering;

    use super::{GermanString, GermanStringType};
    use crate::test_util::{all_strings, boundary_strings, gs};

    const PREFIX_SIZE: usize = 4;
    const SMALL_STRING_SIZE: usize = 12;

    #[should_panic]
    #[test]
    fn test_too_large_panics() {
        let s = "a".repeat(u32::MAX as usize + 1);
        gs(&s);
    }

    #[test]
    fn size_is_16_bytes() {
        assert_eq!(std::mem::size_of::<GermanString>(), 16);
    }

    #[test]
    fn constructs_every_length_up_to_32() {
        let source = "abcdefghijklmnopqrstuvwxyz012345";
        for len in 0..=source.len() {
            let _ = gs(&source[..len]);
        }
    }

    #[test]
    fn strings_up_to_12_bytes_are_small() {
        let source = "abcdefghijkl";
        for len in 0..=SMALL_STRING_SIZE {
            let s = gs(&source[..len]);
            assert!(
                matches!(s.variant_type(), GermanStringType::Small),
                "length {len} should be Small"
            );
        }
    }

    #[test]
    fn strings_over_12_bytes_are_large() {
        let source = "abcdefghijklmnopqrstuvwxyz";
        for len in (SMALL_STRING_SIZE + 1)..=source.len() {
            let s = gs(&source[..len]);
            assert!(
                matches!(s.variant_type(), GermanStringType::Large),
                "length {len} should be Large"
            );
        }
    }

    #[test]
    fn index_returns_every_byte() {
        let source = "abcdefghijklmnopqrstuvwxyz012345";
        for len in 0..=source.len() {
            let expected = &source.as_bytes()[..len];
            let s = gs(&source[..len]);
            for (i, &b) in expected.iter().enumerate() {
                assert_eq!(s[i], b, "length {len}, index {i}");
            }
        }
    }

    #[test]
    fn get_char_ascii() {
        let s = gs("hello, german string");
        for (i, c) in "hello, german string".chars().enumerate() {
            assert_eq!(s.get(i), Some(c));
        }
        assert_eq!(s.get(100), None);
    }

    #[test]
    #[should_panic]
    fn index_out_of_bounds_small_panics() {
        let s = gs("abc");
        let _ = s[3];
    }

    #[test]
    #[should_panic]
    fn index_out_of_bounds_inline_panics() {
        let s = gs("abcdefgh");
        let _ = s[8];
    }

    #[test]
    #[should_panic]
    fn index_out_of_bounds_large_panics() {
        let s = gs("abcdefghijklmnop");
        let _ = s[16];
    }

    #[test]
    fn equal_contents_are_equal_across_separate_allocations() {
        for s in boundary_strings() {
            assert!(gs(s) == gs(s), "{s:?} should equal itself");
        }
    }

    #[test]
    fn equality_matches_str() {
        let words = boundary_strings();
        for a in &words {
            for b in &words {
                assert_eq!(gs(a) == gs(b), a == b, "{a:?} == {b:?}");
            }
        }
    }

    #[test]
    fn trailing_nul_is_not_equal() {
        // Zero padding must not make these collide.
        assert!(gs("a") != gs("a\0"));
        assert!(gs("abcd") != gs("abcd\0"));
    }

    #[test]
    fn ordering_matches_str_for_all_pairs() {
        let words = boundary_strings();
        for a in &words {
            for b in &words {
                assert_eq!(gs(a).cmp(&gs(b)), a.cmp(b), "{a:?} cmp {b:?}");
            }
        }
    }

    #[test]
    fn partial_cmp_agrees_with_cmp() {
        let words = boundary_strings();
        for a in &words {
            for b in &words {
                let (x, y) = (gs(a), gs(b));
                assert_eq!(x.partial_cmp(&y), Some(x.cmp(&y)), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn ordering_is_antisymmetric() {
        let words = boundary_strings();
        for a in &words {
            for b in &words {
                let (x, y) = (gs(a), gs(b));
                assert_eq!(x.cmp(&y), y.cmp(&x).reverse(), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn small_prefix_of_large_is_less() {
        assert_eq!(gs("ab").cmp(&gs("abcdefghijklmnop")), Ordering::Less);
        assert_eq!(gs("abcdefghijklmnop").cmp(&gs("ab")), Ordering::Greater);
    }

    #[test]
    fn small_vs_large_decided_in_inline_bytes() {
        // Same prefix, differ at byte 6, which is in the inline section of the small one.
        assert_eq!(
            gs("abcdefz").cmp(&gs("abcdefghijklmnop")),
            Ordering::Greater
        );
        assert_eq!(gs("abcdefghijklmnop").cmp(&gs("abcdefz")), Ordering::Less);
    }

    #[test]
    fn large_vs_large_decided_in_last_byte() {
        assert_eq!(
            gs("abcdefghijklmnoX").cmp(&gs("abcdefghijklmnoY")),
            Ordering::Less
        );
    }

    #[test]
    fn large_vs_large_shorter_is_less() {
        assert_eq!(
            gs("abcdefghijklmn").cmp(&gs("abcdefghijklmno")),
            Ordering::Less
        );
    }

    #[test]
    fn inline_vs_inline_decided_past_prefix() {
        assert_eq!(gs("abcdefgh").cmp(&gs("abcdefgi")), Ordering::Less);
        assert_eq!(gs("abcde").cmp(&gs("abcdef")), Ordering::Less);
    }

    #[test]
    fn trailing_nul_orders_after() {
        assert_eq!(gs("a").cmp(&gs("a\0")), Ordering::Less);
        assert_eq!(gs("a\0").cmp(&gs("a")), Ordering::Greater);
    }

    #[test]
    fn sorting_matches_str_sorting() {
        let mut expected = boundary_strings();
        let mut actual: Vec<GermanString> = expected.iter().rev().map(|s| gs(s)).collect();
        expected.sort();
        actual.sort();
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(
                a.partial_cmp(e).unwrap(),
                Ordering::Equal,
                "position {i}: expected {e:?}"
            );
        }
    }

    #[test]
    fn compare_str_matches_str_for_all_pairs() {
        let words = boundary_strings();
        for a in &words {
            for b in &words {
                assert_eq!(
                    gs(a).partial_cmp(b).unwrap(),
                    a.cmp(b),
                    "{a:?} compare_str {b:?}"
                );
            }
        }
    }

    #[test]
    fn round_trips_through_string() {
        for s in boundary_strings() {
            let back: String = gs(s).try_into().unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn round_trips_multibyte_utf8() {
        // Multi-byte characters straddling the prefix (byte 4) and inline (byte 12) boundaries.
        for s in [
            "héllo",
            "abcé",
            "abcdefghijké",
            "日本語テキスト",
            "🦀🦀🦀🦀",
        ] {
            let back: String = gs(s).try_into().unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn drops_many_large_strings() {
        for i in 0..1_000 {
            let s = format!("a reasonably long string number {i}");
            let g = gs(&s);
            assert_eq!(g.partial_cmp(&s).unwrap(), Ordering::Equal);
        }
    }

    #[test]
    fn large_string_outlives_source() {
        let g = {
            let source = String::from("this string is dropped before use");
            GermanString::from(source)
        };
        assert_eq!(
            g.partial_cmp("this string is dropped before use").unwrap(),
            Ordering::Equal
        );
    }

    #[test]
    fn heap_reads_stay_in_bounds() {
        // Allocations whose tail lengths are just over the inline limit.
        for len in [
            PREFIX_SIZE + 1,
            SMALL_STRING_SIZE + 1,
            SMALL_STRING_SIZE + 2,
            64,
        ] {
            let s = "x".repeat(len);
            let g = gs(&s);
            assert_eq!(g[len - 1], b'x');
            assert_eq!(g.partial_cmp(&s).unwrap(), Ordering::Equal);
        }
    }
    #[test]
    fn prefix_and_inline_are_contiguous() {
        use std::mem::offset_of;
        assert_eq!(
            offset_of!(GermanString, variant),
            offset_of!(GermanString, prefix) + PREFIX_SIZE
        );
    }

    #[test]
    fn as_bytes_matches_source() {
        for s in all_strings() {
            assert_eq!(gs(s).as_bytes(), s.as_bytes(), "{s:?}");
        }
    }

    #[test]
    fn as_str_matches_source() {
        for s in all_strings() {
            assert_eq!(gs(s).as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn as_ref_str_matches_source() {
        for s in all_strings() {
            let g = gs(s);
            let r: &str = AsRef::<str>::as_ref(&g);
            assert_eq!(r, s, "{s:?}");
        }
    }

    #[test]
    fn display_matches_source() {
        for s in all_strings() {
            assert_eq!(gs(s).to_string(), s, "{s:?}");
        }
    }

    #[test]
    fn debug_shows_internal_layout() {
        for s in all_strings() {
            let out = format!("{:?}", gs(s));
            assert!(out.starts_with("GermanString"), "{out}");
            assert!(out.contains(&format!("size: {}", s.len())), "{out}");
        }
    }

    #[test]
    fn as_string_round_trips() {
        for s in all_strings() {
            let g = gs(s);
            assert_eq!(g.as_string(), s, "{s:?}");
            // Borrowing conversion must leave the original intact.
            assert_eq!(g.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn new_from_str_matches_new_from_string() {
        for s in all_strings() {
            assert!(
                GermanString::new_from_str(s) == GermanString::new_from_string(s.to_string()),
                "{s:?}"
            );
        }
    }

    fn hash_of<T: std::hash::Hash + ?Sized>(value: &T) -> u64 {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn hash_matches_string() {
        for s in all_strings() {
            assert_eq!(hash_of(&gs(s)), hash_of(&s.to_string()), "{s:?}");
        }
    }

    #[test]
    fn hash_matches_str() {
        for s in all_strings() {
            assert_eq!(hash_of(&gs(s)), hash_of(s), "{s:?}");
        }
    }

    #[test]
    fn hash_is_independent_of_constrctor() {
        for s in all_strings() {
            assert_eq!(
                hash_of(&GermanString::new_from_str(s)),
                hash_of(&GermanString::new_from_string(s.to_string())),
                "{s:?}"
            );
        }
    }

    #[test]
    fn equal_values_hash_equal_across_separate_allocations() {
        for s in all_strings() {
            assert_eq!(hash_of(&gs(s)), hash_of(&gs(s)), "{s:?}");
        }
    }

    /// Hashing several strings in sequence must match String too, so that splitting the same
    /// bytes at a different point gives a different hash.
    #[test]
    fn hash_of_composites_matches_string() {
        let strings = all_strings();
        for a in &strings {
            for b in &strings {
                assert_eq!(
                    hash_of(&(gs(a), gs(b))),
                    hash_of(&(a.to_string(), b.to_string())),
                    "{a:?} {b:?}"
                );
            }
        }
        assert_ne!(hash_of(&(gs("ab"), gs("c"))), hash_of(&(gs("a"), gs("bc"))));

        let german: Vec<GermanString> = strings.iter().map(|s| gs(s)).collect();
        let std: Vec<String> = strings.iter().map(|s| s.to_string()).collect();
        assert_eq!(hash_of(&german), hash_of(&std));
    }
}
