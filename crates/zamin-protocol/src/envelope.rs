//! JSON-RPC 2.0 envelope: requests, responses, notifications, and incoming
//! message classification (protocol spec §3).

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC_VERSION: &str = "2.0";

/// Request id: u64 or string on the wire. Clients use monotonically
/// increasing integers. `Null` exists for error replies to malformed
/// requests whose id could not be read (JSON-RPC 2.0 §4.1: "If there was an
/// error in detecting the id in the Request object (e.g. Parse error/Invalid
/// Request), it MUST be Null.").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    String(String),
    Null,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Request {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            method: method.into(),
            params,
        }
    }

    /// Deserialize typed params. A missing `params` field is an error for
    /// methods that take parameters.
    pub fn parse_params<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        let null = Value::Null;
        serde_json::from_value(self.params.as_ref().unwrap_or(&null).clone())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Notification {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.into(),
            params,
        }
    }

    pub fn parse_params<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        let null = Value::Null;
        serde_json::from_value(self.params.as_ref().unwrap_or(&null).clone())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::ProtocolError>,
}

impl Response {
    pub fn ok(id: RequestId, result: Value) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: RequestId, error: crate::ProtocolError) -> Self {
        Response {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// An incoming message, classified per JSON-RPC 2.0:
/// - `method` + `id`        → request
/// - `method` without `id`  → notification
/// - `id` + result/error    → response
#[derive(Debug, Clone, PartialEq)]
pub enum IncomingMessage {
    Request(Request),
    Notification(Notification),
    Response(Response),
}

impl IncomingMessage {
    /// Classify a parsed JSON value. Returns `None` for objects that are none
    /// of the three shapes; tolerant readers do not guess.
    pub fn parse(value: &Value) -> Option<IncomingMessage> {
        let obj = value.as_object()?;
        let has_method = obj.contains_key("method");
        let has_id = obj.contains_key("id");
        let has_result = obj.contains_key("result") || obj.contains_key("error");

        if has_method {
            if has_id {
                Some(IncomingMessage::Request(
                    serde_json::from_value(value.clone()).ok()?,
                ))
            } else {
                Some(IncomingMessage::Notification(
                    serde_json::from_value(value.clone()).ok()?,
                ))
            }
        } else if has_id && has_result {
            Some(IncomingMessage::Response(
                serde_json::from_value(value.clone()).ok()?,
            ))
        } else {
            None
        }
    }
}
