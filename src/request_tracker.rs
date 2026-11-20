// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::*;
use bitcoin::block::Header;
use notification::Notification;
use pending_request::{CompletedRequest, FailedRequest, PendingRequest};
use std::collections::HashMap;

/// Represents a high-level event produced after processing a server notification or response.
#[derive(Debug, Clone)]
pub enum Event {
    /// A successfully satisfied response to a previously tracked request.
    ///
    /// Contains the original request and the parsed result.
    Response(CompletedRequest),

    /// A failed response to a previously tracked request.
    ///
    /// Contains the original request and the error returned by the server.
    ResponseError(FailedRequest),

    /// A server-initiated notification that was not in response to any tracked request.
    ///
    /// Typically includes information such as new block headers or script status changes.
    Notification(Notification),
}

impl Event {
    /// Attempts to extract block headers from the event, if applicable.
    ///
    /// Returns a vector of `(height, Header)` pairs for events that contain header data, whether
    /// from a response to a request (e.g., `blockchain.headers.subscribe`) or from a server
    /// notification.
    ///
    /// Returns `None` if the event does not include any header information.
    pub fn try_to_headers(&self) -> Option<Vec<(u32, Header)>> {
        match self {
            Event::Response(CompletedRequest::Header { req, resp }) => {
                Some(vec![(req.height, resp.header)])
            }
            Event::Response(CompletedRequest::Headers { req, resp }) => {
                Some((req.start_height..).zip(resp.headers.clone()).collect())
            }
            Event::Response(CompletedRequest::HeadersWithCheckpoint { req, resp }) => {
                Some((req.start_height..).zip(resp.headers.clone()).collect())
            }
            Event::Notification(Notification::Header(n)) => Some(vec![(n.height(), *n.header())]),
            _ => None,
        }
    }
}

/// A sans-io structure that manages the state of an Electrum client.
///
/// The [`RequestTracker`] tracks outgoing requests and handles incoming messages from the Electrum server.
///
/// Use [`RequestTracker::track_request`] to register a new request. This method stores the request
/// internally and returns a [`RawRequest`] that can be sent to the server.
///
/// Use [`RequestTracker::handle_incoming`] to handle messages received from the server. It updates internal
/// state as needed and may return an [`Event`] representing a notification or a response to a
/// previously tracked request.
#[derive(Debug)]
pub struct RequestTracker {
    pending: HashMap<u32, PendingRequest>,
}

impl Default for RequestTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestTracker {
    /// Creates a new [`RequestTracker`] instance.
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Clears all pending requests.
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Returns an iterator over all pending requests that have been registered with
    /// [`RequestTracker::track_request`] but have not yet received a response.
    ///
    /// Each item in the iterator is a [`RawRequest`] containing the request ID, method name,
    /// and parameters, which can be serialized and sent to the Electrum server.
    pub fn pending_requests(&self) -> impl Iterator<Item = RawRequest> + '_ {
        self.pending.iter().map(|(&id, pending_req)| {
            let (method, params) = pending_req.to_method_and_params();
            RawRequest::new(id, method, params)
        })
    }

    /// Registers a new request (or batch of requests) and returns the corresponding [`RawRequest`]
    /// or batch of [`RawRequest`]s to be sent to the Electrum server.
    ///
    /// Each request is assigned a unique ID (via `next_id`) and stored internally until a matching
    /// response is received via [`RequestTracker::handle_incoming`].
    ///
    /// Returns a [`RawOneOrMany<RawRequest>`], preserving whether the input was a single request or a
    /// batch.
    pub fn track_request<R>(&mut self, next_id: &mut u32, req: R) -> RawOneOrMany<RawRequest>
    where
        R: Into<RawOneOrMany<PendingRequest>>,
    {
        fn _add_request(
            tracker: &mut RequestTracker,
            next_id: &mut u32,
            req: PendingRequest,
        ) -> RawRequest {
            let id = *next_id;
            *next_id = id.wrapping_add(1);
            let (method, params) = req.to_method_and_params();
            tracker.pending.insert(id, req);
            RawRequest::new(id, method, params)
        }
        match req.into() {
            RawOneOrMany::Single(req) => _add_request(self, next_id, req).into(),
            RawOneOrMany::Batch(v) => v
                .into_iter()
                .map(|req| _add_request(self, next_id, req))
                .collect::<Vec<_>>()
                .into(),
        }
    }

    /// Processes an incoming notification or response from the Electrum server and updates internal
    /// state.
    ///
    /// If the input is a server-initiated notification, an [`Event::Notification`] is returned. If
    /// it is a response to a previously tracked request, the corresponding request is resolved and
    /// either an [`Event::Response`] or [`Event::ResponseError`] is returned.
    ///
    /// Returns `Ok(Some(Event))` if an event was produced, `Ok(None)` if no event was needed, or
    /// `Err(ProcessError)` if the input could not be parsed or did not match any known request.
    pub fn handle_incoming(
        &mut self,
        incoming: RawIncoming,
    ) -> Result<Option<Event>, ProcessError> {
        match incoming {
            RawIncoming::Notification(raw) => {
                let notification = Notification::new(&raw).map_err(|error| {
                    ProcessError::CannotDeserializeNotification {
                        method: raw.method,
                        params: raw.params,
                        error,
                    }
                })?;
                Ok(Some(Event::Notification(notification)))
            }
            RawIncoming::Response(resp) => {
                let pending_req = self
                    .pending
                    .remove(&resp.id)
                    .ok_or(ProcessError::MissingRequest(resp.id))?;
                pending_req
                    .handle(resp.result)
                    .map_err(|de_err| ProcessError::CannotDeserializeResponse(resp.id, de_err))
            }
        }
    }
}

/// An error that occurred while processing an incoming server response or notification.
#[derive(Debug)]
pub enum ProcessError {
    /// A response was received for an unknown or untracked request ID.
    MissingRequest(u32),

    /// The server returned a successful response, but it could not be deserialized into the
    /// expected type.
    ///
    /// The `usize` is the request ID, and the `serde_json::Error` is the underlying deserialization
    /// failure.
    CannotDeserializeResponse(u32, serde_json::Error),

    /// A server notification could not be deserialized into the expected notification type.
    ///
    /// This may happen if the notification method is unknown or its parameters are malformed.
    /// The `method` and `params` are the raw JSON-RPC fields from the server, and `error` is the
    /// deserialization failure.
    CannotDeserializeNotification {
        method: CowStr,
        params: serde_json::Value,
        error: serde_json::Error,
    },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::MissingRequest(id) => {
                write!(f, "no pending request found for response with id {}", id)
            }
            ProcessError::CannotDeserializeResponse(id, err) => {
                write!(
                    f,
                    "failed to deserialize response for request id {}: {}",
                    id, err
                )
            }
            ProcessError::CannotDeserializeNotification { method, error, .. } => {
                write!(
                    f,
                    "failed to deserialize notification for method '{}': {}",
                    method, error
                )
            }
        }
    }
}

impl std::error::Error for ProcessError {}
