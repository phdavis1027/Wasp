# Plan: XMPP Stanza Send Combinators

## Goal

Implement three combinators to send stanzas to other XMPP entities:
- `send_iq(iq)` → sends IQ, awaits correlated response via oneshot channel
- `send_message(msg)` → sends message, optionally awaits response if ID present
- `send_presence(pres)` → sends presence, optionally awaits response if ID present

Single underlying `DashMap` correlates stanza IDs to oneshot senders.

## Architecture

```
Filter calls send_iq(iq)
    │
    ▼
Register ID → oneshot::Sender in DashMap
Queue stanza via mpsc channel
    │
    ▼
Server loop sends via component.send()
    │
    ▼
Filter awaits oneshot::Receiver
    │
    ▲
Server receives response, matches ID in DashMap, delivers via oneshot
```

## Files to Modify/Create

| File | Change |
|------|--------|
| `Cargo.toml` | Add `dashmap` dependency |
| `src/correlation.rs` | **NEW**: `RefCell<CorrelationContext>` with `DashMap` + thread-local access |
| `src/filters/id.rs` | **NEW**: `warp::id(String)` and `warp::id::param` filters |
| `src/server.rs` | Add outbound channel, correlation routing, `tokio::select!` |
| `src/filters/stanza.rs` | Implement send_iq, send_message, send_presence + reply helpers |
| `src/lib.rs` | Export new functions, add `mod correlation` |

## Implementation

### 0. Add dependency

```bash
cargo add dashmap
```

### 1. `src/correlation.rs` (new file)

The whole context is wrapped in a `RefCell`. The `DashMap` is not wrapped in `Rc`.

```rust
use std::cell::RefCell;

use dashmap::DashMap;
use scoped_tls::scoped_thread_local;
use tokio::sync::{mpsc, oneshot};
use tokio_xmpp::Stanza;
use xmpp_parsers::iq::Iq;

scoped_thread_local!(static CORRELATION_CTX: RefCell<CorrelationContext>);

/// Newtype wrapper for stanza ID attributes, providing Hash/Eq for DashMap keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StanzaId(pub String);

/// The pending table maps stanza IDs to oneshot senders for response delivery.
pub type PendingTable = DashMap<StanzaId, oneshot::Sender<Stanza>>;

/// Context for correlating outbound stanzas with their responses.
pub struct CorrelationContext {
    pending: PendingTable,
    outbound_tx: mpsc::UnboundedSender<Stanza>,
}

impl CorrelationContext {
    pub fn new(outbound_tx: mpsc::UnboundedSender<Stanza>) -> Self {
        Self {
            pending: DashMap::new(),
            outbound_tx,
        }
    }

    pub fn register(&mut self, id: StanzaId) -> oneshot::Receiver<Stanza> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);
        rx
    }

    pub fn take_pending(&mut self, id: &StanzaId) -> Option<oneshot::Sender<Stanza>> {
        self.pending.remove(id).map(|(_, tx)| tx)
    }

    pub fn send(&self, stanza: Stanza) -> Result<(), mpsc::error::SendError<Stanza>> {
        self.outbound_tx.send(stanza)
    }
}

pub(crate) fn set<F, U>(ctx: &RefCell<CorrelationContext>, func: F) -> U
where
    F: FnOnce() -> U,
{
    CORRELATION_CTX.set(ctx, func)
}

pub(crate) fn with<F, R>(func: F) -> R
where
    F: FnOnce(&mut CorrelationContext) -> R,
{
    CORRELATION_CTX.with(|ctx| func(&mut ctx.borrow_mut()))
}

pub fn extract_stanza_id(stanza: &Stanza) -> Option<StanzaId> {
    match stanza {
        Stanza::Iq(iq) => {
            let id = match iq {
                Iq::Get { id, .. } | Iq::Set { id, .. }
                | Iq::Result { id, .. } | Iq::Error { id, .. } => id.clone(),
            };
            Some(StanzaId(id))
        }
        Stanza::Message(msg) => msg.id.clone().map(StanzaId),
        Stanza::Presence(pres) => pres.id.clone().map(StanzaId),
    }
}
```

