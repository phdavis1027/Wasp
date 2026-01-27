//! Rejections
//!
//! Part of the power of the [`Filter`](../trait.Filter.html) system is being able to
//! reject a stanza from a filter chain. This allows for filters to be
//! combined with `or`, so that if one side of the chain finds that a stanza
//! doesn't fulfill its requirements, the other side can try to process
//! the stanza.
//!
//! Many of the built-in [`filters`](../filters) will automatically reject
//! the stanza with an appropriate rejection. However, you can also build
//! new custom [`Filter`](../trait.Filter.html)s and still want other routes to be
//! matchable in the case a predicate doesn't hold.
//!
//! As a stanza is processed by a Filter chain, the rejections are accumulated into
//! a list contained by the [`Rejection`](struct.Rejection.html) type. Rejections from
//! filters can be handled using [`Filter::recover`](../trait.Filter.html#method.recover).
//!
//! # XMPP Error Conditions
//!
//! Rejections map to XMPP stanza error conditions as defined in RFC 6120 and XEP-0086.
//! Each rejection type corresponds to a specific XMPP error condition that will be
//! included in the error stanza response.

use std::any::Any;
use std::convert::Infallible;
use std::fmt;

pub use xmpp_parsers::stanza_error::{DefinedCondition, ErrorType, StanzaError};

pub(crate) use self::sealed::{CombineRejection, IsReject};

/// Rejects a stanza with `item-not-found`.
#[inline]
pub fn reject() -> Rejection {
    item_not_found()
}

/// Rejects a stanza with `item-not-found`.
#[inline]
pub fn item_not_found() -> Rejection {
    Rejection {
        reason: Reason::ItemNotFound,
    }
}

/// Rejects a stanza with a custom cause.
///
/// A [`recover`][] filter should convert this `Rejection` into an appropriate
/// XMPP error stanza, or else this will be returned as an `internal-server-error`.
///
/// [`recover`]: ../trait.Filter.html#method.recover
pub fn custom<T: Reject>(err: T) -> Rejection {
    Rejection::custom(Box::new(err))
}

/// Protect against re-rejecting a rejection.
///
/// ```compile_fail
/// fn with(r: warp::Rejection) {
///     let _wat = warp::reject::custom(r);
/// }
/// ```
fn __reject_custom_compilefail() {}

/// A marker trait to ensure proper types are used for custom rejections.
///
/// Can be converted into Rejection.
///
/// # Example
///
/// ```
/// use warp::{Filter, reject::Reject};
///
/// #[derive(Debug)]
/// struct RateLimited;
///
/// impl Reject for RateLimited {}
///
/// let route = warp::any().and_then(|| async {
///     Err::<(), _>(warp::reject::custom(RateLimited))
/// });
/// ```
// Require `Sized` for now to prevent passing a `Box<dyn Reject>`, since we
// would be double-boxing it, and the downcasting wouldn't work as expected.
pub trait Reject: fmt::Debug + Sized + Send + Sync + 'static {}

trait Cause: fmt::Debug + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
}

impl<T> Cause for T
where
    T: fmt::Debug + Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl dyn Cause {
    fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
}

pub(crate) fn known<T: Into<Known>>(err: T) -> Rejection {
    Rejection::known(err.into())
}

/// Rejection of a request by a [`Filter`](crate::Filter).
///
/// See the [`reject`](module@crate::reject) documentation for more.
pub struct Rejection {
    reason: Reason,
}

enum Reason {
    ItemNotFound,
    Other(Box<Rejections>),
}

enum Rejections {
    Known(Known),
    Custom(Box<dyn Cause>),
    Combined(Box<Rejections>, Box<Rejections>),
}

