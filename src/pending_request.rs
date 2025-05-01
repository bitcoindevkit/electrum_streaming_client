use crate::{AsyncResponseSender, BlockingResponseSender, MethodAndParams, Request, ResponseError};

pub type AsyncPendingRequestTuple<Req, Resp> = (Req, Option<AsyncResponseSender<Resp>>);
pub type BlockingPendingRequestTuple<Req, Resp> = (Req, Option<BlockingResponseSender<Resp>>);

macro_rules! gen_pending_request_types {
    ($($name:ident),*) => {

        #[derive(Debug, Clone)]
        pub enum SatisfiedRequest {
            $($name {
                req: crate::request::$name,
                resp: <crate::request::$name as Request>::Response,
            }),*,
        }

        #[derive(Debug, Clone)]
        pub enum ErroredRequest {
            $($name {
                req: crate::request::$name,
                error: ResponseError,
            }),*,
        }

        impl core::fmt::Display for ErroredRequest {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(Self::$name { req, error } => write!(f, "Server responsed to {:?} with error: {}", req, error)),*,
                }
            }
        }

        impl std::error::Error for ErroredRequest {}

        pub trait PendingRequest {
            fn to_method_and_params(&self) -> MethodAndParams;
            fn satisfy(self, raw_resp: serde_json::Value) -> Result<Option<SatisfiedRequest>, serde_json::Error>;
            fn satisfy_error(self, raw_error: serde_json::Value) -> Option<ErroredRequest>;
        }

        #[derive(Debug)]
        pub enum AsyncPendingRequest {
            $($name {
                req: crate::request::$name,
                resp_tx: Option<AsyncResponseSender<<crate::request::$name as Request>::Response>>,
            }),*,
        }

        $(
            impl From<AsyncPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>> for AsyncPendingRequest {
                fn from((req, resp_tx): AsyncPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>) -> Self {
                    Self::$name{ req, resp_tx }
                }
            }
        )*

        impl PendingRequest for AsyncPendingRequest {
            fn to_method_and_params(&self) -> MethodAndParams {
                match self {
                    $(AsyncPendingRequest::$name{ req, .. } => req.to_method_and_params()),*
                }
            }

            fn satisfy(self, raw_resp: serde_json::Value) -> Result<Option<SatisfiedRequest>, serde_json::Error> {
                use crate::request;
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        let resp = serde_json::from_value::<<request::$name as Request>::Response>(raw_resp)?;
                        Ok(match resp_tx {
                            Some(tx) => {
                                let _ = tx.send(Ok(resp));
                                None
                            }
                            None => Some(SatisfiedRequest::$name { req, resp }),
                        })
                    }),*
                }
            }

            fn satisfy_error(self, raw_error: serde_json::Value) -> Option<ErroredRequest> {
                let error = ResponseError(raw_error);
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        match resp_tx {
                            Some(tx) => { let _ = tx.send(Err(error)); None }
                            None => Some(ErroredRequest::$name{ req, error }),
                        }
                    }),*
                }
            }
        }

        #[derive(Debug)]
        pub enum BlockingPendingRequest {
            $($name {
                req: crate::request::$name,
                resp_tx: Option<BlockingResponseSender<<crate::request::$name as Request>::Response>>,
            }),*,
        }

        $(
            impl From<BlockingPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>> for BlockingPendingRequest {
                fn from((req, resp_tx): BlockingPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>) -> Self {
                    Self::$name{ req, resp_tx }
                }
            }
        )*

        impl PendingRequest for BlockingPendingRequest {
            fn to_method_and_params(&self) -> MethodAndParams {
                match self {
                    $(BlockingPendingRequest::$name{ req, .. } => req.to_method_and_params()),*
                }
            }

            fn satisfy(self, raw_resp: serde_json::Value) -> Result<Option<SatisfiedRequest>, serde_json::Error> {
                use crate::request;
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        let resp = serde_json::from_value::<<request::$name as Request>::Response>(raw_resp)?;
                        Ok(match resp_tx {
                            Some(tx) => {
                                let _ = tx.send(Ok(resp));
                                None
                            }
                            None => Some(SatisfiedRequest::$name { req, resp }),
                        })
                    }),*
                }
            }

            fn satisfy_error(self, raw_error: serde_json::Value) -> Option<ErroredRequest> {
                let error = ResponseError(raw_error);
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        match resp_tx {
                            Some(tx) => { let _ = tx.send(Err(error)); None }
                            None => Some(ErroredRequest::$name{ req, error }),
                        }
                    }),*
                }
            }
        }
    };
}

gen_pending_request_types! {
    Header,
    HeaderWithProof,
    Headers,
    HeadersWithCheckpoint,
    EstimateFee,
    HeadersSubscribe,
    RelayFee,
    GetBalance,
    GetHistory,
    GetMempool,
    ListUnspent,
    ScriptHashSubscribe,
    ScriptHashUnsubscribe,
    BroadcastTx,
    GetTx,
    GetTxMerkle,
    GetTxidFromPos,
    GetFeeHistogram,
    Banner,
    Ping,
    Custom
}
