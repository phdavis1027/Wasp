#[cfg(feature = "tls")]
use std::path::Path;

use futures_util::TryFuture;
use tokio_xmpp::connect::TcpServerConnector;
use tokio_xmpp::{self, Component};

use crate::correlation;
use crate::filter::Filter;
use crate::reject::IsReject;
use crate::reply::Reply;

/// A trait for types that can serve XMPP stanzas using a filter chain.
pub trait ServeComponent: Sized {
    /// Start serving stanzas using the provided filter.
    fn serve<F>(self, filter: F) -> Server<F, run::Standard>
    where
        F: Filter + Clone + Send + Sync + 'static,
        F::Extract: Reply,
        F::Error: IsReject;
}

impl ServeComponent for Component<TcpServerConnector> {
    fn serve<F>(self, filter: F) -> Server<F, run::Standard>
    where
        F: Filter + Clone + Send + Sync + 'static,
        F::Extract: Reply,
        F::Error: IsReject,
    {
        Server {
            filter,
            component: self,
            runner: run::Standard,
        }
    }
}

impl<F, R> std::fmt::Debug for Server<F, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Jid: {}", self.component.jid))
    }
}

/// A warp Server ready to filter requests.
///
/// Construct this type using [`serve()`].
///
/// # Unnameable
///
/// This type is publicly available in the docs only.
///
/// It is not otherwise nameable, since it is a builder type using typestate
/// to allow for ergonomic configuration.
pub struct Server<F, R> {
    component: Component<TcpServerConnector>,
    filter: F,
    runner: R,
}

impl<F, R> Server<F, R>
where
    F: Filter + Clone + Send + Sync + 'static,
    <F::Future as TryFuture>::Ok: Reply,
    <F::Future as TryFuture>::Error: IsReject,
    R: run::Run,
{
    /// Add graceful shutdown support to this server.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn ex(addr: std::net::SocketAddr) {
    /// # use warp::Filter;
    /// # let filter = warp::any().map(|| "ok");
    /// warp::serve(filter)
    ///     .bind(addr).await
    ///     .graceful(async {
    ///         // some signal in here, such as ctrl_c
    ///     })
    ///     .run().await;
    /// # }
    /// ```
    // pub fn graceful<Fut>(self, shutdown_signal: Fut) -> Server<F, run::Graceful<Fut>>
    // where
    //     Fut: Future<Output = ()> + Send + 'static,
    // {
    //     Server {
    //         component: self.component,
    //         filter: self.filter,
    //         runner: run::Graceful(shutdown_signal),
    //     }
    // }

    /// Run this server.
    pub async fn run(self) {
        R::run(self).await;
    }
}

mod run {
    use std::cell::RefCell;

    use futures::{SinkExt, StreamExt};
    use tokio::sync::mpsc;
    use tokio_xmpp::Stanza;

    use crate::correlation::{self, CorrelationContext};

    pub trait Run {
        #[allow(async_fn_in_trait)]
        async fn run<F>(server: super::Server<F, Self>)
        where
            F: super::Filter + Clone + Send + Sync + 'static,
            <F::Future as super::TryFuture>::Ok: super::Reply,
            <F::Future as super::TryFuture>::Error: super::IsReject,
            Self: Sized;
    }

    #[derive(Debug)]
    pub struct Standard;

    impl Run for Standard {
        async fn run<F>(mut server: super::Server<F, Self>)
        where
            F: super::Filter + Clone + Send + Sync + 'static,
            <F::Future as super::TryFuture>::Ok: super::Reply,
            <F::Future as super::TryFuture>::Error: super::IsReject,
            Self: Sized,
        {
            let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Stanza>();
            let ctx = RefCell::new(CorrelationContext::new(outbound_tx));
            let svc = crate::service(server.filter.clone());

            loop {
                tokio::select! {
                    stanza = server.component.next() => {
                        let stanza = stanza.expect("XMPP stream closed unexpectedly");

                        // Check if this stanza's ID is pending
                        // if let Some(tx) = correlation::try_take_pending(&stanza) {
                        //     tx.send(stanza).expect("failed to route response to pending request");
                        //     continue;
                        // }

                        // Not pending - run through filters with ctx set

                        let response = correlation::set(&ctx, || svc.call_stanza(stanza)).await;
                        if let Ok(Some(reply)) = response {
                            if let Err(err) = server.component.send(reply).await {
                                tracing::error!("failed to send reply: {:?}", err);
                            }
                        }
                    }

                    Some(outbound) = outbound_rx.recv() => {
                        if let Err(err) = server.component.send(outbound).await {
                            tracing::error!("failed to send outbound stanza: {:?}", err);
                        }
                    }
                }
            }
        }
    }

