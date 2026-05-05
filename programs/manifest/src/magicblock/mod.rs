//! Vendored MagicBlock Ephemeral Rollups CPI helpers.
//!
//! Source: https://github.com/magicblock-labs/ephemeral-rollups-sdk @ v0.2.5
//! Vendored to avoid solana-program version conflicts between newer SDK
//! releases and Manifest's pinned `=2.2`. Kept self-contained so it can be
//! upgraded later by replacing this module with a regular crate dependency.
//!
//! Licensed MIT, same as upstream.

pub mod consts;
pub mod cpi;
pub mod ephem;
pub mod types;
mod utils;

pub use consts::*;
pub use cpi::*;
pub use ephem::*;
pub use types::*;
