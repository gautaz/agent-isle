#![deny(clippy::unwrap_used, clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::indexing_slicing)]
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

pub mod config;
pub mod platform;
pub mod sandbox;
pub mod secrets;
pub mod tools;
pub mod util;