### 2. `src/filters/id.rs` (new file)

Simple ID extraction/matching filters - no relation to correlation.

```rust
use std::convert::Infallible;
use crate::filter::{filter_fn_one, Filter};
use crate::generic::One;
use tokio_xmpp::Stanza;
use xmpp_parsers::iq::Iq;
use futures_util::future;

/// Extract the stanza ID from the current stanza (takes ownership via Option::take for Message/Presence)
pub fn param() -> impl Filter<Extract = One<Option<String>>, Error = Infallible> + Copy {
    filter_fn_one(|stanza: &mut Stanza| {
        let id = match stanza {
            Stanza::Iq(iq) => {
                Some(match iq {
                    Iq::Get { id, .. } | Iq::Set { id, .. }
                    | Iq::Result { id, .. } | Iq::Error { id, .. } => id.clone(),
                })
            }
            Stanza::Message(msg) => msg.id.take(),
            Stanza::Presence(pres) => pres.id.take(),
        };
        future::ok::<_, Infallible>(id)
    })
}

/// Filter that matches stanzas with a specific ID
pub fn id(expected: impl Into<String>) -> impl Filter<Extract = (), Error = crate::Rejection> + Clone {
    let expected = expected.into();
    filter_fn_one(move |stanza: &mut Stanza| {
        let actual = match stanza {
            Stanza::Iq(iq) => Some(match iq {
                Iq::Get { id, .. } | Iq::Set { id, .. }
                | Iq::Result { id, .. } | Iq::Error { id, .. } => id.as_str(),
            }),
            Stanza::Message(msg) => msg.id.as_deref(),
            Stanza::Presence(pres) => pres.id.as_deref(),
        };
        if actual == Some(expected.as_str()) {
            future::ok(())
        } else {
            future::err(crate::reject::item_not_found())
        }
    })
}
```

### 3. `src/server.rs` modifications

```rust
async fn run<F>(mut server: super::Server<F, Self>) {
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Stanza>();
    let ctx = RefCell::new(CorrelationContext::new(outbound_tx));
    let svc = crate::service(server.filter.clone());

    loop {
        tokio::select! {
            stanza = server.component.next() => {
                let stanza = stanza.expect("XMPP stream closed unexpectedly");
                
                // Check if this stanza's ID is pending
                if let Some(id) = correlation::extract_stanza_id(&stanza) {
                    if let Some(tx) = ctx.borrow_mut().take_pending(&id) {
                        // ID was pending - must successfully route or panic
                        tx.send(stanza).expect("failed to route response to pending request");
                        continue;
                    }
                }
                
                // Not pending - run through filters with ctx set
                let response = correlation::set(&ctx, || svc.call_stanza(stanza)).await;
                if let Ok(Some(reply)) = response {
                    let _ = server.component.send(reply).await;
                }
            }
            
            Some(outbound) = outbound_rx.recv() => {
                let _ = server.component.send(outbound).await;
            }
        }
    }
}
```

### 4. `src/filters/stanza.rs` additions

Filters are compositional and use `warp::id::param()` for ID extraction. No `_timeout` variants - compose with a separate `timeout` filter instead.

