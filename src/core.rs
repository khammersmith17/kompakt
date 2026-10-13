use crate::error::GermanStringLengthError;
use std::{
    borrow::Borrow,
    cmp::Ordering,
    fmt,
    ops::{Deref, Range},
    ptr::{self, NonNull},
    str::FromStr,
};

const PREFIX_SIZE: usize = 4_usize;
const INLINE_SIZE: usize = 8_usize;
const SMALL_STRING_SIZE: usize = PREFIX_SIZE + INLINE_SIZE;

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

impl Default for GermanString {
    fn default() -> GermanString {
        GermanString::make_default()
    }
}

impl Clone for GermanString {
    fn clone(&self) -> Self {
        let size = self.size;
        let prefix = self.prefix;

        let variant = self.clone_variant();

        Self {
            size,
            prefix,
            variant,
        }
    }
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

impl Deref for GermanString {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<&str> for GermanString {
    type Error = GermanStringLengthError;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        GermanString::new_from_str(s)
    }
}

impl FromStr for GermanString {
    type Err = GermanStringLengthError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GermanString::new_from_str(s)
    }
}

impl TryFrom<String> for GermanString {
    type Error = GermanStringLengthError;

    #[inline]
    fn try_from(s: String) -> Result<Self, Self::Error> {
        GermanString::new_from_string(s)
    }
}

impl Borrow<str> for GermanString {
    fn borrow(&self) -> &str {
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

impl PartialEq<GermanString> for str {
    fn eq(&self, other: &GermanString) -> bool {
        other.as_str().eq(self)
    }
}

impl PartialEq<GermanString> for &str {
    fn eq(&self, other: &GermanString) -> bool {
        other.as_str().eq(*self)
    }
}

impl PartialEq<GermanString> for String {
    fn eq(&self, other: &GermanString) -> bool {
        self.as_str().eq(other.as_str())
    }
}

impl Eq for GermanString {}

impl Ord for GermanString {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        // First try prefix order.
        let prefix_ord = self.cmp_prefix(other);

        if !matches!(prefix_ord, Ordering::Equal) {
            return prefix_ord;
        }

        // Determine where the rest lives, and use built in u8 ordering.
        match (self.variant_type(), other.variant_type()) {
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
                mine.cmp(theirs)
            }
        }
    }
}

impl PartialOrd for GermanString {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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

impl PartialOrd<GermanString> for str {
    /// Delegate to str ordering.
    #[inline]
    fn partial_cmp(&self, other: &GermanString) -> Option<Ordering> {
        Some(other.as_str().partial_cmp(self)?.reverse())
    }
}

impl PartialOrd<GermanString> for &str {
    /// Delegate to str ordering.
    #[inline]
    fn partial_cmp(&self, other: &GermanString) -> Option<Ordering> {
        Some(other.as_str().partial_cmp(*self)?.reverse())
    }
}

impl PartialOrd<GermanString> for String {
    /// Delegate to str ordering.
    #[inline]
    fn partial_cmp(&self, other: &GermanString) -> Option<Ordering> {
        self.as_str().partial_cmp(other.as_str())
    }
}

impl GermanString {
    /// Build a new [GermanString] from a [&str].
    /// The only fallible condition is when the [&str] is larger than u32::MAX in size.
    #[inline]
    pub fn new_from_str(s: &str) -> Result<GermanString, GermanStringLengthError> {
        GermanString::new_from_str_fallible(s)
    }

    /// Build a new [GermanString] from a [String]. This is most performant when the string is
    /// large.
    #[inline]
    pub fn new_from_string(s: String) -> Result<GermanString, GermanStringLengthError> {
        GermanString::new_fallible(s)
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
    #[inline]
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

    pub fn is_empty(&self) -> bool {
        self.len() == 0_usize
    }
}

/// Private methods.
impl GermanString {
    #[inline]
    fn clone_variant(&self) -> Variant {
        match self.variant_type() {
            GermanStringType::Small => {
                // Clone the 8 byte inline portion.
                let small = unsafe { self.variant.small };
                Variant { small }
            }
            GermanStringType::Large => self.clone_heap_allocation(),
        }
    }

