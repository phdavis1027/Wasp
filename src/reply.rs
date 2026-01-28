//! Reply to stanzas.
//!
//! A [`Reply`](./trait.Reply.html) is a type that can be converted into an XMPP
//! stanza to be sent back to the sender. These are typically the successful
//! counterpart to a [rejection](../reject).

use tokio_xmpp::Stanza;
use xmpp_parsers::iq::Iq;
use xmpp_parsers::message::Message;
use xmpp_parsers::presence::Presence;

use crate::generic::{Either, One};

/// A type that can be converted into an optional XMPP stanza response.
///
/// Types implementing this trait can be returned from filter chains.
pub trait Reply: ReplySealed + Send {
    /// Convert this reply into an optional stanza to send.
    ///
    /// Returns `None` if no response stanza should be sent.
    fn into_response(self) -> Option<Stanza>;
}

impl<T: Reply + Send> Reply for Option<T> {
    fn into_response(self) -> Option<Stanza> {
        self.and_then(Reply::into_response)
    }
}

impl Reply for Stanza {
    fn into_response(self) -> Option<Stanza> {
        Some(self)
    }
}

impl ReplySealed for Stanza {}

impl Reply for Iq {
    fn into_response(self) -> Option<Stanza> {
        Some(Stanza::Iq(self))
    }
}

impl ReplySealed for Iq {}

impl Reply for Message {
    fn into_response(self) -> Option<Stanza> {
        Some(Stanza::Message(self))
    }
}

impl ReplySealed for Message {}

impl Reply for Presence {
    fn into_response(self) -> Option<Stanza> {
        Some(Stanza::Presence(self))
    }
}

impl ReplySealed for Presence {}

impl<T: Reply + Send> Reply for One<T> {
    fn into_response(self) -> Option<Stanza> {
        self.0.into_response()
    }
}

impl<T: Reply + Send> ReplySealed for One<T> {}

impl<T, U> Reply for Either<T, U>
where
    T: Reply,
    U: Reply,
{
    fn into_response(self) -> Option<Stanza> {
        match self {
            Either::A(a) => a.into_response(),
            Either::B(b) => b.into_response(),
        }
    }
}

impl<T, U> ReplySealed for Either<T, U>
where
    T: Reply,
    U: Reply,
{
}

mod sealed {
    pub trait ReplySealed {}

    impl<T: ReplySealed + Send> ReplySealed for Option<T> {}
    impl ReplySealed for crate::filters::log::internal::Logged {}
}

pub(crate) use self::sealed::ReplySealed;