macro_rules! enum_known {
     ($($(#[$attr:meta])* $var:ident($ty:path),)+) => (
        pub(crate) enum Known {
            $(
            $(#[$attr])*
            $var($ty),
            )+
        }

        impl Known {
            fn inner_as_any(&self) -> &dyn Any {
                match *self {
                    $(
                    $(#[$attr])*
                    Known::$var(ref t) => t,
                    )+
                }
            }
        }

        impl fmt::Debug for Known {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match *self {
                    $(
                    $(#[$attr])*
                    Known::$var(ref t) => t.fmt(f),
                    )+
                }
            }
        }

        impl fmt::Display for Known {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match *self {
                    $(
                    $(#[$attr])*
                    Known::$var(ref t) => t.fmt(f),
                    )+
                }
            }
        }

        $(
        #[doc(hidden)]
        $(#[$attr])*
        impl From<$ty> for Known {
            fn from(ty: $ty) -> Known {
                Known::$var(ty)
            }
        }
        )+
    );
}

enum_known! {
    BadRequest(BadRequest),
    Conflict(Conflict),
    FeatureNotImplemented(FeatureNotImplemented),
    Forbidden(Forbidden),
    Gone(Gone),
    InternalServerError(InternalServerError),
    ItemNotFound(ItemNotFound),
    JidMalformed(JidMalformed),
    NotAcceptable(NotAcceptable),
    NotAllowed(NotAllowed),
    NotAuthorized(NotAuthorized),
    RecipientUnavailable(RecipientUnavailable),
    Redirect(Redirect),
    RegistrationRequired(RegistrationRequired),
    RemoteServerNotFound(RemoteServerNotFound),
    RemoteServerTimeout(RemoteServerTimeout),
    ResourceConstraint(ResourceConstraint),
    ServiceUnavailable(ServiceUnavailable),
    SubscriptionRequired(SubscriptionRequired),
    UndefinedCondition(UndefinedCondition),
    UnexpectedRequest(UnexpectedRequest),
}

impl Rejection {
    fn known(known: Known) -> Self {
        Rejection {
            reason: Reason::Other(Box::new(Rejections::Known(known))),
        }
    }

    fn custom(other: Box<dyn Cause>) -> Self {
        Rejection {
            reason: Reason::Other(Box::new(Rejections::Custom(other))),
        }
    }

    /// Searches this `Rejection` for a specific cause.
    ///
    /// A `Rejection` will accumulate causes over a `Filter` chain. This method
    /// can search through them and return the first cause of this type.
    ///
    /// # Example
    ///
    /// ```
    /// #[derive(Debug)]
    /// struct Nope;
    ///
    /// impl warp::reject::Reject for Nope {}
    ///
    /// let reject = warp::reject::custom(Nope);
    ///
    /// if let Some(nope) = reject.find::<Nope>() {
    ///    println!("found it: {:?}", nope);
    /// }
    /// ```
    pub fn find<T: 'static>(&self) -> Option<&T> {
        if let Reason::Other(ref rejections) = self.reason {
            return rejections.find();
        }
        None
    }

    /// Returns true if this Rejection was made via `warp::reject::item_not_found`.
    ///
    /// # Example
    ///
    /// ```
    /// let rejection = warp::reject();
    ///
    /// assert!(rejection.is_item_not_found());
    /// ```
    pub fn is_item_not_found(&self) -> bool {
        matches!(self.reason, Reason::ItemNotFound)
    }
}

impl<T: Reject> From<T> for Rejection {
    #[inline]
    fn from(err: T) -> Rejection {
        custom(err)
    }
}

impl From<Infallible> for Rejection {
    #[inline]
    fn from(infallible: Infallible) -> Rejection {
        match infallible {}
    }
}

impl IsReject for Infallible {
    fn error_condition(&self) -> DefinedCondition {
        match *self {}
    }

    fn into_stanza_error(&self) -> StanzaError {
        match *self {}
    }
}

impl IsReject for Rejection {
    fn error_condition(&self) -> DefinedCondition {
        match self.reason {
            Reason::ItemNotFound => DefinedCondition::ItemNotFound,
            Reason::Other(ref other) => other.error_condition(),
        }
    }

    fn into_stanza_error(&self) -> StanzaError {
        match self.reason {
            Reason::ItemNotFound => StanzaError::new(
                ErrorType::Cancel,
                DefinedCondition::ItemNotFound,
                "en",
                "item-not-found",
            ),
            Reason::Other(ref other) => other.into_stanza_error(),
        }
    }
}

impl fmt::Debug for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Rejection").field(&self.reason).finish()
    }
}

impl fmt::Debug for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Reason::ItemNotFound => f.write_str("ItemNotFound"),
            Reason::Other(ref other) => match **other {
                Rejections::Known(ref e) => fmt::Debug::fmt(e, f),
                Rejections::Custom(ref e) => fmt::Debug::fmt(e, f),
                Rejections::Combined(ref a, ref b) => {
                    let mut list = f.debug_list();
                    a.debug_list(&mut list);
                    b.debug_list(&mut list);
                    list.finish()
                }
            },
        }
    }
}

