//! Stanza correlation for request/response matching.
//!
//! This module provides the infrastructure for correlating outbound stanzas
//! with their responses. It uses a thread-local context to track pending
//! requests and deliver responses via oneshot channels.

use std::cell::RefCell;

use dashmap::DashMap;
use scoped_tls::scoped_thread_local;
use tokio::sync::{mpsc, oneshot};
use tokio_xmpp::Stanza;

pub use stanza_id::{GetStanzaId, StanzaId};

scoped_thread_local!(static CORRELATION_CTX: RefCell<CorrelationContext>);

pub(crate) mod stanza_id {
    use std::borrow::Borrow;
    use std::hash::{Hash, Hasher};

    use xmpp_parsers::iq::Iq;

    /// Private token that prevents external construction of `StanzaId`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Seal;

    /// Newtype wrapper for stanza ID attributes, providing Hash/Eq for DashMap keys.
    ///
    /// This type is public so it can be referenced in type signatures, but cannot
    /// be constructed outside this module due to the private `Seal` field.
    #[derive(Debug, Clone, Copy)]
    pub struct StanzaId<T>(T, Seal);

    impl<T: AsRef<str>> StanzaId<T> {
        pub fn as_str(&self) -> &str {
            self.0.as_ref()
        }

        pub fn to_owned(&self) -> StanzaId<String> {
            StanzaId(self.0.as_ref().to_owned(), Seal)
        }
    }

    impl<T: AsRef<str>> PartialEq for StanzaId<T> {
        fn eq(&self, other: &Self) -> bool {
            self.as_str() == other.as_str()
        }
    }

    impl<T: AsRef<str>> Eq for StanzaId<T> {}

    impl<T: AsRef<str>> Hash for StanzaId<T> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.as_str().hash(state)
        }
    }

    impl Borrow<str> for StanzaId<String> {
        fn borrow(&self) -> &str {
            self.as_str()
        }
    }

    /// Trait for extracting a stanza ID from a stanza type.
    pub trait GetStanzaId {
        fn get_stanza_id(&self) -> Option<StanzaId<&str>>;
    }

    impl GetStanzaId for tokio_xmpp::Stanza {
        fn get_stanza_id(&self) -> Option<StanzaId<&str>> {
            match self {
                tokio_xmpp::Stanza::Iq(ref iq) => {
                    let id = match iq {
                        Iq::Get { id, .. }
                        | Iq::Set { id, .. }
                        | Iq::Result { id, .. }
                        | Iq::Error { id, .. } => id,
                    };
                    Some(StanzaId(id.as_str(), Seal))
                }
                tokio_xmpp::Stanza::Message(ref msg) => {
                    msg.id.as_ref().map(|id| StanzaId(id.0.as_str(), Seal))
                }
                tokio_xmpp::Stanza::Presence(ref pres) => {
                    pres.id.as_deref().map(|id| StanzaId(id, Seal))
                }
            }
        }
    }
}

/// The pending table maps stanza IDs to oneshot senders for response delivery.
pub type PendingTable = DashMap<StanzaId<String>, oneshot::Sender<Stanza>>;

/// Context for correlating outbound stanzas with their responses.
pub struct CorrelationContext {
    pending: PendingTable,
    outbound_tx: mpsc::UnboundedSender<Stanza>,
}

impl CorrelationContext {
    /// Create a new correlation context with the given outbound channel.
    pub fn new(outbound_tx: mpsc::UnboundedSender<Stanza>) -> Self {
        Self {
            pending: DashMap::new(),
            outbound_tx,
        }
    }

    /// Register a pending request and return a receiver for the response.
    pub fn register(&mut self, id: StanzaId<String>) -> oneshot::Receiver<Stanza> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);
        rx
    }

    /// Remove a pending entry and return the sender.
    pub fn take_pending(&mut self, id: &str) -> Option<oneshot::Sender<Stanza>> {
        self.pending.remove(id).map(|(_, tx)| tx)
    }

    pub fn try_take_pending(&mut self, stanza: &Stanza) -> Option<oneshot::Sender<Stanza>> {
        stanza
            .get_stanza_id()
            .and_then(|id| self.pending.remove(id.as_str()))
            .map(|(_, tx)| tx)
    }
    /// Send a stanza to the outbound channel.
    pub fn send(&self, stanza: Stanza) -> Result<(), mpsc::error::SendError<Stanza>> {
        self.outbound_tx.send(stanza)
    }
}

/// Set the correlation context for the duration of a function call.
pub(crate) fn set<F, U>(ctx: &RefCell<CorrelationContext>, func: F) -> U
where
    F: FnOnce() -> U,
{
    CORRELATION_CTX.set(ctx, func)
}

/// Access the correlation context within a function.
pub(crate) fn with<F, R>(func: F) -> R
where
    F: FnOnce(&mut CorrelationContext) -> R,
{
    CORRELATION_CTX.with(|ctx| func(&mut ctx.borrow_mut()))
}
