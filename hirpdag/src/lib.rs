#![forbid(unsafe_code)]

pub mod base;

pub use hirpdag_hashconsing;

// Re-export #[derive(Hirpdag)]
#[doc(hidden)]
pub use hirpdag_derive::*;

pub use lazy_static::lazy_static;
