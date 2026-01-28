#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![deny(rust_2018_idioms)]
#![cfg_attr(test, deny(warnings))]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! # wasp
//!
//! wasp is a composable XMPP component framework built on the Filter pattern.
//!
//! Thanks to its [`Filter`][Filter] system, wasp provides composable stanza routing
//! and processing for XMPP components.
//!
//! ## Filters
//!
//! The main concept in wasp is the [`Filter`][Filter], which allows composition
//! to describe various stanza handlers in your XMPP component. Besides this powerful
//! trait, wasp comes with several built in [filters](filters/index.html), which
//! can be combined for your specific needs.
//!
//! Filters can [`reject`][reject] stanzas that don't match their requirements,
//! allowing the next filter in an `or` chain to try processing the stanza.
//!
//! [Filter]: trait.Filter.html
//! [reject]: reject/index.html

pub(crate) mod correlation;
mod error;
mod filter;
mod filtered_stanza;
pub mod filters;
mod generic;
pub mod reject;
pub mod reply;
#[cfg(feature = "server")]
mod server;
mod service;

pub use self::error::Error;
pub use self::filter::wrap_fn;
pub use self::filter::Filter;
pub use self::filters::any::any;
pub use self::filters::id::id;
pub mod id {
    //! Stanza ID filters.
    pub use crate::filters::id::param;
}
pub use self::filters::stanza::iq;
pub use self::filters::stanza::message;
pub use self::filters::stanza::presence;
pub use self::filters::stanza::{recipient, reply, sender};
pub use self::reject::{reject, Rejection};
pub use self::reply::Reply;
#[cfg(feature = "server")]
pub use self::server::ServeComponent;
pub use self::service::service;

// Re-export XMPP types for convenience
#[doc(hidden)]
pub use tokio_xmpp::Stanza;
#[doc(hidden)]
pub use xmpp_parsers;

#[doc(hidden)]
pub use futures_util::{Future, Sink, Stream};
