//! Built-in Filters
//!
//! This module mostly serves as documentation to group together the list of
//! built-in filters. Most of these are available at more convenient paths.

pub mod any;
pub mod id;
pub mod log;
pub mod stanza;

pub use crate::filter::BoxedFilter;
pub use id::id;