```rust
use crate::correlation::{self, StanzaId};
use crate::filter::Filter;
use crate::generic::One;
use std::convert::Infallible;
use tokio_xmpp::Stanza;
use xmpp_parsers::iq::Iq;
use xmpp_parsers::message::Message;
use xmpp_parsers::presence::Presence;
use xmpp_parsers::jid::Jid;

/// Send IQ, await correlated response.
/// Compose with `warp::timeout(duration)` if you need custom timeout.
pub fn send_iq(iq: Iq) -> impl Filter<Extract = One<Option<Iq>>, Error = Infallible> + Clone {
    let id = extract_iq_id(&iq);
    warp::any()
        .map(move || (iq.clone(), id.clone()))
        .then(|(iq, id): (Iq, StanzaId)| async move {
            let rx = correlation::with(|ctx| {
                let rx = ctx.register(id);
                let _ = ctx.send(Stanza::Iq(iq));
                rx
            });
            match rx.await {
                Ok(Stanza::Iq(response)) => Some(response),
                _ => None,
            }
        })
}

/// Send message. If message has an ID, awaits correlated response.
/// Returns Some(response) on success, None on timeout or fire-and-forget.
pub fn send_message(msg: Message) -> impl Filter<Extract = One<Option<Message>>, Error = Infallible> + Clone {
    let id = msg.id.clone().map(StanzaId);
    warp::any()
        .map(move || (msg.clone(), id.clone()))
        .then(|(msg, id): (Message, Option<StanzaId>)| async move {
            match id {
                Some(id) => {
                    let rx = correlation::with(|ctx| {
                        let rx = ctx.register(id);
                        let _ = ctx.send(Stanza::Message(msg));
                        rx
                    });
                    match rx.await {
                        Ok(Stanza::Message(response)) => Some(response),
                        _ => None,
                    }
                }
                None => {
                    correlation::with(|ctx| { let _ = ctx.send(Stanza::Message(msg)); });
                    None
                }
            }
        })
}

/// Send presence. If presence has an ID, awaits correlated response.
/// Returns Some(response) on success, None on timeout or fire-and-forget.
pub fn send_presence(pres: Presence) -> impl Filter<Extract = One<Option<Presence>>, Error = Infallible> + Clone {
    let id = pres.id.clone().map(StanzaId);
    warp::any()
        .map(move || (pres.clone(), id.clone()))
        .then(|(pres, id): (Presence, Option<StanzaId>)| async move {
            match id {
                Some(id) => {
                    let rx = correlation::with(|ctx| {
                        let rx = ctx.register(id);
                        let _ = ctx.send(Stanza::Presence(pres));
                        rx
                    });
                    match rx.await {
                        Ok(Stanza::Presence(response)) => Some(response),
                        _ => None,
                    }
                }
                None => {
                    correlation::with(|ctx| { let _ = ctx.send(Stanza::Presence(pres)); });
                    None
                }
            }
        })
}

fn extract_iq_id(iq: &Iq) -> StanzaId {
    match iq {
        Iq::Get { id, .. } | Iq::Set { id, .. }
        | Iq::Result { id, .. } | Iq::Error { id, .. } => StanzaId(id.clone()),
    }
}

// =============================================================================
// Reply Helpers - for ergonomic `.then(warp::message::forward(...))` style
// =============================================================================

pub mod message {
    use super::*;
    
    /// Forward a message to another JID and await the response.
    /// Usage: `warp::stanza::message().then(warp::stanza::message::forward(jid))`
    pub fn forward(to: Jid) -> impl Fn(Message) -> futures_util::future::BoxFuture<'static, Option<Message>> + Clone {
        move |mut msg: Message| {
            let to = to.clone();
            Box::pin(async move {
                let id = msg.id.take().map(StanzaId);
                msg.to = Some(to);
                
                match id {
                    Some(id) => {
                        let rx = correlation::with(|ctx| {
                            let rx = ctx.register(id);
                            let _ = ctx.send(Stanza::Message(msg));
                            rx
                        });
                        match rx.await {
                            Ok(Stanza::Message(response)) => Some(response),
                            _ => None,
                        }
                    }
                    None => {
                        correlation::with(|ctx| { let _ = ctx.send(Stanza::Message(msg)); });
                        None
                    }
                }
            })
        }
    }
}

pub mod iq {
    use super::*;
    
    /// Forward an IQ to another JID and await the response.
    /// Usage: `warp::stanza::iq().then(warp::stanza::iq::forward(jid))`
    pub fn forward(to: Jid) -> impl Fn(Iq) -> futures_util::future::BoxFuture<'static, Option<Iq>> + Clone {
        move |iq: Iq| {
            let to = to.clone();
            Box::pin(async move {
                let (id, payload) = match iq {
                    Iq::Get { id, payload, .. } => (id, payload),
                    Iq::Set { id, payload, .. } => (id, payload),
                    _ => return None,  // Don't forward Result/Error
                };
                
                let request = Iq::Get {
                    from: None,
                    to: Some(to),
                    id: id.clone(),
                    payload,
                };
                
                let rx = correlation::with(|ctx| {
                    let rx = ctx.register(StanzaId(id));
                    let _ = ctx.send(Stanza::Iq(request));
                    rx
                });
                
                match rx.await {
                    Ok(Stanza::Iq(response)) => Some(response),
                    _ => None,
                }
            })
        }
    }
}

pub mod presence {
    use super::*;
    
    /// Broadcast presence (fire-and-forget).
    /// Usage: `warp::any().then(warp::stanza::presence::broadcast(PresenceType::Available))`
    pub fn broadcast(type_: xmpp_parsers::presence::Type) -> impl Fn() -> futures_util::future::BoxFuture<'static, ()> + Clone {
        move || {
            let type_ = type_.clone();
            Box::pin(async move {
                let pres = Presence::new(type_);
                correlation::with(|ctx| { let _ = ctx.send(Stanza::Presence(pres)); });
            })
        }
    }
}
```

