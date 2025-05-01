use crate::*;
use bitcoin::block::Header;
use notification::Notification;
use pending_request::{ErroredRequest, PendingRequest, SatisfiedRequest};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Event {
    Response(SatisfiedRequest),
    ResponseError(ErroredRequest),
    Notification(Notification),
}

impl Event {
    pub fn try_to_headers(&self) -> Option<Vec<(u32, Header)>> {
        match self {
            Event::Response(SatisfiedRequest::Header { req, resp }) => {
                Some(vec![(req.height, resp.header)])
            }
            Event::Response(SatisfiedRequest::Headers { req, resp }) => {
                Some((req.start_height..).zip(resp.headers.clone()).collect())
            }
            Event::Response(SatisfiedRequest::HeadersWithCheckpoint { req, resp }) => {
                Some((req.start_height..).zip(resp.headers.clone()).collect())
            }
            Event::Notification(Notification::Header(n)) => Some(vec![(n.height(), *n.header())]),
            _ => None,
        }
    }
}

pub struct State<PReq: PendingRequest> {
    next_id: usize,
    pending: HashMap<usize, PReq>,
}

impl<PReq: PendingRequest> State<PReq> {
    pub fn new(next_id: usize) -> Self {
        Self {
            next_id,
            pending: HashMap::new(),
        }
    }

    pub fn pending_requests(&self) -> impl Iterator<Item = RawRequest> + '_ {
        self.pending.iter().map(|(&id, pending_req)| {
            let (method, params) = pending_req.to_method_and_params();
            RawRequest::new(id, method, params)
        })
    }

    pub fn add_request<R>(&mut self, req: R) -> MaybeBatch<RawRequest>
    where
        R: Into<MaybeBatch<PReq>>,
    {
        fn _add_request<PReq: PendingRequest>(state: &mut State<PReq>, req: PReq) -> RawRequest {
            let id = state.next_id;
            state.next_id = id + 1;
            let (method, params) = req.to_method_and_params();
            state.pending.insert(id, req);
            RawRequest::new(id, method, params)
        }
        match req.into() {
            MaybeBatch::Single(req) => _add_request(self, req).into(),
            MaybeBatch::Batch(v) => v
                .into_iter()
                .map(|req| _add_request(self, req))
                .collect::<Vec<_>>()
                .into(),
        }
    }

    pub fn consume(
        &mut self,
        notification_or_response: RawNotificationOrResponse,
    ) -> Result<Option<Event>, ProcessError> {
        match notification_or_response {
            RawNotificationOrResponse::Notification(raw) => {
                let notification = Notification::new(&raw).map_err(|error| {
                    ProcessError::CannotDeserializeNotification {
                        method: raw.method,
                        params: raw.params,
                        error,
                    }
                })?;
                Ok(Some(Event::Notification(notification)))
            }
            RawNotificationOrResponse::Response(resp) => {
                let pending_req = self
                    .pending
                    .remove(&resp.id)
                    .ok_or(ProcessError::MissingRequest(resp.id))?;
                Ok(match resp.result {
                    Ok(raw_resp) => pending_req
                        .satisfy(raw_resp)
                        .map_err(|de_err| ProcessError::CannotDeserializeResponse(resp.id, de_err))?
                        .map(Event::Response),
                    Err(raw_err) => pending_req.satisfy_error(raw_err).map(Event::ResponseError),
                })
            }
        }
    }
}

#[derive(Debug)]
pub enum ProcessError {
    MissingRequest(usize),
    CannotDeserializeResponse(usize, serde_json::Error),
    CannotDeserializeNotification {
        method: CowStr,
        params: serde_json::Value,
        error: serde_json::Error,
    },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "")
    }
}

impl std::error::Error for ProcessError {}
