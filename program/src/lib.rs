#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;

pub mod error;
pub mod instruction;
pub mod state;

pub mod utils;

pub mod processor;

#[cfg(feature = "fuzz")]
#[path = "../tests/common/mod.rs"]
pub mod common;