### 5. `src/lib.rs` additions

```rust
mod correlation;

pub use self::correlation::StanzaId;
pub use self::filters::id;
pub use self::filters::stanza::{
    send_iq, send_message, send_presence,
    message, iq, presence,  // reply helper modules
};
```

## Usage Examples

### Example 1: Echo bot - forward and relay with helper

```rust
use warp::Filter;

let echo = warp::stanza::message()
    .then(warp::stanza::message::forward("other@example.com".parse().unwrap()));
```

### Example 2: IQ forwarding with helper

```rust
use warp::Filter;

let proxy = warp::stanza::iq()
    .then(warp::stanza::iq::forward("pubsub.example.com".parse().unwrap()));
```

### Example 3: Fire-and-forget presence broadcast

```rust
use warp::Filter;
use xmpp_parsers::presence::Type as PresenceType;

let broadcast = warp::any()
    .then(warp::stanza::presence::broadcast(PresenceType::Available));
```

### Example 4: Chained IQ requests with timeout composition

```rust
use warp::Filter;
use std::time::Duration;

let info_request = Iq::Get {
    from: None,
    to: Some("pubsub.example.com".parse().unwrap()),
    id: "info-1".into(),
    payload: DiscoInfoQuery { node: None }.into(),
};

// Compose send_iq with timeout filter
let chained = warp::stanza::message()
    .and(
        warp::stanza::send_iq(info_request)
            .with(warp::timeout(Duration::from_secs(10)))
    )
    .then(|_msg: Message, info_response: Option<Iq>| async move {
        let items_request = Iq::Get {
            from: None,
            to: Some("pubsub.example.com".parse().unwrap()),
            id: "items-1".into(),
            payload: DiscoItemsQuery { node: None }.into(),
        };
        
        warp::stanza::send_iq(items_request).await
    });
```

### Example 5: Using ID filters

```rust
use warp::Filter;

// Match stanzas with a specific ID
let specific = warp::id::id("request-123")
    .and(warp::stanza::iq())
    .map(|iq: Iq| { /* handle */ });

// Extract the ID from incoming stanza
let extract = warp::stanza::message()
    .and(warp::id::param())
    .map(|msg: Message, id: Option<String>| {
        // id was taken from the message
    });
```

## Verification

1. `cargo check` - verify compilation
2. `cargo test` - verify existing tests still pass
3. Write integration test that mocks component stream and verifies correlation
