# kompakt
This crate is an implementation of whats often called a German String. The idea came from the
following paper out of TU Munich: [Umbra: A Disk-Based System with In-Memory Performance](https://www.cidrdb.org/cidr2020/papers/p29-neumann-cidr20.pdf)
(Neumann and Freitag, CIDR 2020).

compakt is the german word for compact.

The idea is that strings are often rather small and they do not change often. [String] from the
standard library is designed from growable strings, and just wraps a [Vec<u8>].

Given the property of immutability, and that most strings are small, a string can be
represented in 16 bytes, and only needs to store data indirect on the heap when needed.

This representation stores the size as a [u32], a 4 byte prefix, and either an inline 8 byte
array, or a 8 byte pointer to the heap. This compact representation can store strings whose
underlying bytes are less than or equal to length can be stored on the stack.

An allocation of [GermanString] can be stored largely contiguously.

Ordering also becomes extremely cheap in many cases, especially when strings do not share a
prefix.

[GermanString] includes common iterator implementations and opt in serde with the "serde"
feature.

For long strings that need mutation, this is not the data structure. But for the many string use case in modern applications, this data structure significantly lowers the overhead of strings.

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
| short distinct  |     58.99 |    **14.96** |
| short shared    |     53.37 |    **18.70** |
| medium distinct |     40.29 |    **15.24** |
| medium shared   | **40.17** |        47.72 |
| long distinct   |     30.32 |    **15.20** |
| long shared     | **30.23** |        45.86 |

### Ordering (`cmp`)

| dataset         |   String | GermanString |
|-----------------|---------:|-------------:|
| short distinct  |     3.41 |     **1.28** |
| short shared    |     4.01 |     **1.60** |
| medium distinct |     2.51 |     **1.28** |
| medium shared   | **2.52** |         4.59 |
| long distinct   |     2.14 |     **1.28** |
| long shared     | **2.17** |         3.81 |

### Construction (from `&str`)

| dataset         |    String | GermanString |
|-----------------|----------:|-------------:|
| short distinct  |     23.26 |     **4.01** |
| short shared    |     28.35 |     **3.81** |
| medium distinct | **31.51** |        32.93 |
| medium shared   |     36.23 |    **33.09** |
| long distinct   |     48.10 |    **36.16** |
| long shared     |     42.28 |    **36.29** |

### Equality (`==`)

| dataset                  |   String | GermanString |
|--------------------------|---------:|-------------:|
| short distinct, equal    |     5.78 |     **1.28** |
| short distinct, unequal  | **1.01** |         1.28 |
| short shared, equal      |     5.16 |     **1.28** |
| short shared, unequal    | **1.17** |         1.96 |
| medium distinct, equal   | **3.54** |         4.18 |
| medium distinct, unequal | **1.07** |         1.28 |
| medium shared, equal     | **3.63** |         4.23 |
| medium shared, unequal   | **1.13** |         1.79 |
| long distinct, equal     |     6.92 |     **5.21** |
| long distinct, unequal   | **0.98** |         1.28 |
| long shared, equal       |     6.54 |     **5.35** |
| long shared, unequal     | **0.75** |         1.30 |

### Borrowing as `&str`

`GermanString` and `String` both take about 0.33 ns across every dataset.

### Summary

- Sorting and ordering are 2x to 4x faster than `String` whenever the first 4 bytes differ.
- Short strings construct about 6x faster than `String`, since nothing is allocated, and equal
  short strings compare about 4x faster.
- Medium and long strings need a heap allocation either way, so construction costs about the
  same as `String`.
- When every string shares its first 4 bytes, ordering heap allocated strings is slower than
  `String`, since the prefix check is wasted work before the same pointer chase.
- Comparing unequal strings for equality is slightly slower than `String`.

