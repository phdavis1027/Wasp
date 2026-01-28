//! Presence stanza extraction.

use futures_util::future;
use tokio_xmpp::Stanza;
use xmpp_parsers::presence::Presence;

use crate::filter::{filter_fn_one, Filter};
use crate::generic::One;
use crate::reject::Rejection;

/// Extract the incoming stanza as a [`Presence`], rejecting non-presence stanzas.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
/// use xmpp_parsers::presence::Presence;
///
/// let route = warp::presence::param()
///     .map(|presence: Presence| {
///         // Handle the presence
///     });
/// ```
pub fn param() -> impl Filter<Extract = One<Presence>, Error = Rejection> + Copy {
    filter_fn_one(|stanza: &mut Stanza| match stanza {
        Stanza::Presence(pres) => future::ok(pres.clone()),
        _ => future::err(crate::reject::item_not_found()),
    })
}
