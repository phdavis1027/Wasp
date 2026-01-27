use std::cell::RefCell;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::future::TryFuture;
use pin_project::pin_project;
use tokio_xmpp::Stanza;
use tower_service::Service;
use xmpp_parsers::iq::Iq;
use xmpp_parsers::message::{Message, MessageType};
use xmpp_parsers::presence::{Presence, Type as PresenceType};
use xmpp_parsers::stanza_error::StanzaError;

use crate::filtered_stanza;
use crate::reject::IsReject;
use crate::reply::Reply;
use crate::Filter;

/// Convert a `Filter` into a `Service`.
///
/// Filters are normally what APIs are built on in warp. However, it can be
/// useful to convert a `Filter` into a [`Service`][Service], such as if
/// further customizing a `hyper::Service`, or if wanting to make use of
/// the greater [Tower][tower] set of middleware.
///
/// # Example
///
/// Running a `warp::Filter` on a regular `hyper::Server`:
///
/// ```
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// use std::convert::Infallible;
/// use warp::Filter;
///
/// // Our Filter...
/// let route = warp::any().map(|| "Hello From Warp!");
///
/// // Convert it into a `Service`...
/// let svc = warp::service(route);
/// # drop(svc);
/// # Ok(())
/// # }
/// ```
///
/// [Service]: https://docs.rs/tower_service/latest/tower_service/trait.Service.html
/// [tower]: https://docs.rs/tower
pub fn service<F>(filter: F) -> FilteredService<F>
where
    F: Filter,
    <F::Future as TryFuture>::Ok: Reply,
    <F::Future as TryFuture>::Error: IsReject,
{
    FilteredService { filter }
}

#[derive(Copy, Clone, Debug)]
pub struct FilteredService<F> {
    filter: F,
}

impl<F> FilteredService<F>
where
    F: Filter,
    <F::Future as TryFuture>::Ok: Reply,
    <F::Future as TryFuture>::Error: IsReject,
{
    #[inline]
    pub(crate) fn call_stanza(&self, stanza: Stanza) -> FilteredFuture<F::Future> {
        debug_assert!(!filtered_stanza::is_set(), "nested route::set calls");

        let stanza = RefCell::new(stanza);
        let fut = filtered_stanza::set(&stanza, || self.filter.filter(super::Internal));
        FilteredFuture {
            future: fut,
            stanza,
        }
    }
}

impl<F> Service<Stanza> for FilteredService<F>
where
    F: Filter,
    <F::Future as TryFuture>::Ok: Reply,
    <F::Future as TryFuture>::Error: IsReject,
{
    type Response = Option<Stanza>;
    type Error = Infallible;
    type Future = FilteredFuture<F::Future>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, stanza: Stanza) -> Self::Future {
        self.call_stanza(stanza)
    }
}

#[pin_project]
#[derive(Debug)]
pub struct FilteredFuture<F> {
    #[pin]
    future: F,
    stanza: ::std::cell::RefCell<Stanza>,
}

impl<F> Future for FilteredFuture<F>
where
    F: TryFuture,
    F::Ok: Reply,
    F::Error: IsReject,
{
    type Output = Result<Option<Stanza>, Infallible>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        debug_assert!(!filtered_stanza::is_set(), "nested route::set calls");

        let pin = self.project();
        let fut = pin.future;
        match filtered_stanza::set(pin.stanza, || fut.try_poll(cx)) {
            Poll::Ready(Ok(ok)) => Poll::Ready(Ok(ok.into_response())),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                tracing::debug!("rejected: {:?}", err);
                let stanza_error = err.into_stanza_error();
                let error_stanza = make_error_stanza(&pin.stanza.borrow(), stanza_error);
                Poll::Ready(Ok(error_stanza))
            }
        }
    }
}

/// Construct an error stanza from the original stanza and a StanzaError.
fn make_error_stanza(original: &Stanza, error: StanzaError) -> Option<Stanza> {
    match original {
        Stanza::Iq(iq) => {
            let (from, to, id) = match iq {
                Iq::Get { from, to, id, .. }
                | Iq::Set { from, to, id, .. }
                | Iq::Result { from, to, id, .. }
                | Iq::Error { from, to, id, .. } => (from.clone(), to.clone(), id.clone()),
            };
            Some(Stanza::Iq(Iq::Error {
                from: to,
                to: from,
                id,
                error,
                payload: None,
            }))
        }
        Stanza::Message(msg) => {
            // Only respond to messages that have an id and aren't already errors
            if msg.type_ == MessageType::Error || msg.id.is_none() {
                return None;
            }
            let mut error_msg = Message::new(msg.from.clone());
            error_msg.from = msg.to.clone();
            error_msg.id = msg.id.clone();
            error_msg.type_ = MessageType::Error;
            error_msg.payloads.push(error.into());
            Some(Stanza::Message(error_msg))
        }
        Stanza::Presence(pres) => {
            // Only respond to presence that has an id and isn't already an error
            if pres.type_ == PresenceType::Error || pres.id.is_none() {
                return None;
            }
            let mut error_pres = Presence::new(PresenceType::Error);
            error_pres.from = pres.to.clone();
            error_pres.to = pres.from.clone();
            error_pres.id = pres.id.clone();
            error_pres.payloads.push(error.into());
            Some(Stanza::Presence(error_pres))
        }
    }
}