// ===== Rejections =====

impl Rejections {
    fn error_condition(&self) -> DefinedCondition {
        match *self {
            Rejections::Known(ref k) => match *k {
                Known::BadRequest(_) => DefinedCondition::BadRequest,
                Known::Conflict(_) => DefinedCondition::Conflict,
                Known::FeatureNotImplemented(_) => DefinedCondition::FeatureNotImplemented,
                Known::Forbidden(_) => DefinedCondition::Forbidden,
                Known::Gone(_) => DefinedCondition::Gone { new_address: None },
                Known::InternalServerError(_) => DefinedCondition::InternalServerError,
                Known::ItemNotFound(_) => DefinedCondition::ItemNotFound,
                Known::JidMalformed(_) => DefinedCondition::JidMalformed,
                Known::NotAcceptable(_) => DefinedCondition::NotAcceptable,
                Known::NotAllowed(_) => DefinedCondition::NotAllowed,
                Known::NotAuthorized(_) => DefinedCondition::NotAuthorized,
                Known::RecipientUnavailable(_) => DefinedCondition::RecipientUnavailable,
                Known::Redirect(_) => DefinedCondition::Redirect { new_address: None },
                Known::RegistrationRequired(_) => DefinedCondition::RegistrationRequired,
                Known::RemoteServerNotFound(_) => DefinedCondition::RemoteServerNotFound,
                Known::RemoteServerTimeout(_) => DefinedCondition::RemoteServerTimeout,
                Known::ResourceConstraint(_) => DefinedCondition::ResourceConstraint,
                Known::ServiceUnavailable(_) => DefinedCondition::ServiceUnavailable,
                Known::SubscriptionRequired(_) => DefinedCondition::SubscriptionRequired,
                Known::UndefinedCondition(_) => DefinedCondition::UndefinedCondition,
                Known::UnexpectedRequest(_) => DefinedCondition::UnexpectedRequest,
            },
            Rejections::Custom(..) => DefinedCondition::UndefinedCondition,
            Rejections::Combined(..) => self.preferred().error_condition(),
        }
    }

    fn error_type(&self) -> ErrorType {
        match *self {
            Rejections::Known(ref k) => match *k {
                // Auth errors - retry after providing credentials
                Known::NotAuthorized(_)
                | Known::Forbidden(_)
                | Known::RegistrationRequired(_)
                | Known::SubscriptionRequired(_) => ErrorType::Auth,

                // Cancel errors - do not retry
                Known::Conflict(_)
                | Known::FeatureNotImplemented(_)
                | Known::Gone(_)
                | Known::InternalServerError(_)
                | Known::ItemNotFound(_)
                | Known::NotAllowed(_)
                | Known::RemoteServerNotFound(_) => ErrorType::Cancel,

                // Modify errors - retry after changing data
                Known::BadRequest(_)
                | Known::JidMalformed(_)
                | Known::NotAcceptable(_)
                | Known::Redirect(_) => ErrorType::Modify,

                // Wait errors - retry after waiting
                Known::RecipientUnavailable(_)
                | Known::RemoteServerTimeout(_)
                | Known::ResourceConstraint(_)
                | Known::ServiceUnavailable(_) => ErrorType::Wait,

                // Undefined - default to cancel
                Known::UndefinedCondition(_) | Known::UnexpectedRequest(_) => ErrorType::Cancel,
            },
            Rejections::Custom(..) => ErrorType::Cancel,
            Rejections::Combined(..) => self.preferred().error_type(),
        }
    }

