//! Binary frames carry terminal bytes: `opcode u8 | slot u16 BE | payload`
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §3). The slot is the
//! connection-local number `terminal.attach` hands out for a terminal.
//!
//! Opcodes 4..=6 are reserved for snapshots; until they exist `decode` refuses them.

const HEADER: usize = 3;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
	/// Core to client: bytes the terminal printed.
	Output = 1,
	/// Client to core: bytes the user typed.
	Input = 2,
	/// Client to core: payload is `cols u16 BE, rows u16 BE`.
	Resize = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
	#[error("frame shorter than its header")]
	TooShort,
	#[error("unknown opcode {0}")]
	UnknownOp(u8),
	#[error("resize payload is not four bytes")]
	BadResize,
}

pub fn encode(op: Op, slot: u16, payload: &[u8]) -> Vec<u8> {
	let mut out = Vec::with_capacity(HEADER + payload.len());
	out.push(op as u8);
	out.extend_from_slice(&slot.to_be_bytes());
	out.extend_from_slice(payload);
	out
}

pub fn decode(frame: &[u8]) -> Result<(Op, u16, &[u8]), FrameError> {
	let [op, s0, s1, payload @ ..] = frame else {
		return Err(FrameError::TooShort);
	};
	let op = match op {
		1 => Op::Output,
		2 => Op::Input,
		3 => Op::Resize,
		other => return Err(FrameError::UnknownOp(*other)),
	};
	if op == Op::Resize && payload.len() != 4 {
		return Err(FrameError::BadResize);
	}
	Ok((op, u16::from_be_bytes([*s0, *s1]), payload))
}

pub fn encode_resize(slot: u16, cols: u16, rows: u16) -> Vec<u8> {
	let mut payload = [0u8; 4];
	payload[..2].copy_from_slice(&cols.to_be_bytes());
	payload[2..].copy_from_slice(&rows.to_be_bytes());
	encode(Op::Resize, slot, &payload)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roundtrip_output() {
		// A cheap deterministic spread of every byte value; the content is irrelevant.
		let data: Vec<u8> =
			(0..32 * 1024u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
		let frame = encode(Op::Output, 513, &data);
		let (op, slot, payload) = decode(&frame).unwrap();
		assert_eq!((op, slot), (Op::Output, 513));
		assert_eq!(payload, &data[..]);
	}

	#[test]
	fn resize_payload_is_four_bytes() {
		let frame = encode_resize(7, 120, 40);
		assert_eq!(frame, [3, 0, 7, 0, 120, 0, 40]);
		assert_eq!(decode(&frame).unwrap(), (Op::Resize, 7, &frame[3..]));
		assert_eq!(decode(&[3, 0, 7, 0, 120]), Err(FrameError::BadResize));
	}

	#[test]
	fn rejects_short_frame() {
		assert_eq!(decode(&[1, 0]), Err(FrameError::TooShort));
	}

	#[test]
	fn rejects_unknown_op() {
		assert_eq!(decode(&[9, 0, 0]), Err(FrameError::UnknownOp(9)));
		// Reserved for snapshots.
		assert_eq!(decode(&[4, 0, 0]), Err(FrameError::UnknownOp(4)));
	}

	/// The same vector is asserted in `apps/desktop/src/lib/core/frames.test.ts`.
	#[test]
	fn shared_vector() {
		assert_eq!(decode(&[1, 2, 1, 104, 105]).unwrap(), (Op::Output, 513, &b"hi"[..]));
	}
}
