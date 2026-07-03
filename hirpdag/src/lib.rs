#![forbid(unsafe_code)]

pub mod base;

pub use hirpdag_hashconsing;

// Re-export #[derive(Hirpdag)]
#[doc(hidden)]
pub use hirpdag_derive::*;

pub use lazy_static::lazy_static;

// Re-exported for use by hirpdag_derive generated code, so that user crates
// do not need to declare these dependencies themselves.
pub use postcard;
pub use serde;
pub use serde_json;