    fn into_stanza_error(&self) -> StanzaError {
        match *self {
            Rejections::Known(ref e) => StanzaError::new(
                self.error_type(),
                self.error_condition(),
                "en",
                e.to_string(),
            ),
            Rejections::Custom(ref e) => {
                tracing::error!(
                    "unhandled custom rejection, returning undefined-condition: {:?}",
                    e
                );
                StanzaError::new(
                    ErrorType::Cancel,
                    DefinedCondition::UndefinedCondition,
                    "en",
                    format!("Unhandled rejection: {:?}", e),
                )
            }
            Rejections::Combined(..) => self.preferred().into_stanza_error(),
        }
    }

    fn find<T: 'static>(&self) -> Option<&T> {
        match *self {
            Rejections::Known(ref e) => e.inner_as_any().downcast_ref(),
            Rejections::Custom(ref e) => e.downcast_ref(),
            Rejections::Combined(ref a, ref b) => a.find().or_else(|| b.find()),
        }
    }

    fn debug_list(&self, f: &mut fmt::DebugList<'_, '_>) {
        match *self {
            Rejections::Known(ref e) => {
                f.entry(e);
            }
            Rejections::Custom(ref e) => {
                f.entry(e);
            }
            Rejections::Combined(ref a, ref b) => {
                a.debug_list(f);
                b.debug_list(f);
            }
        }
    }

    fn preferred(&self) -> &Rejections {
        match self {
            Rejections::Known(_) | Rejections::Custom(_) => self,
            Rejections::Combined(a, b) => {
                let a = a.preferred();
                let b = b.preferred();
                // Compare error types with this priority:
                // - ItemNotFound is lowest (default rejection)
                // - Custom rejections are higher priority
                // - Otherwise prefer the first one
                match (a.error_condition(), b.error_condition()) {
                    (_, DefinedCondition::ItemNotFound) => a,
                    (DefinedCondition::ItemNotFound, _) => b,
                    _ => a,
                }
            }
        }
    }
}

crate::unit_error! {
    /// The sender has sent a stanza containing XML that does not conform to the appropriate schema
    /// or that cannot be processed (e.g., an IQ stanza that includes an unrecognized value of the
    /// 'type' attribute, or an element that is qualified by a recognized namespace but that violates
    /// the defined syntax for that element).
    pub BadRequest: "bad-request"
}

crate::unit_error! {
    /// Access cannot be granted because an existing resource exists with the same name or address.
    pub Conflict: "conflict"
}

crate::unit_error! {
    /// The feature requested is not implemented by the recipient or server and therefore cannot be processed.
    pub FeatureNotImplemented: "feature-not-implemented"
}

crate::unit_error! {
    /// The requesting entity does not possess the necessary permissions to perform an action that
    /// only certain authorized roles or individuals are allowed to complete.
    pub Forbidden: "forbidden"
}

crate::unit_error! {
    /// The recipient or server can no longer be contacted at this address, typically on a permanent
    /// basis. The associated error text SHOULD include a new address or inform the sender of
    /// appropriate action to take.
    pub Gone: "gone"
}

crate::unit_error! {
    /// The server has experienced a misconfiguration or other internal error that prevents it from
    /// processing the stanza.
    pub InternalServerError: "internal-server-error"
}

crate::unit_error! {
    /// The addressed JID or item requested cannot be found.
    pub ItemNotFound: "item-not-found"
}

crate::unit_error! {
    /// The sending entity has provided an invalid JID.
    pub JidMalformed: "jid-malformed"
}

crate::unit_error! {
    /// The recipient or server understands the request but cannot process it because it does not
    /// meet criteria imposed by the recipient or server (e.g., a request to subscribe to information
    /// that does not simultaneously include configuration parameters acceptable to the recipient).
    pub NotAcceptable: "not-acceptable"
}

crate::unit_error! {
    /// The recipient or server does not allow any entity to perform the action.
    pub NotAllowed: "not-allowed"
}

crate::unit_error! {
    /// The sender needs to provide credentials before being allowed to perform the action, or has
    /// provided improper credentials.
    pub NotAuthorized: "not-authorized"
}

crate::unit_error! {
    /// The intended recipient is temporarily unavailable, undergoing maintenance, etc.
    pub RecipientUnavailable: "recipient-unavailable"
}

