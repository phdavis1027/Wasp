//! XMPP Stanza filters.
//!
//! These filters extract and match incoming XMPP stanzas, and provide
//! combinators for sending stanzas to other XMPP entities.
//!
//! # Stanza Type Filters
//!
//! The stanza type filters come in two forms:
//! - `warp::message()` / `warp::iq()` / `warp::presence()` - Predicate filters
//!   that match the stanza type without extracting it
//! - `warp::message::param()` / `warp::iq::param()` / `warp::presence::param()` -
//!   Extraction filters that yield the stanza as a parameter to subsequent filters

use std::convert::Infallible;

use futures_util::future;
use tokio_xmpp::Stanza;
use xmpp_parsers::jid::Jid;
use xmpp_parsers::message::{Lang, Message};

use crate::filter::{filter_fn, filter_fn_one, Filter};
use crate::generic::One;
use crate::reject::Rejection;

pub mod iq;
pub mod message;
pub mod presence;

/// Match incoming message stanzas without extracting.
///
/// Use `warp::message::param()` to extract the message for subsequent filters.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
///
/// let route = warp::message()
///     .map(|| {
///         // Handle any message
///     });
/// ```
pub fn message() -> impl Filter<Extract = (), Error = Rejection> + Copy {
    filter_fn(|stanza: &mut Stanza| match stanza {
        Stanza::Message(_) => future::ok(()),
        _ => future::err(crate::reject::item_not_found()),
    })
}

/// Match incoming IQ stanzas without extracting.
///
/// Use `warp::iq::param()` to extract the IQ for subsequent filters.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
///
/// let route = warp::iq()
///     .map(|| {
///         // Handle any IQ
///     });
/// ```
pub fn iq() -> impl Filter<Extract = (), Error = Rejection> + Copy {
    filter_fn(|stanza: &mut Stanza| match stanza {
        Stanza::Iq(_) => future::ok(()),
        _ => future::err(crate::reject::item_not_found()),
    })
}

/// Match incoming presence stanzas without extracting.
///
/// Use `warp::presence::param()` to extract the presence for subsequent filters.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
///
/// let route = warp::presence()
///     .map(|| {
///         // Handle any presence
///     });
/// ```
pub fn presence() -> impl Filter<Extract = (), Error = Rejection> + Copy {
    filter_fn(|stanza: &mut Stanza| match stanza {
        Stanza::Presence(_) => future::ok(()),
        _ => future::err(crate::reject::item_not_found()),
    })
}

/// Extract the sender's JID (`from` attribute) from the incoming stanza.
pub fn sender() -> impl Filter<Extract = One<Option<Jid>>, Error = Infallible> + Copy {
    filter_fn_one(|stanza: &mut Stanza| {
        let from = match stanza {
            Stanza::Message(msg) => msg.from.clone(),
            Stanza::Iq(iq) => match iq {
                xmpp_parsers::iq::Iq::Get { from, .. }
                | xmpp_parsers::iq::Iq::Set { from, .. }
                | xmpp_parsers::iq::Iq::Result { from, .. }
                | xmpp_parsers::iq::Iq::Error { from, .. } => from.clone(),
            },
            Stanza::Presence(pres) => pres.from.clone(),
        };
        future::ok::<_, Infallible>(from)
    })
}

/// Extract the recipient's JID (`to` attribute) from the incoming stanza.
pub fn recipient() -> impl Filter<Extract = One<Option<Jid>>, Error = Infallible> + Copy {
    filter_fn_one(|stanza: &mut Stanza| {
        let to = match stanza {
            Stanza::Message(msg) => msg.to.clone(),
            Stanza::Iq(iq) => match iq {
                xmpp_parsers::iq::Iq::Get { to, .. }
                | xmpp_parsers::iq::Iq::Set { to, .. }
                | xmpp_parsers::iq::Iq::Result { to, .. }
                | xmpp_parsers::iq::Iq::Error { to, .. } => to.clone(),
            },
            Stanza::Presence(pres) => pres.to.clone(),
        };
        future::ok::<_, Infallible>(to)
    })
}

/// Create a message reply with the given body.
///
/// The reply's `to` is the incoming stanza's `from`, and the reply's `from`
/// is the incoming stanza's `to`.
pub fn reply(
    body: impl Into<String>,
) -> impl Filter<Extract = One<Message>, Error = Infallible> + Clone {
    let body = body.into();
    sender()
        .and(recipient())
        .map(move |from: Option<Jid>, to: Option<Jid>| {
            let mut msg = Message::new(from);
            msg.from = to;
            msg.with_body(Lang::default(), body.clone())
        })
}