    #[inline]
    fn clone_heap_allocation(&self) -> Variant {
        debug_assert_eq!(self.variant_type(), GermanStringType::Large);
        // Copy the allocation into a new Box. Box::from on a reference clones.
        let copy: Box<[u8]> = Box::from(self.as_ref());
        let rest_ptr = Box::into_raw(copy) as *mut u8;

        // SAFETY: `into_raw` consumes the Box.
        let rest = unsafe { NonNull::new_unchecked(rest_ptr) };
        Variant { large: rest }
    }

    // Default to an empty string.
    fn make_default() -> GermanString {
        let size = 0_u32;
        let prefix = [0_u8; 4];
        let inline = [0_u8; 8];

        GermanString {
            size,
            prefix,
            variant: Variant { small: inline },
        }
    }

    /// Get a slice to the string. Serves both references, and sliced references in the iterators.
    #[inline]
    pub(crate) fn as_str_slice(&self, rng: Range<usize>) -> &str {
        // SAFETY: It is impossible to construct a non-utf8 encoded string.
        unsafe { std::str::from_utf8_unchecked(self.as_ref().get_unchecked(rng)) }
    }

    #[inline]
    pub(crate) fn size(&self) -> usize {
        self.size as usize
    }

    #[inline]
    pub(crate) fn new_fallible(s: String) -> Result<GermanString, GermanStringLengthError> {
        if s.len() > u32::MAX as usize {
            return Err(GermanStringLengthError(s.len() as u64));
        }
        if s.len() <= SMALL_STRING_SIZE {
            Ok(GermanString::new_small(s))
        } else {
            Ok(GermanString::new_large(s))
        }
    }

