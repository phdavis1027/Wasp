//! IQ stanza extraction.

use futures_util::future;
use tokio_xmpp::Stanza;
use xmpp_parsers::iq::Iq;

use crate::filter::{filter_fn_one, Filter};
use crate::generic::One;
use crate::reject::Rejection;

/// Extract the incoming stanza as an [`Iq`], rejecting non-IQ stanzas.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
/// use xmpp_parsers::iq::Iq;
///
/// let route = warp::iq::param()
///     .map(|iq: Iq| {
///         // Handle the IQ request
///     });
/// ```
pub fn param() -> impl Filter<Extract = One<Iq>, Error = Rejection> + Copy {
    filter_fn_one(|stanza: &mut Stanza| match stanza {
        Stanza::Iq(iq) => future::ok(iq.clone()),
        _ => future::err(crate::reject::item_not_found()),
    })
}
