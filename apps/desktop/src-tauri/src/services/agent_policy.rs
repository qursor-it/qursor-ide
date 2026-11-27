//! The per-command policy decision for agents in a server's terminals (F46,
//! ADR-0110). It reads the address book, which is the app's; the rest of the
//! policy is `qursor_core::services::agent_policy`.

pub use qursor_core::services::agent_policy::*;

use qursor_core::services::terminal::PolicyRequest;

/// The policy for one request: run, run edited, or refuse with a reason the
/// agent reads. Every decision is recorded.
pub fn decide(
	db: &crate::db::Db,
	confirmations: &Confirmations,
	req: &PolicyRequest,
	announce: impl Fn(&ConfirmEvent),
) -> Result<Option<String>, String> {
	let now = || chrono::Utc::now().timestamp_millis();
	let log = |decision: &str| {
		let _ = db.with(|c| Ok(record(c, req, decision, now())?));
	};
	let server = db
		.with(|c| crate::services::servers::get(c, &req.server_id))
		.map_err(|_| "this server is no longer in the address book".to_string())?;
	match server.agent_policy.as_str() {
		"free" => {
			log("allowed");
			Ok(None)
		}
		"readonly" => {
			let ok =
				if req.keys { keys_are_harmless(&req.command) } else { is_read_only(&req.command) };
			if ok {
				log("allowed (read-only)");
				Ok(None)
			} else {
				log("refused (read-only)");
				Err(format!(
					"{} is read-only for agents: only commands that look at things run here. Nothing \
					 was typed. Tell the human what you would run and why.",
					server.name
				))
			}
		}
		_ => {
			let ev = ConfirmEvent {
				id: uuid::Uuid::new_v4().to_string(),
				server_id: server.id.clone(),
				server_name: server.name.clone(),
				terminal_id: req.terminal_id.clone(),
				session_id: req.session_id.clone(),
				command: req.command.clone(),
				keys: req.keys,
				editable: req.editable,
				production: server.environment == "prod",
			};
			match confirmations.ask(ev, announce) {
				Answer::Run => {
					log("confirmed");
					Ok(None)
				}
				Answer::RunEdited(cmd) => {
					log(&format!("edited to: {cmd}"));
					Ok(Some(cmd))
				}
				Answer::Refuse => {
					log("refused");
					Err("the human refused this command; nothing was typed. Do not retry it \
						 unchanged — ask them what they want instead."
						.into())
				}
			}
		}
	}
}