    #[inline]
    pub(crate) fn new_from_str_fallible(s: &str) -> Result<GermanString, GermanStringLengthError> {
        if s.len() > u32::MAX as usize {
            return Err(GermanStringLengthError(s.len() as u64));
        }
        if s.len() <= SMALL_STRING_SIZE {
            Ok(GermanString::new_small_from_str(s))
        } else {
            Ok(GermanString::new_large_from_str(s))
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
                let lo = u64::from(head) | (u64::from(tail) << ((size - 4) * 8));
                (lo, 0)
            } else if size >= 2 {
                // u16 reads
                // No inline portion here.
                let head = u16::from_le(ptr::read_unaligned(src as *const u16));
                let tail = u16::from_le(ptr::read_unaligned(src.add(size - 2) as *const u16));

                let lo = u64::from(head) | (u64::from(tail) << ((size - 2) * 8));
                (lo, 0)
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
    fn into_string_large(self) -> String {
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
    fn into_string_small(self) -> String {
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

impl From<GermanString> for String {
    // Into given it is impossible to construct a non utf8 string here.
    fn from(gs: GermanString) -> String {
        match gs.variant_type() {
            GermanStringType::Small => gs.into_string_small(),
            GermanStringType::Large => gs.into_string_large(),
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
    fn get_char_ascii() {
        let s = gs("hello, german string");
        for (i, c) in "hello, german string".chars().enumerate() {
            assert_eq!(s.get(i), Some(c));
        }
        assert_eq!(s.get(100), None);
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
            GermanString::try_from(source).unwrap()
        };
        assert_eq!(
            g.partial_cmp("this string is dropped before use").unwrap(),
            Ordering::Equal
        );
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

    // ---------------------------------------------------------------------------
    // Default
    // ---------------------------------------------------------------------------

    #[test]
    fn default_is_the_empty_string() {
        let d = GermanString::default();
        assert_eq!(d.as_str(), "");
        assert_eq!(d.len(), 0);
        assert_eq!(d.variant_type(), GermanStringType::Small);
        assert_eq!(d.as_str(), String::default());
    }

    /// The default must be indistinguishable from an empty string built any other way.
    #[test]
    fn default_matches_constructed_empty_string() {
        let d = GermanString::default();
        let empty = gs("");
        assert!(d == empty);
        assert_eq!(d.cmp(&empty), Ordering::Equal);
        assert_eq!(hash_of(&d), hash_of(&empty));
        assert_eq!(hash_of(&d), hash_of(""));
    }

    // ---------------------------------------------------------------------------
    // TryFrom and FromStr
    // ---------------------------------------------------------------------------

    #[test]
    fn try_from_str_matches_source() {
        for s in all_strings() {
            let g = GermanString::try_from(s).unwrap();
            assert_eq!(g.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn try_from_string_matches_source() {
        for s in all_strings() {
            let g = GermanString::try_from(s.to_string()).unwrap();
            assert_eq!(g.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn from_str_matches_source() {
        use std::str::FromStr;

        for s in all_strings() {
            assert_eq!(GermanString::from_str(s).unwrap().as_str(), s, "{s:?}");

            let parsed: GermanString = s.parse().unwrap();
            assert_eq!(parsed.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn every_conversion_builds_the_same_value() {
        for s in all_strings() {
            let expected = GermanString::new_from_str(s).unwrap();
            let from_str: GermanString = s.try_into().unwrap();
            let from_string: GermanString = s.to_string().try_into().unwrap();
            let parsed: GermanString = s.parse().unwrap();

            for (name, g) in [
                ("TryFrom<&str>", from_str),
                ("TryFrom<String>", from_string),
                ("FromStr", parsed),
            ] {
                assert!(g == expected, "{name} {s:?}");
                assert_eq!(g.variant_type(), expected.variant_type(), "{name} {s:?}");
                assert_eq!(hash_of(&g), hash_of(&expected), "{name} {s:?}");
            }
        }
    }

    /// The fallible conversions have to be usable with `?` in functions returning a boxed error.
    #[test]
    fn conversion_errors_convert_to_boxed_errors() {
        fn convert(s: &str) -> Result<GermanString, Box<dyn std::error::Error>> {
            let from_str = GermanString::try_from(s)?;
            let parsed: GermanString = s.parse()?;
            assert!(from_str == parsed);
            Ok(GermanString::try_from(s.to_string())?)
        }

        assert_eq!(
            convert("hello, german string").unwrap().as_str(),
            "hello, german string"
        );
    }

    #[test]
    fn length_error_reports_the_length() {
        use crate::GermanStringLengthError;
        use std::error::Error;

        let too_long = u64::from(u32::MAX) + 1;
        let err = GermanStringLengthError(too_long);
        assert!(err.to_string().contains(&too_long.to_string()), "{err}");
        assert!(format!("{err:?}").contains(&too_long.to_string()));
        assert!(err.source().is_none());
        assert!(err == GermanStringLengthError(too_long));
        assert!(err != GermanStringLengthError(too_long + 1));
    }

    // ---------------------------------------------------------------------------
    // Borrow<str>
    // ---------------------------------------------------------------------------

    #[test]
    fn borrow_matches_source() {
        use std::borrow::Borrow;

        for s in all_strings() {
            let g = gs(s);
            let borrowed: &str = g.borrow();
            assert_eq!(borrowed, s, "{s:?}");
        }
    }

    /// Borrow<str> promises the borrowed form hashes and compares like the owned one, which is
    /// what lets a map keyed by GermanString be queried with a &str.
    #[test]
    fn hash_map_is_queryable_by_str() {
        use std::collections::HashMap;

        let strings = all_strings();
        let map: HashMap<GermanString, usize> = strings
            .iter()
            .enumerate()
            .map(|(i, s)| (gs(s), i))
            .collect();
        assert_eq!(map.len(), strings.len());

        for (i, s) in strings.iter().enumerate() {
            assert_eq!(map.get(*s), Some(&i), "{s:?}");
            assert!(map.contains_key(*s), "{s:?}");
            assert_eq!(map.get(s.to_string().as_str()), Some(&i), "{s:?}");
        }
        assert_eq!(map.get("not a key"), None);
        assert_eq!(map.get("a string too long to be stored inline"), None);
    }

    #[test]
    fn hash_set_is_queryable_by_str() {
        use std::collections::HashSet;

        let strings = all_strings();
        let mut set: HashSet<GermanString> = strings.iter().map(|s| gs(s)).collect();
        for s in &strings {
            assert!(set.contains(*s), "{s:?}");
        }
        assert!(!set.contains("not a key"));

        assert!(set.remove("abcdefghijklm"));
        assert!(!set.contains("abcdefghijklm"));
    }

    /// The ordered collections rely on Ord agreeing with str rather than Hash.
    #[test]
    fn btree_map_is_queryable_by_str() {
        use std::collections::BTreeMap;
        use std::ops::Bound;

        let strings = all_strings();
        let map: BTreeMap<GermanString, usize> = strings
            .iter()
            .enumerate()
            .map(|(i, s)| (gs(s), i))
            .collect();

        for (i, s) in strings.iter().enumerate() {
            assert_eq!(map.get(*s), Some(&i), "{s:?}");
        }
        assert_eq!(map.get("not a key"), None);

        let mut expected = strings.clone();
        expected.sort_unstable();
        let keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, expected);

        let from_b: Vec<&str> = map
            .range::<str, _>((Bound::Included("b"), Bound::Unbounded))
            .map(|(k, _)| k.as_str())
            .collect();
        let expected_from_b: Vec<&str> = expected.iter().copied().filter(|s| *s >= "b").collect();
        assert_eq!(from_b, expected_from_b);
    }

    // ---------------------------------------------------------------------------
    // Deref<Target = str>
    // ---------------------------------------------------------------------------

    #[test]
    fn deref_matches_source() {
        for s in all_strings() {
            let g = gs(s);
            let derefed: &str = &g;
            assert_eq!(derefed, s, "{s:?}");
            assert_eq!(&*g, s, "{s:?}");
        }
    }

    #[test]
    fn deref_coerces_at_call_sites() {
        fn byte_len(s: &str) -> usize {
            s.len()
        }

        for s in all_strings() {
            assert_eq!(byte_len(&gs(s)), s.len(), "{s:?}");
        }
    }

    #[test]
    fn str_methods_are_available_through_deref() {
        for s in all_strings() {
            let g = gs(s);
            assert_eq!(g.is_empty(), s.is_empty(), "{s:?}");
            assert_eq!(g.chars().count(), s.chars().count(), "{s:?}");
            assert_eq!(g.bytes().collect::<Vec<_>>(), s.as_bytes(), "{s:?}");
            assert_eq!(g.starts_with("abc"), s.starts_with("abc"), "{s:?}");
            assert_eq!(g.contains('é'), s.contains('é'), "{s:?}");
            assert_eq!(g.to_uppercase(), s.to_uppercase(), "{s:?}");
            assert_eq!(g.trim(), s.trim(), "{s:?}");
            assert_eq!(g.find('d'), s.find('d'), "{s:?}");
        }
    }

    /// `len` and `get` exist both on GermanString and on str. The inherent ones win, so they
    /// have to agree with what the str ones would have returned for the same byte offset.
    #[test]
    fn inherent_methods_agree_with_the_str_methods_they_shadow() {
        for s in all_strings() {
            let g = gs(s);
            assert_eq!(g.len(), s.len(), "{s:?}");
            for idx in 0..=s.len() + 1 {
                let expected = s.get(idx..).and_then(|rest| rest.chars().next());
                assert_eq!(g.get(idx), expected, "{s:?} at {idx}");
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Clone
    // ---------------------------------------------------------------------------

    #[test]
    fn clone_matches_original() {
        for s in all_strings() {
            let g = gs(s);
            let c = g.clone();
            assert_eq!(c.as_str(), s, "{s:?}");
            assert_eq!(c.len(), s.len(), "{s:?}");
            assert!(c == g, "{s:?}");
            assert_eq!(c.cmp(&g), Ordering::Equal, "{s:?}");
            assert_eq!(c.variant_type(), g.variant_type(), "{s:?}");
            assert_eq!(hash_of(&c), hash_of(&g), "{s:?}");
        }
    }

    /// Both ways of building a large string allocate differently (copied from a &str, or
    /// adopted from a String), and a clone of either has to be freed the same way.
    #[test]
    fn clone_matches_original_for_both_constructors() {
        for s in all_strings() {
            let from_str = GermanString::new_from_str(s).unwrap();
            let from_string = GermanString::new_from_string(s.to_string()).unwrap();
            assert_eq!(from_str.clone().as_str(), s, "{s:?}");
            assert_eq!(from_string.clone().as_str(), s, "{s:?}");
        }
    }

    /// A clone must never share the original's heap allocation.
    #[test]
    fn clone_of_large_string_has_its_own_allocation() {
        for s in all_strings() {
            let g = gs(s);
            let c = g.clone();
            assert_ne!(g.as_str().as_ptr(), c.as_str().as_ptr(), "{s:?}");
        }
    }

    #[test]
    fn clone_outlives_original() {
        for s in all_strings() {
            let g = gs(s);
            let c = g.clone();
            drop(g);
            assert_eq!(c.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn original_outlives_clone() {
        for s in all_strings() {
            let g = gs(s);
            let c = g.clone();
            drop(c);
            assert_eq!(g.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn clone_of_clone_matches_original() {
        for s in all_strings() {
            let g = gs(s);
            let c2 = {
                let c1 = g.clone();
                c1.clone()
            };
            drop(g);
            assert_eq!(c2.as_str(), s, "{s:?}");
        }
    }

    /// Many clones of one string, dropped in an order unrelated to the order they were made in.
    #[test]
    fn many_clones_drop_independently() {
        for s in all_strings() {
            let g = gs(s);
            let mut clones: Vec<GermanString> = (0..8).map(|_| g.clone()).collect();
            drop(g);

            for idx in [5, 0, 3, 0, 1] {
                drop(clones.remove(idx));
                for c in &clones {
                    assert_eq!(c.as_str(), s, "{s:?}");
                }
            }
        }
    }

    /// Converting into a String hands the heap allocation over without copying, which only
    /// works if a clone's allocation has the same layout as the original's.
    #[test]
    fn clone_converts_into_string() {
        for s in all_strings() {
            let g = gs(s);
            let from_clone = String::from(g.clone());
            assert_eq!(from_clone, s, "{s:?}");
            assert_eq!(g.as_str(), s, "{s:?}");

            let c = g.clone();
            let from_original = String::from(g);
            assert_eq!(from_original, s, "{s:?}");
            assert_eq!(c.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn clone_can_be_consumed_by_the_owning_iterator() {
        for s in all_strings() {
            let g = gs(s);
            let chars: Vec<char> = g.clone().into_iter().collect();
            assert_eq!(chars, s.chars().collect::<Vec<_>>(), "{s:?}");
            assert_eq!(g.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn collections_of_german_strings_clone() {
        let strings = all_strings();
        let original: Vec<GermanString> = strings.iter().map(|s| gs(s)).collect();
        let cloned = original.clone();
        drop(original);

        assert_eq!(cloned.len(), strings.len());
        for (c, s) in cloned.iter().zip(&strings) {
            assert_eq!(c.as_str(), *s);
        }
    }

    #[test]
    fn clone_from_replaces_the_previous_value() {
        let strings = all_strings();
        for from in &strings {
            for to in &strings {
                let mut g = gs(from);
                g.clone_from(&gs(to));
                assert_eq!(g.as_str(), *to, "{from:?} -> {to:?}");
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Comparisons with str, &str and String on the left hand side
    // ---------------------------------------------------------------------------

    #[test]
    fn reverse_equality_matches_str() {
        let strings = all_strings();
        for &a in &strings {
            for &b in &strings {
                let g = gs(b);
                let expected = a == b;

                // str, &str and String on the left, in that order.
                assert_eq!(*a == g, expected, "str {a:?} == {b:?}");
                assert_eq!(a == g, expected, "&str {a:?} == {b:?}");
                assert_eq!(a.to_string() == g, expected, "String {a:?} == {b:?}");

                assert_eq!(*a != g, !expected, "str {a:?} != {b:?}");
                assert_eq!(a != g, !expected, "&str {a:?} != {b:?}");
                assert_eq!(a.to_string() != g, !expected, "String {a:?} != {b:?}");
            }
        }
    }

    #[test]
    fn reverse_partial_cmp_matches_str() {
        let strings = all_strings();
        for &a in &strings {
            for &b in &strings {
                let g = gs(b);
                let expected = Some(a.cmp(b));

                assert_eq!(a[..].partial_cmp(&g), expected, "str {a:?} vs {b:?}");
                // Spelled out, since `a.partial_cmp` would auto-deref to the str impl.
                assert_eq!(<&str as PartialOrd<GermanString>>::partial_cmp(&a, &g), expected, "&str {a:?} vs {b:?}");
                assert_eq!(a.to_string().partial_cmp(&g), expected, "String {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn reverse_comparison_operators_match_str() {
        let strings = all_strings();
        for &a in &strings {
            for &b in &strings {
                let g = gs(b);
                let owned = a.to_string();

                assert_eq!(*a < g, a < b, "str {a:?} < {b:?}");
                assert_eq!(*a <= g, a <= b, "str {a:?} <= {b:?}");
                assert_eq!(*a > g, a > b, "str {a:?} > {b:?}");
                assert_eq!(*a >= g, a >= b, "str {a:?} >= {b:?}");

                assert_eq!(a < g, a < b, "&str {a:?} < {b:?}");
                assert_eq!(a <= g, a <= b, "&str {a:?} <= {b:?}");
                assert_eq!(a > g, a > b, "&str {a:?} > {b:?}");
                assert_eq!(a >= g, a >= b, "&str {a:?} >= {b:?}");

                assert_eq!(owned < g, a < b, "String {a:?} < {b:?}");
                assert_eq!(owned <= g, a <= b, "String {a:?} <= {b:?}");
                assert_eq!(owned > g, a > b, "String {a:?} > {b:?}");
                assert_eq!(owned >= g, a >= b, "String {a:?} >= {b:?}");
            }
        }
    }

    /// Swapping the two sides of a comparison must flip the result and nothing else.
    #[test]
    fn reverse_comparisons_mirror_forward_comparisons() {
        let strings = all_strings();
        for &a in &strings {
            for &b in &strings {
                let g = gs(b);
                let owned = a.to_string();

                assert_eq!(*a == g, g == *a, "str {a:?} {b:?}");
                assert_eq!(a == g, g == a, "&str {a:?} {b:?}");
                assert_eq!(owned == g, g == owned, "String {a:?} {b:?}");

                let forward = |o: Option<Ordering>| o.map(Ordering::reverse);
                assert_eq!(a[..].partial_cmp(&g), forward(g.partial_cmp(&a[..])), "str {a:?} {b:?}");
                assert_eq!(
                    <&str as PartialOrd<GermanString>>::partial_cmp(&a, &g),
                    forward(g.partial_cmp(&a)),
                    "&str {a:?} {b:?}"
                );
                assert_eq!(owned.partial_cmp(&g), forward(g.partial_cmp(&owned)), "String {a:?} {b:?}");
            }
        }
    }

    /// Sorting plain strings against a GermanString pivot only needs the reverse impls.
    #[test]
    fn reverse_ordering_partitions_like_str() {
        let strings = all_strings();
        for &pivot in &strings {
            let g = gs(pivot);
            let below: Vec<&str> = strings.iter().copied().filter(|s| *s < g).collect();
            let expected: Vec<&str> = strings.iter().copied().filter(|s| *s < pivot).collect();
            assert_eq!(below, expected, "pivot {pivot:?}");
        }
    }
}
