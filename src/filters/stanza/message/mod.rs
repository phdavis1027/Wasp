//! Message stanza extraction.

pub mod body;

use futures_util::future;
use tokio_xmpp::Stanza;
use xmpp_parsers::message::Message;

use crate::filter::{filter_fn_one, Filter};
use crate::generic::One;
use crate::reject::Rejection;

/// Extract the incoming stanza as a [`Message`], rejecting non-message stanzas.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
/// use xmpp_parsers::message::Message;
///
/// let route = warp::message::param()
///     .map(|msg: Message| {
///         // Respond to the message
///     });
/// ```
pub fn param() -> impl Filter<Extract = One<Message>, Error = Rejection> + Copy {
    filter_fn_one(|stanza: &mut Stanza| match stanza {
        Stanza::Message(msg) => future::ok(msg.clone()),
        _ => future::err(crate::reject::item_not_found()),
    })
}
