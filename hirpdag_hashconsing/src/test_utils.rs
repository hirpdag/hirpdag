#![cfg(test)]

use crate::reference::*;
use crate::table::*;

#[derive(Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct TestData {
    a: i32,
    b: i32,
    c: String,
}

impl TestData {
    pub fn new(a: i32, b: i32, c: String) -> Self {
        Self { a: a, b: b, c: c }
    }
}

pub fn test_interface_impl<R, TS>(tableshared: &TS, data: TestData)
where
    R: Reference<TestData>,
    TS: Table<TestData, R>,
{
    let data_clone = data.clone();
    let x: R = tableshared.get_or_insert(data, |_s| {});
    assert_eq!(R::strong_deref(&x), &data_clone);
    let y = R::strong_clone(&x);
    assert!(R::strong_ptr_eq(&x, &y));
}

pub fn populate_linear<R, TS>(out: &mut Vec<R>, tableshared: &TS, range: std::ops::Range<usize>)
where
    R: Reference<TestData>,
    TS: Table<TestData, R>,
{
    for k in range {
        let data1 = TestData {
            a: k as i32,
            b: 0,
            c: "hello".to_string(),
        };
        out.push(tableshared.get_or_insert(data1, |_s| {}));
    }
}

pub fn assert_match_and_unique<R>(v1: &Vec<R>, v2: &Vec<R>)
where
    R: Reference<TestData>,
{
    assert_eq!(v1.len(), v2.len());
    for v1e in v1.iter().enumerate() {
        for v2e in v2.iter().enumerate() {
            // Corresponding elements should be the same.
            // Non-corresponging elements should be different.
            let expect_equal: bool = v1e.0 == v2e.0;
            let actual_equal: bool = R::strong_ptr_eq(v1e.1, v2e.1);
            assert!(
                actual_equal == expect_equal,
                "\nlhs[{:?}]={:?}\nrhs[{:?}]={:?}\nexpect_equal {:?}\n",
                v1e.0,
                R::strong_deref(v1e.1),
                v2e.0,
                R::strong_deref(v2e.1),
                expect_equal
            );
        }
    }
}

pub fn hashcons_two_copies<R, TS>(tableshared: &TS)
where
    R: Reference<TestData>,
    TS: Table<TestData, R>,
{
    let n = 32usize;
    let mut v1: Vec<R> = vec![];
    let mut v2: Vec<R> = vec![];
    populate_linear(&mut v1, tableshared, 0..n);
    populate_linear(&mut v2, tableshared, 0..n);
    // Examples for n=4...
    // 0,1,2,3
    assert_match_and_unique(&v1, &v2);
    v1.drain(0..n / 2);
    v2.drain(0..n / 2);
    // 2,3
    populate_linear(&mut v1, tableshared, 0..n / 2);
    populate_linear(&mut v2, tableshared, 0..n / 2);
    // 2,3,0,1
    assert_match_and_unique(&v1, &v2);
    v1.drain(0..n / 2);
    v2.drain(0..n / 2);
    // 0,1
    populate_linear(&mut v1, tableshared, n / 2..n);
    populate_linear(&mut v2, tableshared, n / 2..n);
    // 0,1,2,3
    assert_match_and_unique(&v1, &v2);
}

fn test_tableshared_interface<R, TS>(tableshared: TS)
where
    R: Reference<TestData>,
    TS: Table<TestData, R>,
{
    let data = TestData::new(2, 4, "6".to_string());
    test_interface_impl::<R, TS>(&tableshared, data);
}

fn test_tableshared_deduplication_basic<R, TS>(tableshared: TS)
where
    R: Reference<TestData>,
    TS: Table<TestData, R>,
{
    hashcons_two_copies::<R, TS>(&tableshared);
}

/// `get` finds only what `get_or_insert` interned, and finds the same pointer.
///
/// Nothing in hirpdag itself calls `get`, so this is its only coverage. What
/// `get` returns once every strong reference is gone is left unchecked: weak
/// tables forget the entry, `RefLeak` and the strong concurrent tables keep it.
fn test_tableshared_get<R, TS>(tableshared: TS)
where
    R: Reference<TestData>,
    TS: Table<TestData, R>,
{
    let n = 32usize;
    for k in 0..n {
        let data = TestData::new(k as i32, 0, "hello".to_string());
        assert!(tableshared.get(&data).is_none(), "key {} before insert", k);
    }
    let mut interned: Vec<R> = vec![];
    populate_linear(&mut interned, &tableshared, 0..n);
    for (k, x) in interned.iter().enumerate() {
        let data = TestData::new(k as i32, 0, "hello".to_string());
        let found = tableshared
            .get(&data)
            .unwrap_or_else(|| panic!("key {} after insert", k));
        assert!(R::strong_ptr_eq(&found, x), "key {} found another node", k);
    }
    let absent = TestData::new(n as i32, 0, "hello".to_string());
    assert!(tableshared.get(&absent).is_none());
}

/// Run the shared-table checks, each against a table fresh from `new_table`.
pub fn test_tableshared<R, TS>(new_table: impl Fn() -> TS)
where
    R: Reference<TestData>,
    TS: Table<TestData, R>,
{
    test_tableshared_interface::<R, TS>(new_table());
    test_tableshared_deduplication_basic::<R, TS>(new_table());
    test_tableshared_get::<R, TS>(new_table());
}
