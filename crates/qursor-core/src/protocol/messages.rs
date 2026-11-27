//! JSON control messages (docs/superpowers/specs/2026-11-26-local-core-design.md §3).
//! The TS mirror is `packages/types/src/core.ts`.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::services::event_sink::CoreEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientType {
	App,
	Renderer,
	Cli,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ClientMsg {
	Hello {
		secret: String,
		client_id: String,
		client_type: ClientType,
		protocol_version: u32,
		app_version: String,
		capabilities: Vec<String>,
	},
	Ping,
	Request {
		request_id: u64,
		method: String,
		params: serde_json::Value,
	},
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ServerMsg {
	ServerInfo {
		host_id: String,
		version: String,
		protocol_version: u32,
		features: Vec<String>,
	},
	Pong,
	Response {
		request_id: u64,
		result: serde_json::Value,
	},
	RpcError {
		request_id: u64,
		error: AppError,
	},
	/// Never `CoreEvent::TerminalData`: its bytes go out as binary `Output` frames.
	Event(CoreEvent),
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn hello_json_shape() {
		let hello = ClientMsg::Hello {
			secret: "s".into(),
			client_id: "c".into(),
			client_type: ClientType::Renderer,
			protocol_version: 1,
			app_version: "0.1.0".into(),
			capabilities: vec![],
		};
		assert_eq!(
			serde_json::to_value(&hello).unwrap(),
			json!({"type":"hello","secret":"s","clientId":"c","clientType":"renderer",
				"protocolVersion":1,"appVersion":"0.1.0","capabilities":[]})
		);
	}

	#[test]
	fn request_parses() {
		let msg: ClientMsg = serde_json::from_str(
			r#"{"type":"request","requestId":3,"method":"core.counts","params":{}}"#,
		)
		.unwrap();
		assert!(matches!(msg, ClientMsg::Request { request_id: 3, .. }));
	}

	#[test]
	fn server_msgs_shape() {
		let err = ServerMsg::RpcError { request_id: 4, error: AppError::NotFound("x".into()) };
		assert_eq!(
			serde_json::to_value(&err).unwrap(),
			json!({"type":"rpc_error","requestId":4,"error":{"kind":"NotFound","message":"x"}})
		);
		let ev = ServerMsg::Event(CoreEvent::DelegationsChanged { project_id: "p".into() });
		assert_eq!(
			serde_json::to_value(&ev).unwrap(),
			json!({"type":"event","event":"delegationsChanged","payload":{"projectId":"p"}})
		);
	}
}
