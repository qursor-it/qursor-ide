//! Project Intelligence (F33, ADR-0084).
pub mod dedupe;
pub mod evidence;
pub mod redact;

use sha2::{Digest, Sha256};

/// Lowercase hex sha256. The one hash this feature uses: fingerprints, evidence
/// ids, dedupe keys and the file hashes a change is verified against.
pub fn sha256_hex(bytes: &[u8]) -> String {
	let digest = Sha256::digest(bytes);
	let mut out = String::with_capacity(64);
	for byte in digest {
		out.push_str(&format!("{byte:02x}"));
	}
	out
}
