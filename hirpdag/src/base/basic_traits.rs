/// Sealed marker trait for primitive numeric types.
///
/// Provides blanket [`HirpdagComputeMeta`](crate::base::meta::HirpdagComputeMeta) and
/// [`HirpdagRewritable`](crate::base::rewrite::HirpdagRewritable) implementations that treat
/// numbers as opaque leaves — they contribute no metadata and pass through rewrites unchanged.
pub trait IsNumber {}

impl IsNumber for i8 {}
impl IsNumber for i16 {}
impl IsNumber for i32 {}
impl IsNumber for i64 {}
impl IsNumber for i128 {}
impl IsNumber for isize {}
impl IsNumber for u8 {}
impl IsNumber for u16 {}
impl IsNumber for u32 {}
impl IsNumber for u64 {}
impl IsNumber for u128 {}
impl IsNumber for usize {}
impl IsNumber for f32 {}
impl IsNumber for f64 {}
