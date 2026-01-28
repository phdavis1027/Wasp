//! Message body extraction.

use futures_util::future;
use tokio_xmpp::Stanza;
use xmpp_parsers::message::Lang;

use crate::filter::{filter_fn_one, Filter};
use crate::generic::One;
use crate::reject::Rejection;

/// Extract the best matching body text from a message stanza.
///
/// Uses the default language preference. Rejects with `item-not-found` if:
/// - The stanza is not a message
/// - The message has no body
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
///
/// let route = warp::message::body::param()
///     .map(|body: String| {
///         warp::reply(body)
///     });
/// ```
pub fn param() -> impl Filter<Extract = One<String>, Error = Rejection> + Copy {
    param_with_lang(&[]).map(|(_lang, body)| body)
}

/// Extract body with language tag as `(Lang, String)`.
///
/// Returns the language tag and body text as a tuple. Rejects with `item-not-found` if:
/// - The stanza is not a message
/// - The message has no body matching the preferred languages
///
/// # Arguments
///
/// * `preferred_langs` - List of preferred language codes (e.g., `&["en", "de"]`).
///   Pass an empty slice to accept any language.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
/// use xmpp_parsers::message::Lang;
///
/// let route = warp::message::body::param_with_lang(&["en"])
///     .map(|(lang, body): (Lang, String)| {
///         println!("Received {} message: {}", lang, body);
///     });
/// ```
pub fn param_with_lang(
    preferred_langs: &'static [&'static str],
) -> impl Filter<Extract = One<(Lang, String)>, Error = Rejection> + Copy {
    filter_fn_one(move |stanza: &mut Stanza| {
        let result = match stanza {
            Stanza::Message(msg) => msg
                .get_best_body_cloned(preferred_langs.to_vec())
                .ok_or_else(crate::reject::item_not_found),
            _ => Err(crate::reject::item_not_found()),
        };
        future::ready(result)
    })
}
