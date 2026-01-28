//! Logger Filters

use std::fmt;
use std::time::{Duration, Instant};

use tokio_xmpp::Stanza;
use xmpp_parsers::jid::Jid;

use crate::filter::{Filter, WrapSealed};
use crate::reject::IsReject;
use crate::reply::Reply;

use self::internal::WithLog;

/// Create a wrapping [`Filter`] with the specified `name` as the `target`.
///
/// This uses the default access logging format, and log records produced
/// will have their `target` set to `name`.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
///
/// let log = warp::log("example::api");
/// let route = warp::presence()
///     .map(warp::sink)
///     .with(log);
/// ```
pub fn log(name: &'static str) -> Log<impl Fn(Info<'_>) + Copy> {
    let func = move |info: Info<'_>| {
        log::info!(
            target: name,
            "{} from={} to={} id={} {:?}",
            info.stanza_type(),
            OptFmt(info.from()),
            OptFmt(info.to()),
            OptFmt(info.id()),
            info.elapsed(),
        );
    };
    Log { func }
}

/// Create a wrapping [`Filter`](crate::Filter) that receives `warp::log::Info`.
///
/// # Example
///
/// ```ignore
/// use warp::Filter;
///
/// let log = warp::log::custom(|info| {
///     eprintln!(
///         "{} from {:?}",
///         info.stanza_type(),
///         info.from(),
///     );
/// });
/// let route = warp::presence()
///     .map(warp::sink)
///     .with(log);
/// ```
pub fn custom<F>(func: F) -> Log<F>
where
    F: Fn(Info<'_>),
{
    Log { func }
}

/// Decorates a [`Filter`] to log stanzas.
#[derive(Clone, Copy, Debug)]
pub struct Log<F> {
    func: F,
}

/// Information about the stanza being processed.
#[allow(missing_debug_implementations)]
pub struct Info<'a> {
    stanza: &'a Stanza,
    start: Instant,
}

impl<FN, F> WrapSealed<F> for Log<FN>
where
    FN: Fn(Info<'_>) + Clone + Send,
    F: Filter + Clone + Send,
    F::Extract: Reply,
    F::Error: IsReject,
{
    type Wrapped = WithLog<FN, F>;

    fn wrap(&self, filter: F) -> Self::Wrapped {
        WithLog {
            filter,
            log: self.clone(),
        }
    }
}

impl<'a> Info<'a> {
    /// The type of stanza ("message", "iq", or "presence").
    pub fn stanza_type(&self) -> &'static str {
        match self.stanza {
            Stanza::Message(_) => "message",
            Stanza::Iq(_) => "iq",
            Stanza::Presence(_) => "presence",
        }
    }

    /// The sender JID (from attribute).
    pub fn from(&self) -> Option<&Jid> {
        match self.stanza {
            Stanza::Message(m) => m.from.as_ref(),
            Stanza::Iq(iq) => match iq {
                xmpp_parsers::iq::Iq::Get { from, .. }
                | xmpp_parsers::iq::Iq::Set { from, .. }
                | xmpp_parsers::iq::Iq::Result { from, .. }
                | xmpp_parsers::iq::Iq::Error { from, .. } => from.as_ref(),
            },
            Stanza::Presence(p) => p.from.as_ref(),
        }
    }

    /// The recipient JID (to attribute).
    pub fn to(&self) -> Option<&Jid> {
        match self.stanza {
            Stanza::Message(m) => m.to.as_ref(),
            Stanza::Iq(iq) => match iq {
                xmpp_parsers::iq::Iq::Get { to, .. }
                | xmpp_parsers::iq::Iq::Set { to, .. }
                | xmpp_parsers::iq::Iq::Result { to, .. }
                | xmpp_parsers::iq::Iq::Error { to, .. } => to.as_ref(),
            },
            Stanza::Presence(p) => p.to.as_ref(),
        }
    }

    /// The stanza ID.
    pub fn id(&self) -> Option<&str> {
        match self.stanza {
            Stanza::Message(m) => m.id.as_ref().map(|id| id.0.as_str()),
            Stanza::Iq(iq) => Some(match iq {
                xmpp_parsers::iq::Iq::Get { id, .. }
                | xmpp_parsers::iq::Iq::Set { id, .. }
                | xmpp_parsers::iq::Iq::Result { id, .. }
                | xmpp_parsers::iq::Iq::Error { id, .. } => id.as_str(),
            }),
            Stanza::Presence(p) => p.id.as_deref(),
        }
    }

    /// The full stanza for custom inspection.
    pub fn stanza(&self) -> &Stanza {
        self.stanza
    }

    /// Time elapsed since filter started processing.
    pub fn elapsed(&self) -> Duration {
        tokio::time::Instant::now().into_std() - self.start
    }
}

struct OptFmt<T>(Option<T>);

impl<T: fmt::Display> fmt::Display for OptFmt<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref t) = self.0 {
            fmt::Display::fmt(t, f)
        } else {
            f.write_str("-")
        }
    }
}

pub(crate) mod internal {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Instant;

    use futures_util::{ready, TryFuture};
    use pin_project::pin_project;
    use tokio_xmpp::Stanza;

    use super::{Info, Log};
    use crate::filter::{Filter, FilterBase, Internal};
    use crate::filtered_stanza;
    use crate::reject::IsReject;
    use crate::reply::Reply;

    #[allow(missing_debug_implementations)]
    pub struct Logged(pub(super) Option<Stanza>);

    impl Reply for Logged {
        #[inline]
        fn into_response(self) -> Option<Stanza> {
            self.0
        }
    }

    #[allow(missing_debug_implementations)]
    #[derive(Clone, Copy)]
    pub struct WithLog<FN, F> {
        pub(super) filter: F,
        pub(super) log: Log<FN>,
    }

    impl<FN, F> FilterBase for WithLog<FN, F>
    where
        FN: Fn(Info<'_>) + Clone + Send,
        F: Filter + Clone + Send,
        F::Extract: Reply,
        F::Error: IsReject,
    {
        type Extract = (Logged,);
        type Error = F::Error;
        type Future = WithLogFuture<FN, F::Future>;

        fn filter(&self, _: Internal) -> Self::Future {
            let started = tokio::time::Instant::now().into_std();
            WithLogFuture {
                log: self.log.clone(),
                future: self.filter.filter(Internal),
                started,
            }
        }
    }

    #[allow(missing_debug_implementations)]
    #[pin_project]
    pub struct WithLogFuture<FN, F> {
        log: Log<FN>,
        #[pin]
        future: F,
        started: Instant,
    }

    impl<FN, F> Future for WithLogFuture<FN, F>
    where
        FN: Fn(Info<'_>),
        F: TryFuture,
        F::Ok: Reply,
        F::Error: IsReject,
    {
        type Output = Result<(Logged,), F::Error>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let pin = self.as_mut().project();
            let result = match ready!(pin.future.try_poll(cx)) {
                Ok(reply) => {
                    let resp = reply.into_response();
                    filtered_stanza::with(|stanza| {
                        (self.log.func)(Info {
                            stanza,
                            start: self.started,
                        });
                    });
                    Poll::Ready(Ok((Logged(resp),)))
                }
                Err(reject) => Poll::Ready(Err(reject)),
            };

            result
        }
    }
}