    // #[derive(Debug)]
    // pub struct Graceful<Fut>(pub(super) Fut);

    // impl<Fut> Run for Graceful<Fut>
    // where
    //     Fut: super::Future<Output = ()> + Send + 'static,
    // {
    //     async fn run<F>(mut server: super::Server<F, Self, Component<TcpServerConnector>>)
    //     where
    //         F: super::Filter + Clone + Send + Sync + 'static,
    //         <F::Future as super::TryFuture>::Ok: super::Reply,
    //         <F::Future as super::TryFuture>::Error: super::IsReject,
    //         Self: Sized,
    //     {
    //         use futures_util::future;

    //         let pipeline = server.pipeline;
    //         let graceful_util = hyper_util::server::graceful::GracefulShutdown::new();
    //         let mut shutdown_signal = std::pin::pin!(server.runner.0);
    //         loop {
    //             let accept = std::pin::pin!(server.acceptor.accept());
    //             let accepting = match future::select(accept, &mut shutdown_signal).await {
    //                 future::Either::Left((Ok(fut), _)) => fut,
    //                 future::Either::Left((Err(err), _)) => {
    //                     handle_accept_error(err).await;
    //                     continue;
    //                 }
    //                 future::Either::Right(((), _)) => {
    //                     tracing::debug!("shutdown signal received, starting graceful shutdown");
    //                     break;
    //                 }
    //             };
    //             let svc = crate::service(server.filter.clone());
    //             let svc = hyper_util::service::TowerToHyperService::new(svc);
    //             let watcher = graceful_util.watcher();
    //             tokio::spawn(async move {
    //                 let io = match accepting.await {
    //                     Ok(io) => io,
    //                     Err(err) => {
    //                         tracing::debug!("server accepting error: {:?}", err);
    //                         return;
    //                     }
    //                 };
    //                 let mut hyper = hyper_util::server::conn::auto::Builder::new(
    //                     hyper_util::rt::TokioExecutor::new(),
    //                 );
    //                 hyper.http1().pipeline_flush(pipeline);
    //                 let conn = hyper.serve_connection_with_upgrades(io, svc);
    //                 let conn = watcher.watch(conn);
    //                 if let Err(err) = conn.await {
    //                     tracing::error!("server connection error: {:?}", err)
    //                 }
    //             });
    //         }

    //         drop(server.acceptor); // close listener
    //         graceful_util.shutdown().await;
    //     }
    // }

    // TODO: allow providing your own handler
    async fn handle_accept_error(e: std::io::Error) {
        if is_connection_error(&e) {
            return;
        }
        // [From `hyper::Server` in 0.14](https://github.com/hyperium/hyper/blob/v0.14.27/src/server/tcp.rs#L186)
        //
        // > A possible scenario is that the process has hit the max open files
        // > allowed, and so trying to accept a new connection will fail with
        // > `EMFILE`. In some cases, it's preferable to just wait for some time, if
        // > the application will likely close some files (or connections), and try
        // > to accept the connection again. If this option is `true`, the error
        // > will be logged at the `error` level, since it is still a big deal,
        // > and then the listener will sleep for 1 second.
        tracing::error!("accept error: {:?}", e);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    fn is_connection_error(e: &std::io::Error) -> bool {
        // some errors that occur on the TCP stream are emitted when
        // accepting, they can be ignored.
        matches!(
            e.kind(),
            std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
        )
    }
}