crate::unit_error! {
    /// The recipient or server is redirecting requests for this information to another entity,
    /// typically in a temporary fashion.
    pub Redirect: "redirect"
}

crate::unit_error! {
    /// The requesting entity is not authorized to access the requested service because prior
    /// registration is necessary.
    pub RegistrationRequired: "registration-required"
}

crate::unit_error! {
    /// A remote server or service specified as part or all of the JID of the intended recipient
    /// does not exist or cannot be resolved.
    pub RemoteServerNotFound: "remote-server-not-found"
}

crate::unit_error! {
    /// A remote server or service specified as part or all of the JID of the intended recipient
    /// could not be contacted within a reasonable amount of time.
    pub RemoteServerTimeout: "remote-server-timeout"
}

crate::unit_error! {
    /// The server or recipient is busy or lacks the system resources necessary to service the request.
    pub ResourceConstraint: "resource-constraint"
}

crate::unit_error! {
    /// The server or recipient does not currently provide the requested service.
    pub ServiceUnavailable: "service-unavailable"
}

crate::unit_error! {
    /// The requesting entity is not authorized to access the requested service because a prior
    /// subscription is necessary.
    pub SubscriptionRequired: "subscription-required"
}

crate::unit_error! {
    /// The error condition is not one of those defined by the other conditions in this list.
    pub UndefinedCondition: "undefined-condition"
}

crate::unit_error! {
    /// The recipient or server understood the request but was not expecting it at this time
    /// (e.g., the request was out of order).
    pub UnexpectedRequest: "unexpected-request"
}

mod sealed {
    use super::{DefinedCondition, Reason, Rejection, Rejections, StanzaError};
    use std::convert::Infallible;
    use std::fmt;

    // This sealed trait exists to allow Filters to return either `Rejection`
    // or `!`. There are no other types that make sense, and so it is sealed.
    pub trait IsReject: fmt::Debug + Send + Sync {
        fn error_condition(&self) -> DefinedCondition;
        fn into_stanza_error(&self) -> StanzaError;
    }

    fn _assert_object_safe() {
        fn _assert(_: &dyn IsReject) {}
    }

    // This weird trait is to allow optimizations of propagating when a
    // rejection can *never* happen (currently with the `Never` type,
    // eventually to be replaced with `!`).
    //
    // Using this trait means the `Never` gets propagated to chained filters,
    // allowing LLVM to eliminate more code paths. Without it, such as just
    // requiring that `Rejection::from(Never)` were used in those filters,
    // would mean that links later in the chain may assume a rejection *could*
    // happen, and no longer eliminate those branches.
    pub trait CombineRejection<E>: Send + Sized {
        /// The type that should be returned when only 1 of the two
        /// "rejections" occurs.
        ///
        /// # For example:
        ///
        /// `warp::any().and(warp::path("foo"))` has the following steps:
        ///
        /// 1. Since this is `and`, only **one** of the rejections will occur,
        ///    and as soon as it does, it will be returned.
        /// 2. `warp::any()` rejects with `Never`. So, it will never return `Never`.
        /// 3. `warp::path()` rejects with `Rejection`. It may return `Rejection`.
        ///
        /// Thus, if the above filter rejects, it will definitely be `Rejection`.
        type One: IsReject + From<Self> + From<E> + Into<Rejection>;

        /// The type that should be returned when both rejections occur,
        /// and need to be combined.
        type Combined: IsReject;

        fn combine(self, other: E) -> Self::Combined;
    }

    impl CombineRejection<Rejection> for Rejection {
        type One = Rejection;
        type Combined = Rejection;

        fn combine(self, other: Rejection) -> Self::Combined {
            let reason = match (self.reason, other.reason) {
                (Reason::Other(left), Reason::Other(right)) => {
                    Reason::Other(Box::new(Rejections::Combined(left, right)))
                }
                (Reason::Other(other), Reason::ItemNotFound)
                | (Reason::ItemNotFound, Reason::Other(other)) => {
                    // ignore the ItemNotFound
                    Reason::Other(other)
                }
                (Reason::ItemNotFound, Reason::ItemNotFound) => Reason::ItemNotFound,
            };

            Rejection { reason }
        }
    }

