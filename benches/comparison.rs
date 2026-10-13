//! Compares GermanString against String.
//!
//! Run with `cargo bench --bench comparison`, or filter, e.g. `cargo bench -- cmp/medium`.
//!
//! The datasets are chosen around GermanString's inline capacity of 12 bytes:
//!
//! | dataset | lengths  | GermanString |
//! |---------|----------|--------------|
//! | short   | 1..=12   | inline       |
//! | medium  | 13..=23  | heap         |
//! | long    | 32..=128 | heap         |
//!
//! Each dataset comes in two flavours: `distinct` strings mostly differ in their first 4 bytes
//! (comparisons resolved by the prefix), `shared` strings all start with the same 4 bytes
//! (comparisons must look past the prefix).

use std::cmp::Ordering;
use std::hint::black_box;

use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, criterion_group, criterion_main,
};
use kompakt::GermanString;

const N: usize = 1_000;

/// The operations every benchmarked string type needs.
trait BenchStr: Ord + Eq {
    const NAME: &'static str;
    fn make(s: &str) -> Self;
    fn view(&self) -> &str;
}

impl BenchStr for String {
    const NAME: &'static str = "String";
    fn make(s: &str) -> Self {
        s.to_owned()
    }
    fn view(&self) -> &str {
        self.as_str()
    }
}

impl BenchStr for GermanString {
    const NAME: &'static str = "GermanString";
    fn make(s: &str) -> Self {
        GermanString::new_from_str(s).unwrap()
    }
    fn view(&self) -> &str {
        self.as_str()
    }
}

// ---------------------------------------------------------------------------
// Data generation (deterministic, no rand dependency)
// ---------------------------------------------------------------------------

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() as usize) % (hi - lo + 1)
    }

    fn ascii(&mut self) -> char {
        (b'a' + (self.next() % 26) as u8) as char
    }
}

struct Dataset {
    name: String,
    strings: Vec<String>,
}

fn dataset(size: &str, lo: usize, hi: usize, shared_prefix: bool, seed: u64) -> Dataset {
    let mut rng = Lcg(seed);
    let strings = (0..N)
        .map(|_| {
            let len = rng.range(lo, hi);
            let mut s = String::with_capacity(len);
            if shared_prefix {
                s.push_str("user");
            }
            while s.len() < len {
                s.push(rng.ascii());
            }
            s.truncate(len);
            s
        })
        .collect();
    let flavour = if shared_prefix { "shared" } else { "distinct" };
    Dataset {
        name: format!("{size}/{flavour}"),
        strings,
    }
}

fn datasets() -> Vec<Dataset> {
    let mut v = Vec::new();
    for (i, (size, lo, hi)) in [("short", 1, 12), ("medium", 13, 23), ("long", 32, 128)]
        .into_iter()
        .enumerate()
    {
        // Short strings under 4 bytes can't carry the shared prefix, so start them at 5.
        let shared_lo = lo.max(5);
        v.push(dataset(size, lo, hi, false, 0x5eed + i as u64));
        v.push(dataset(size, shared_lo, hi, true, 0xbeef + i as u64));
    }
    v
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_construct<T: BenchStr>(group: &mut BenchmarkGroup<WallTime>, data: &[Dataset]) {
    for d in data {
        group.bench_with_input(
            BenchmarkId::new(T::NAME, &d.name),
            &d.strings,
            |b, strings| {
                b.iter(|| {
                    for s in strings {
                        black_box(T::make(black_box(s)));
                    }
                });
            },
        );
    }
}

fn bench_eq<T: BenchStr>(group: &mut BenchmarkGroup<WallTime>, data: &[Dataset]) {
    for d in data {
        // Two independently built copies, so equal values never share an allocation.
        let a: Vec<T> = d.strings.iter().map(|s| T::make(s)).collect();
        let b_same: Vec<T> = d.strings.iter().map(|s| T::make(s)).collect();
        let b_diff: Vec<T> = d.strings.iter().rev().map(|s| T::make(s)).collect();

        group.bench_function(
            BenchmarkId::new(T::NAME, format!("{}/equal", d.name)),
            |b| {
                b.iter(|| {
                    let mut n = 0;
                    for (x, y) in a.iter().zip(&b_same) {
                        n += (black_box(x) == black_box(y)) as usize;
                    }
                    n
                });
            },
        );
        group.bench_function(
            BenchmarkId::new(T::NAME, format!("{}/unequal", d.name)),
            |b| {
                b.iter(|| {
                    let mut n = 0;
                    for (x, y) in a.iter().zip(&b_diff) {
                        n += (black_box(x) == black_box(y)) as usize;
                    }
                    n
                });
            },
        );
    }
}

fn bench_cmp<T: BenchStr>(group: &mut BenchmarkGroup<WallTime>, data: &[Dataset]) {
    for d in data {
        let v: Vec<T> = d.strings.iter().map(|s| T::make(s)).collect();
        group.bench_function(BenchmarkId::new(T::NAME, &d.name), |b| {
            b.iter(|| {
                let mut less = 0;
                for pair in v.windows(2) {
                    less +=
                        (black_box(&pair[0]).cmp(black_box(&pair[1])) == Ordering::Less) as usize;
                }
                less
            });
        });
    }
}

fn bench_sort<T: BenchStr>(group: &mut BenchmarkGroup<WallTime>, data: &[Dataset]) {
    for d in data {
        group.bench_function(BenchmarkId::new(T::NAME, &d.name), |b| {
            // Building the vector is setup and not timed.
            b.iter_batched(
                || d.strings.iter().map(|s| T::make(s)).collect::<Vec<T>>(),
                |mut v| {
                    v.sort_unstable();
                    v
                },
                BatchSize::SmallInput,
            );
        });
    }
}

fn bench_as_str<T: BenchStr>(group: &mut BenchmarkGroup<WallTime>, data: &[Dataset]) {
    for d in data {
        let v: Vec<T> = d.strings.iter().map(|s| T::make(s)).collect();
        group.bench_function(BenchmarkId::new(T::NAME, &d.name), |b| {
            b.iter(|| {
                let mut total = 0;
                for s in &v {
                    total += black_box(s).view().len();
                }
                total
            });
        });
    }
}

/// Opens one group per operation and runs every type inside it, so reports line up.
macro_rules! group {
    ($c:expr, $name:literal, $f:ident, $data:expr) => {{
        let mut group = $c.benchmark_group($name);
        $f::<String>(&mut group, $data);
        $f::<GermanString>(&mut group, $data);
        group.finish();
    }};
}

fn comparison(c: &mut Criterion) {
    println!(
        "size_of: String={} GermanString={}",
        size_of::<String>(),
        size_of::<GermanString>(),
    );
    println!(
        "size_of Option: String={} GermanString={}",
        size_of::<Option<String>>(),
        size_of::<Option<GermanString>>(),
    );

    let data = datasets();
    group!(c, "construct", bench_construct, &data);
    group!(c, "eq", bench_eq, &data);
    group!(c, "cmp", bench_cmp, &data);
    group!(c, "sort", bench_sort, &data);
    group!(c, "as_str", bench_as_str, &data);
}

criterion_group!(benches, comparison);
criterion_main!(benches);
