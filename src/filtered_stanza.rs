use scoped_tls::scoped_thread_local;
use std::cell::RefCell;
use tokio_xmpp::Stanza;

scoped_thread_local!(static FILTERED_STANZA: RefCell<Stanza>);

pub(crate) fn set<F, U>(r: &RefCell<Stanza>, func: F) -> U
where
    F: FnOnce() -> U,
{
    FILTERED_STANZA.set(r, func)
}

pub(crate) fn is_set() -> bool {
    FILTERED_STANZA.is_set()
}

pub(crate) fn with<F, R>(func: F) -> R
where
    F: FnOnce(&mut Stanza) -> R,
{
    FILTERED_STANZA.with(move |maybe_stanza| func(&mut maybe_stanza.borrow_mut()))
}
