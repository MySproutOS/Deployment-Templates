//! The versioned, credential-free process boundary between SproutOS and deployment-template
//! plugins.

pub mod v1;
mod validation;

pub use v1::*;
pub use validation::{
    ProtocolParseError, Validate, ValidationError, parse_request, parse_response,
};

/// The only protocol version understood by this crate.
pub const PROTOCOL_VERSION: u32 = 1;
