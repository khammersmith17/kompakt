# kompakt
This crate is a lightweight, standalone, implementation of what's often called a German String. The idea came from the
following paper out of TU Munich: [Umbra: A Disk-Based System with In-Memory Performance](https://www.cidrdb.org/cidr2020/papers/p29-neumann-cidr20.pdf)
(Neumann and Freitag, CIDR 2020).

Arrow (`StringView`) has their own implementation, but that does not have a `serde` implementation and is generally designed around batch data processing with Arrow arrays. That is a tremendous implementation for the analytics use case. 

This is designed to be an immutable alternative to `String` where applicable.

`kompakt` is the German word for compact.

The idea is that strings are often rather small and they do not change often. `String` from the
standard library is designed for growable strings, and just wraps a `Vec<u8>`.

Given the property of immutability, and that most strings are small, a string can be
represented in 16 bytes, and only needs to store data indirect on the heap when needed.

There are two variants, small and large. Small strings are strings of 12 bytes or fewer. Both have the same representation for the first 8 bytes. This shared 8 bytes is a u32 size, and 4 byte prefix.

For small strings, an additional 8 bytes is an inline array, storing up to the 8 remaining bytes. If the string is smaller than 12 bytes, all additional space is zero padded. In this case, the entire string is stored inline with no heap allocation, so it can live on the stack, or contiguously in an array.

For large strings, the remaining 8 bytes is a pointer to the entire string on the heap. The prefix is therefore stored twice, once in the 16 byte header and once at the start of the heap allocation. This costs 4 duplicated bytes, but it allows for contiguous views of the string, and more straightforward comparisons. The heap allocation is fixed size and is immutable.

One thing the paper has that this crate does not is a storage class for the out of line data (persistent, transient or temporary), which lets a database avoid copying strings it already owns. A `GermanString` always owns its heap allocation.

An array of small `GermanString`s is stored contiguously. Large strings still point to their own heap allocations.

Ordering also becomes extremely cheap in many cases, especially when strings do not share a
prefix.

`GermanString` includes common iterator implementations and opt in serde with the "serde"
feature.

The size is stored as a u32, so a `GermanString` can hold at most `u32::MAX` bytes (about 4 GiB). Construction is fallible for that reason and returns an error for longer strings.

For long strings that need mutation, this is not the data structure. But for the many string use cases in modern applications, this data structure significantly lowers the overhead of strings. As seen in the benchmarks (run on Apple M1 chip), constructing and comparing short strings is much faster than with standard strings. Longer strings are a trade off: ordering and sorting are still much faster when their prefixes differ, construction costs about the same, and comparing strings that share a prefix or are equal is slower.

Also, on 64 bit targets `GermanString` is 16 bytes and `String` is 24 bytes. `Option<GermanString>` is 24 bytes, the same as `Option<String>`.

## Benchmarks

Compared against `String` using criterion on an Apple M1. Run them with
`cargo bench --bench comparison`.

All numbers are the median time per string in nanoseconds; lower is better.

The datasets are chosen around the inline capacity of 12 bytes:

| dataset | length in bytes | GermanString |
|---------|-----------------|--------------|
| short   | 1 to 12         | inline       |
| medium  | 13 to 23        | heap         |
| long    | 32 to 128       | heap         |

Each dataset comes in two flavours. `distinct` strings mostly differ in their first 4 bytes, so
comparisons are decided by the prefix. `shared` strings all start with the same 4 bytes, so
comparisons have to look past the prefix.

### Sorting (`sort_unstable`, per element)

| dataset         |    String | GermanString |
|-----------------|----------:|-------------:|
| short distinct  |     59.68 |    **10.69** |
| short shared    |     54.09 |    **15.33** |
| medium distinct |     40.00 |    **10.12** |
| medium shared   |     40.35 |    **37.52** |
| long distinct   |     30.27 |    **10.22** |
| long shared     | **30.25** |        37.20 |

### Ordering (`cmp`)

| dataset         |   String | GermanString |
|-----------------|---------:|-------------:|
| short distinct  |     3.17 |     **0.76** |
| short shared    |     3.63 |     **1.31** |
| medium distinct |     2.33 |     **0.76** |
| medium shared   | **2.47** |         4.32 |
| long distinct   |     2.07 |     **0.80** |
| long shared     | **2.11** |         3.46 |

### Construction (from `&str`)

| dataset         |    String | GermanString |
|-----------------|----------:|-------------:|
| short distinct  |     22.54 |     **2.45** |
| short shared    |     22.55 |     **2.53** |
| medium distinct | **23.92** |        25.60 |
| medium shared   | **23.94** |        25.90 |
| long distinct   | **26.74** |        28.60 |
| long shared     | **25.62** |        27.57 |

### Equality (`==`)

| dataset                  |   String | GermanString |
|--------------------------|---------:|-------------:|
| short distinct, equal    |     4.13 |     **1.14** |
| short distinct, unequal  |     0.84 |     **0.80** |
| short shared, equal      |     3.93 |     **1.27** |
| short shared, unequal    |     1.00 |     **0.86** |
| medium distinct, equal   | **2.64** |         3.83 |
| medium distinct, unequal | **0.71** |         0.76 |
| medium shared, equal     | **2.55** |         3.88 |
| medium shared, unequal   | **0.82** |         0.93 |
| long distinct, equal     | **4.78** |         5.60 |
| long distinct, unequal   | **0.69** |         0.85 |
| long shared, equal       | **4.76** |         5.61 |
| long shared, unequal     | **0.57** |         0.81 |

### Borrowing as `&str`

`GermanString` and `String` both take about 0.35 ns across every dataset.

### Summary

- Sorting and ordering are about 2.5x to 5.5x faster than `String` whenever the first 4 bytes
  differ.
- Short strings construct about 9x faster than `String`, since nothing is allocated, and equal
  short strings compare about 3x faster.
- Medium and long strings need a heap allocation either way, so construction costs about the
  same as `String` (about 7% slower).
- When every string shares its first 4 bytes, comparing heap allocated strings is about 1.7x
  slower than `String`, since the prefix check is wasted work before the same pointer chase.
  Sorting medium strings costs about the same as `String`; sorting long ones is about 1.2x
  slower.
- Equality on heap allocated strings is slower than `String`, by 1.2x to 1.5x when the strings
  are equal.
- Borrowing as `&str` costs the same as `String`.

