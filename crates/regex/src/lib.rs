/*!
An implementation of `grep-matcher`'s `Matcher` trait for Rust's regex engine.
*/
#![deny(missing_docs)]

pub use crate::{
    allof::AllOfMatcher,
    error::{Error, ErrorKind},
    literalset::LiteralSetMatcher,
    matcher::{RegexCaptures, RegexMatcher, RegexMatcherBuilder},
};

mod allof;
mod ast;
mod ban;
mod config;
mod error;
mod everyline;
mod literal;
mod literalset;
mod matcher;
mod non_matching;
mod strip;