    impl CombineRejection<Infallible> for Rejection {
        type One = Rejection;
        type Combined = Infallible;

        fn combine(self, other: Infallible) -> Self::Combined {
            match other {}
        }
    }

    impl CombineRejection<Rejection> for Infallible {
        type One = Rejection;
        type Combined = Infallible;

        fn combine(self, _: Rejection) -> Self::Combined {
            match self {}
        }
    }

    impl CombineRejection<Infallible> for Infallible {
        type One = Infallible;
        type Combined = Infallible;

        fn combine(self, _: Infallible) -> Self::Combined {
            match self {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Left;

    #[derive(Debug, PartialEq)]
    struct Right;

    impl Reject for Left {}
    impl Reject for Right {}

    #[test]
    fn rejection_error_condition() {
        assert_eq!(
            item_not_found().error_condition(),
            DefinedCondition::ItemNotFound
        );
        assert_eq!(
            custom(Left).error_condition(),
            DefinedCondition::UndefinedCondition
        );
    }

    #[test]
    fn combine_rejection_causes_with_some_left_and_none_right() {
        let left = custom(Left);
        let right = item_not_found();
        let reject = left.combine(right);
        let err = reject.into_stanza_error();

        assert_eq!(err.defined_condition, DefinedCondition::UndefinedCondition);
    }

    #[test]
    fn combine_rejection_causes_with_none_left_and_some_right() {
        let left = item_not_found();
        let right = custom(Right);
        let reject = left.combine(right);
        let err = reject.into_stanza_error();

        assert_eq!(err.defined_condition, DefinedCondition::UndefinedCondition);
    }

    #[test]
    fn unhandled_customs() {
        let reject = item_not_found().combine(custom(Right));

        let err = reject.into_stanza_error();
        assert_eq!(err.defined_condition, DefinedCondition::UndefinedCondition);

        // There's no real way to determine which is worse, so pick the first one.
        let reject = custom(Left).combine(custom(Right));

        let err = reject.into_stanza_error();
        assert_eq!(err.defined_condition, DefinedCondition::UndefinedCondition);

        // With many rejections, custom still is top priority over item-not-found.
        let reject = item_not_found()
            .combine(item_not_found())
            .combine(item_not_found())
            .combine(custom(Right))
            .combine(item_not_found());

        let err = reject.into_stanza_error();
        assert_eq!(err.defined_condition, DefinedCondition::UndefinedCondition);
    }

    #[test]
    fn find_cause() {
        let rej = custom(Left);

        assert_eq!(rej.find::<Left>(), Some(&Left));

        let rej = rej.combine(known(BadRequest { _p: () }));

        assert_eq!(rej.find::<Left>(), Some(&Left));
        assert!(rej.find::<BadRequest>().is_some(), "BadRequest");
    }

    #[test]
    fn size_of_rejection() {
        assert_eq!(
            ::std::mem::size_of::<Rejection>(),
            ::std::mem::size_of::<usize>(),
        );
    }

    #[derive(Debug)]
    struct X(#[allow(unused)] u32);
    impl Reject for X {}

    fn combine_n<F, R>(n: u32, new_reject: F) -> Rejection
    where
        F: Fn(u32) -> R,
        R: Reject,
    {
        let mut rej = item_not_found();

        for i in 0..n {
            rej = rej.combine(custom(new_reject(i)));
        }

        rej
    }

    #[test]
    fn test_debug() {
        let rej = combine_n(3, X);

        let s = format!("{:?}", rej);
        assert_eq!(s, "Rejection([X(0), X(1), X(2)])");
    }

    #[test]
    fn convert_big_rejections_into_stanza_error() {
        let mut rejections = Rejections::Custom(Box::new(std::io::Error::from_raw_os_error(100)));
        for _ in 0..50 {
            rejections = Rejections::Combined(
                Box::new(Rejections::Known(Known::BadRequest(BadRequest { _p: () }))),
                Box::new(rejections),
            );
        }
        let reason = Reason::Other(Box::new(rejections));
        let rejection = Rejection { reason };
        assert_eq!(
            DefinedCondition::UndefinedCondition,
            rejection.into_stanza_error().defined_condition
        );
    }
}
