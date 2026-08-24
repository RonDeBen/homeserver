use anyhow::{Context as _, Result};
use async_nats::Client;
use futures::future::BoxFuture;
use futures::stream::{select_all, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use tracing::{error, info};

use crate::Context;

/// Connect to NATS. Jobs publish/subscribe on this client; when we add
/// durable streams later, JetStream is built on top of the same connection
/// (`async_nats::jetstream::new(client)`).
pub async fn connect(nats_url: &str) -> Result<Client> {
    let client = async_nats::connect(nats_url).await?;
    Ok(client)
}

/// A typed bus message: a payload struct bound to the subject it travels on.
///
/// This is the system's message catalog — every message is a type with its
/// subject as an associated const, colocated with the job that owns it. To see
/// every message in the system: `grep -r "impl.*Event for"`. Producers and
/// consumers share the type, so the subject and the payload shape can't drift.
///
/// ```ignore
/// struct CalendarUpdated { stored: u64 }
/// impl common::Event for CalendarUpdated {
///     const SUBJECT: &'static str = "calendar.updated";
/// }
/// ```
pub trait Event: Serialize + DeserializeOwned + Send + Sync + 'static {
    const SUBJECT: &'static str;
}

/// Publish a typed [`Event`], inferring the subject from the type.
pub async fn emit<E: Event>(bus: &Client, event: &E) -> Result<()> {
    publish_json(bus, E::SUBJECT, event).await
}

/// Serialize `payload` as JSON and publish it to `subject`, then flush so it's
/// actually on the wire before we return (matters for short-lived `run-once` /
/// `hsctl` invocations that exit right after publishing). The untyped escape
/// hatch behind [`emit`]; prefer `emit` with an [`Event`] when the subject is
/// known at compile time.
pub async fn publish_json<T: Serialize>(bus: &Client, subject: &str, payload: &T) -> Result<()> {
    let bytes = serde_json::to_vec(payload)?;
    bus.publish(subject.to_string(), bytes.into()).await?;
    bus.flush().await?;
    Ok(())
}

/// Handler after type-erasure: takes the shared context and the raw message,
/// deserializes internally, returns the handler's future.
type Route = Box<dyn Fn(Arc<Context>, async_nats::Message) -> BoxFuture<'static, Result<()>> + Send + Sync>;

/// A job's bus wiring, read top-to-bottom as "what this job listens for and
/// what each message triggers":
///
/// ```ignore
/// Subscriptions::new(ctx)
///     .on(|ctx, _: CalendarRefresh|   async move { refresh(&ctx).await })
///     .on(|ctx, e: UserSubmittedEvent| async move { submit(&ctx, e).await })
///     .serve()
///     .await
/// ```
///
/// Each `.on()` subscribes to the event type's `SUBJECT` (inferred from the
/// handler's parameter type), deserializes into it, and calls the handler.
/// Business logic lives in the handler fns, not here.
pub struct Subscriptions {
    ctx: Arc<Context>,
    routes: Vec<(String, Route)>,
}

impl Subscriptions {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx: Arc::new(ctx),
            routes: Vec::new(),
        }
    }

    /// Register a handler for event type `E`. The handler receives a shared
    /// `Arc<Context>` (cheap to clone — the DB pool and NATS client are already
    /// reference-counted) and the deserialized event.
    pub fn on<E, F, Fut>(mut self, handler: F) -> Self
    where
        E: Event,
        F: Fn(Arc<Context>, E) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let route: Route = Box::new(move |ctx, msg| {
            let handler = Arc::clone(&handler);
            Box::pin(async move {
                let event: E = serde_json::from_slice(&msg.payload)
                    .with_context(|| format!("deserializing {}", E::SUBJECT))?;
                handler(ctx, event).await
            })
        });
        self.routes.push((E::SUBJECT.to_string(), route));
        self
    }

    /// Subscribe to every registered subject and dispatch messages to their
    /// handlers until a shutdown signal arrives. A handler error is logged and
    /// the loop continues — one bad message shouldn't kill the daemon.
    pub async fn serve(self) -> Result<()> {
        let Subscriptions { ctx, routes } = self;
        if routes.is_empty() {
            anyhow::bail!("Subscriptions::serve called with no routes registered");
        }

        let mut handlers = Vec::with_capacity(routes.len());
        let mut streams = Vec::with_capacity(routes.len());
        for (idx, (subject, handler)) in routes.into_iter().enumerate() {
            let sub = ctx.bus.subscribe(subject.clone()).await?;
            info!(%subject, "subscribed");
            handlers.push(handler);
            // Tag each message with its route index; NATS does the (possibly
            // wildcard) subject matching, so we never match subjects by hand.
            streams.push(sub.map(move |msg| (idx, msg)).boxed());
        }

        let mut merged = select_all(streams);
        let shutdown = crate::shutdown();
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    info!("shutdown signal received, stopping");
                    break;
                }
                item = merged.next() => {
                    match item {
                        Some((idx, msg)) => {
                            let subject = msg.subject.to_string();
                            // Sequential dispatch: fine for this system's volume.
                            // If a slow handler ever needs to not block others,
                            // spawn here (handlers are already Arc-shareable).
                            if let Err(e) = handlers[idx](Arc::clone(&ctx), msg).await {
                                error!(%subject, error = %e, "handler error");
                            }
                        }
                        None => {
                            info!("all subscriptions closed, stopping");
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
