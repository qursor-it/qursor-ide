//! What the agent's `canvas*` tools do to a canvas document (F44, ADR-0104).
//!
//! Pure functions over the parsed JSON (`serde_json::Value`, with
//! `preserve_order`, so unknown fields and key order survive). Reading and
//! writing the file is `services::canvas`; wiring is `agent_tools`.
//!
//! The containment rule mirrors `parseCanvas` in `jsonCanvas.ts`: the smallest
//! group that fully contains a node is its parent. A shared fixture keeps the
//! two honest (`containment.canvas`).

use serde_json::{json, Map, Value};

const DEFAULT_SIZE: f64 = 160.0;
/// Grid step and margin of [`free_spot`].
const GRID: f64 = 40.0;
/// How many rings out [`free_spot`] looks before giving up and going right.
const RINGS: i64 = 60;

#[derive(Debug, Clone, Copy)]
struct Bx {
	x: f64,
	y: f64,
	w: f64,
	h: f64,
}

fn num(v: Option<&Value>, fallback: f64) -> f64 {
	v.and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(fallback)
}

fn bx(n: &Value) -> Bx {
	Bx {
		x: num(n.get("x"), 0.0),
		y: num(n.get("y"), 0.0),
		w: num(n.get("width"), DEFAULT_SIZE),
		h: num(n.get("height"), DEFAULT_SIZE),
	}
}

fn contains(outer: Bx, inner: Bx) -> bool {
	inner.x >= outer.x
		&& inner.y >= outer.y
		&& inner.x + inner.w <= outer.x + outer.w
		&& inner.y + inner.h <= outer.y + outer.h
		&& inner.w * inner.h < outer.w * outer.h
}

fn nodes(doc: &Value) -> &[Value] {
	doc.get("nodes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn edges(doc: &Value) -> &[Value] {
	doc.get("edges").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn id_of(n: &Value) -> &str {
	n.get("id").and_then(Value::as_str).unwrap_or_default()
}

fn is_group(n: &Value) -> bool {
	n.get("type").and_then(Value::as_str) == Some("group")
}

/// Index of each node's parent group, in `nodes(doc)` order.
pub fn parents(doc: &Value) -> Vec<Option<usize>> {
	let ns = nodes(doc);
	let groups: Vec<usize> = (0..ns.len()).filter(|&i| is_group(&ns[i])).collect();
	let area = |i: usize| {
		let b = bx(&ns[i]);
		b.w * b.h
	};
	ns.iter()
		.enumerate()
		.map(|(i, n)| {
			groups
				.iter()
				.copied()
				.filter(|&g| g != i && contains(bx(&ns[g]), bx(n)))
				// `min_by` keeps the first of equals, like JS's stable sort.
				.min_by(|&a, &b| area(a).total_cmp(&area(b)))
		})
		.collect()
}

/// Parent group id per node id, for the parity test against `parseCanvas`.
pub fn parent_ids(doc: &Value) -> Vec<(String, Option<String>)> {
	let ns = nodes(doc);
	parents(doc)
		.iter()
		.enumerate()
		.map(|(i, p)| (id_of(&ns[i]).to_string(), p.map(|g| id_of(&ns[g]).to_string())))
		.collect()
}

fn bbox_of<'a>(boxes: impl Iterator<Item = &'a Value>) -> Option<Bx> {
	boxes.map(bx).reduce(|a, b| {
		let (x, y) = (a.x.min(b.x), a.y.min(b.y));
		Bx { x, y, w: (a.x + a.w).max(b.x + b.w) - x, h: (a.y + a.h).max(b.y + b.h) - y }
	})
}

fn bbox_json(b: Option<Bx>) -> Value {
	match b {
		Some(b) => json!({ "x": js(b.x), "y": js(b.y), "w": js(b.w), "h": js(b.h) }),
		None => Value::Null,
	}
}

/// A whole `f64` as an integer, the way JS prints it.
fn js(n: f64) -> Value {
	if n.fract() == 0.0 && n.abs() < 1e15 {
		json!(n as i64)
	} else {
		json!(n)
	}
}

fn colour(n: &Value) -> &str {
	n.get("color").and_then(Value::as_str).unwrap_or("none")
}

/// Title, counts, sections by containment and the loose bucket. `revision` is
/// added by the caller, which knows the file's hash.
pub fn outline(doc: &Value) -> Value {
	let ns = nodes(doc);
	let ps = parents(doc);
	let mut sections = Vec::new();
	for (gi, g) in ns.iter().enumerate().filter(|(_, n)| is_group(n)) {
		let cards: Vec<&Value> = ns
			.iter()
			.enumerate()
			.filter(|(i, n)| ps[*i] == Some(gi) && !is_group(n))
			.map(|(_, n)| n)
			.collect();
		let mut colours = Map::new();
		for c in &cards {
			let n = colours.entry(colour(c)).or_insert(json!(0));
			*n = json!(n.as_i64().unwrap_or(0) + 1);
		}
		sections.push(json!({
			"id": id_of(g),
			"label": g.get("label").and_then(Value::as_str).unwrap_or_default(),
			"parent": ps[gi].map(|p| id_of(&ns[p])),
			"bbox": bbox_json(Some(bx(g))),
			"cards": cards.len(),
			"colors": colours,
		}));
	}
	let loose: Vec<&Value> = ns
		.iter()
		.enumerate()
		.filter(|(i, n)| ps[*i].is_none() && !is_group(n))
		.map(|(_, n)| n)
		.collect();
	json!({
		"title": doc.get("title").cloned().unwrap_or(Value::Null),
		"nodeCount": ns.len(),
		"edgeCount": edges(doc).len(),
		"sections": sections,
		"loose": {
			"count": loose.len(),
			"bbox": bbox_json(bbox_of(loose.iter().copied())),
			"read": "canvasRead({loose:true})",
		},
	})
}

/// Which nodes `read_part` should return.
#[derive(Debug, Default)]
pub struct Selector {
	pub group_id: Option<String>,
	pub node_ids: Vec<String>,
	/// The cards that sit in no group (the outline's `loose` bucket).
	pub loose: bool,
}

const NODE_KEYS: [&str; 13] = [
	"id", "type", "x-kind", "text", "label", "file", "url", "color", "x", "y", "width", "height",
	"x-author",
];

/// The selected nodes (a group selects everything inside it, nested groups
/// included) and the edges that touch them, each marked `out`, `in` or
/// `within` relative to the selection.
pub fn read_part(doc: &Value, sel: &Selector) -> Result<Value, String> {
	if sel.group_id.is_none() && sel.node_ids.is_empty() && !sel.loose {
		return Err("pass groupId, nodeIds or loose; call canvasOutline to see the sections".into());
	}
	let ns = nodes(doc);
	let ps = parents(doc);
	let index = |id: &str| ns.iter().position(|n| id_of(n) == id);
	let mut picked = vec![false; ns.len()];
	if sel.loose {
		for (i, n) in ns.iter().enumerate() {
			picked[i] = ps[i].is_none() && !is_group(n);
		}
	}
	if let Some(gid) = &sel.group_id {
		let g = index(gid)
			.filter(|&i| is_group(&ns[i]))
			.ok_or_else(|| format!("no group {gid}; call canvasOutline for the ids"))?;
		for i in 0..ns.len() {
			let mut p = ps[i];
			while let Some(a) = p {
				if a == g {
					picked[i] = true;
					break;
				}
				p = ps[a];
			}
		}
	}
	for id in &sel.node_ids {
		picked[index(id).ok_or_else(|| format!("no node {id}"))?] = true;
	}
	let chosen: Vec<&Value> = ns.iter().zip(&picked).filter(|(_, p)| **p).map(|(n, _)| n).collect();
	let has = |id: &str| chosen.iter().any(|n| id_of(n) == id);
	let views: Vec<Value> = chosen
		.iter()
		.map(|n| {
			let mut out = Map::new();
			for k in NODE_KEYS {
				if let Some(v) = n.get(k) {
					out.insert(k.into(), v.clone());
				}
			}
			Value::Object(out)
		})
		.collect();
	let touching: Vec<Value> = edges(doc)
		.iter()
		.filter_map(|e| {
			let from = e.get("fromNode").and_then(Value::as_str).unwrap_or_default();
			let to = e.get("toNode").and_then(Value::as_str).unwrap_or_default();
			let dir = match (has(from), has(to)) {
				(true, true) => "within",
				(true, false) => "out",
				(false, true) => "in",
				_ => return None,
			};
			let mut out = e.as_object().cloned().unwrap_or_default();
			out.insert("direction".into(), json!(dir));
			Some(Value::Object(out))
		})
		.collect();
	Ok(json!({ "nodes": views, "edges": touching }))
}

fn overlaps(a: Bx, b: Bx, margin: f64) -> bool {
	a.x < b.x + b.w + margin
		&& a.x + a.w + margin > b.x
		&& a.y < b.y + b.h + margin
		&& a.y + a.h + margin > b.y
}

/// A spot where a `w` x `h` box overlaps no node and keeps 40 px clear (a group
/// only blocks a box that crosses its border). Tries right of and below `near` (or of everything) on a 40 px
/// grid, ring by ring.
///
/// shortcut: a first-fit scan, not a packing; if 60 rings find nothing it goes
/// right of everything. Upgrade if boards get dense enough to notice.
pub fn free_spot(doc: &Value, w: f64, h: f64, near: Option<&str>) -> Result<(i64, i64), String> {
	if !(w > 0.0 && h > 0.0) {
		return Err("width and height must be positive numbers".into());
	}
	let ns = nodes(doc);
	let all = bbox_of(ns.iter());
	if all.is_none() && near.is_none() {
		return Ok((0, 0));
	}
	let anchor = match near {
		Some(id) => bx(ns
			.iter()
			.find(|n| id_of(n) == id && is_group(n))
			.ok_or_else(|| format!("no group {id}; call canvasOutline for the ids"))?),
		None => all.unwrap_or(Bx { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }),
	};
	// A group does not block a box that sits wholly inside it: that is how a
	// card lands in a section. It blocks one that straddles its border.
	let free = |x: f64, y: f64| {
		let me = Bx { x, y, w, h };
		ns.iter().all(|n| {
			let g = bx(n);
			let inside = x >= g.x && y >= g.y && x + w <= g.x + g.w && y + h <= g.y + g.h;
			(is_group(n) && inside) || !overlaps(me, g, GRID)
		})
	};
	let steps = |len: f64| (0..=((len / GRID).ceil() as i64)).map(|i| i as f64 * GRID);
	for ring in 0..RINGS {
		let shift = ring as f64 * GRID;
		let right = anchor.x + anchor.w + GRID + shift;
		for dy in steps(anchor.h) {
			if free(right, anchor.y + dy) {
				return Ok((right.ceil() as i64, (anchor.y + dy).ceil() as i64));
			}
		}
		let below = anchor.y + anchor.h + GRID + shift;
		for dx in steps(anchor.w) {
			if free(anchor.x + dx, below) {
				return Ok(((anchor.x + dx).ceil() as i64, below.ceil() as i64));
			}
		}
	}
	let far = all.map(|b| (b.x + b.w + GRID, b.y)).unwrap_or((0.0, 0.0));
	Ok((far.0.ceil() as i64, far.1.ceil() as i64))
}

/// Whole floats as integers, so `160.0` is written `160` like the editor does.
fn norm(v: &Value) -> Value {
	match v {
		Value::Number(n) => match n.as_f64() {
			Some(f) if n.is_f64() => js(f),
			_ => v.clone(),
		},
		Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
		Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
		_ => v.clone(),
	}
}

fn str_field<'a>(op: &'a Value, key: &str) -> Result<&'a str, String> {
	op.get(key).and_then(Value::as_str).ok_or_else(|| format!("needs a string \"{key}\""))
}

fn object_field<'a>(op: &'a Value, key: &str) -> Result<&'a Map<String, Value>, String> {
	op.get(key).and_then(Value::as_object).ok_or_else(|| format!("needs an object \"{key}\""))
}

/// Apply `ops` in order to a copy of `doc`. All or nothing: the first op that
/// fails aborts the batch and the error names it (`op 2 (updateNode): ...`).
///
/// Ops: `addNode {node}`, `updateNode {id, fields}`, `removeNode {id}`,
/// `addEdge {edge}`, `updateEdge {id, fields}`, `removeEdge {id}`. A `null` in
/// `fields` deletes that key. Nodes added get `"x-author": "agent"`.
pub fn apply_ops(doc: &Value, ops: &Value) -> Result<Value, String> {
	let ops = ops.as_array().ok_or("ops must be an array")?;
	check_arrays(doc)?;
	let mut out = doc.clone();
	let root = out.as_object_mut().ok_or("a canvas must be a JSON object")?;
	let mut ns = take_array(root, "nodes");
	let mut es = take_array(root, "edges");
	for (i, op) in ops.iter().enumerate() {
		let kind = op.get("op").and_then(Value::as_str).unwrap_or("?");
		apply_one(&mut ns, &mut es, kind, op).map_err(|e| format!("op {i} ({kind}): {e}"))?;
	}
	root.insert("nodes".into(), Value::Array(ns));
	root.insert("edges".into(), Value::Array(es));
	Ok(out)
}

/// `nodes` / `edges` may be missing (an empty board) but not another type: the
/// editor refuses such a file, and a tool must not "repair" it by emptying it.
pub fn check_arrays(doc: &Value) -> Result<(), String> {
	for key in ["nodes", "edges"] {
		if doc.get(key).is_some_and(|v| !v.is_array()) {
			return Err(format!("the canvas \"{key}\" is not an array; fix the file by hand"));
		}
	}
	Ok(())
}

fn take_array(root: &mut Map<String, Value>, key: &str) -> Vec<Value> {
	match root.get_mut(key).map(Value::take) {
		Some(Value::Array(a)) => a,
		_ => Vec::new(),
	}
}

fn merge(target: &mut Value, fields: &Map<String, Value>) -> Result<(), String> {
	if fields.contains_key("id") {
		return Err("an id cannot be changed".into());
	}
	let obj = target.as_object_mut().ok_or("not an object")?;
	for (k, v) in fields {
		if v.is_null() {
			obj.shift_remove(k);
		} else {
			obj.insert(k.clone(), norm(v));
		}
	}
	Ok(())
}

fn apply_one(
	ns: &mut Vec<Value>,
	es: &mut Vec<Value>,
	kind: &str,
	op: &Value,
) -> Result<(), String> {
	let has_node = |ns: &[Value], id: &str| ns.iter().any(|n| id_of(n) == id);
	match kind {
		"addNode" => {
			let mut node = norm(&Value::Object(object_field(op, "node")?.clone()));
			let id = node.get("id").and_then(Value::as_str).ok_or("node needs a string \"id\"")?;
			if has_node(ns, id) {
				return Err(format!("node {id} already exists"));
			}
			let o = node.as_object_mut().expect("built from an object");
			for k in ["x", "y"] {
				if !o.get(k).is_some_and(Value::is_number) {
					return Err(format!("node needs a numeric \"{k}\" (see canvasFreeSpot)"));
				}
			}
			o.entry("type").or_insert(json!("text"));
			o.entry("width").or_insert(json!(160));
			o.entry("height").or_insert(json!(160));
			o.insert("x-author".into(), json!("agent"));
			ns.push(node);
		}
		"updateNode" => {
			let id = str_field(op, "id")?;
			let fields = object_field(op, "fields")?;
			// Same strictness as addNode: a node keeps a string type and numeric geometry.
			for k in ["x", "y", "width", "height"] {
				if fields.get(k).is_some_and(|v| v.as_f64().is_none_or(|f| !f.is_finite())) {
					return Err(format!("\"{k}\" must be a number"));
				}
			}
			if fields.get("type").is_some_and(|v| !v.is_string()) {
				return Err("\"type\" must be a string".into());
			}
			let n = ns.iter_mut().find(|n| id_of(n) == id).ok_or(format!("no node {id}"))?;
			merge(n, fields)?;
		}
		"removeNode" => {
			let id = str_field(op, "id")?;
			if !has_node(ns, id) {
				return Err(format!("no node {id}"));
			}
			ns.retain(|n| id_of(n) != id);
			es.retain(|e| {
				e.get("fromNode").and_then(Value::as_str) != Some(id)
					&& e.get("toNode").and_then(Value::as_str) != Some(id)
			});
		}
		"addEdge" => {
			let edge = norm(&Value::Object(object_field(op, "edge")?.clone()));
			let id = str_field(&edge, "id").map_err(|_| "edge needs a string \"id\"")?;
			if es.iter().any(|e| id_of(e) == id) {
				return Err(format!("edge {id} already exists"));
			}
			for end in ["fromNode", "toNode"] {
				let n = str_field(&edge, end)?;
				if !has_node(ns, n) {
					return Err(format!("{end} {n} is not a node"));
				}
			}
			es.push(edge);
		}
		"updateEdge" => {
			let id = str_field(op, "id")?;
			let fields = object_field(op, "fields")?;
			// parseCanvas rejects the whole board for an edge without string ends.
			for end in ["fromNode", "toNode"] {
				if let Some(v) = fields.get(end) {
					let n = v.as_str().ok_or_else(|| format!("{end} must be a node id string"))?;
					if !has_node(ns, n) {
						return Err(format!("{end} {n} is not a node"));
					}
				}
			}
			let e = es.iter_mut().find(|e| id_of(e) == id).ok_or(format!("no edge {id}"))?;
			merge(e, fields)?;
		}
		"removeEdge" => {
			let id = str_field(op, "id")?;
			let before = es.len();
			es.retain(|e| id_of(e) != id);
			if es.len() == before {
				return Err(format!("no edge {id}"));
			}
		}
		other => return Err(format!("unknown op {other}")),
	}
	Ok(())
}

/// The file text, the way the editor writes it: two-space indent, a trailing
/// newline, keys in the order they were read (`serializeCanvas`).
pub fn to_text(doc: &Value) -> String {
	// A `Value` always serializes.
	format!("{}\n", serde_json::to_string_pretty(doc).unwrap_or_default())
}

#[cfg(test)]
mod tests {
	use super::*;

	const FIXTURE: &str = include_str!(
		"../../../../apps/desktop/src/components/canvas/__fixtures__/containment.canvas"
	);

	fn doc() -> Value {
		serde_json::from_str(FIXTURE).unwrap()
	}

	#[test]
	fn containment_matches_the_shared_fixture() {
		let got: Vec<(String, Option<String>)> = parent_ids(&doc());
		let want = [
			("g-big", None),
			("g-small", Some("g-big")),
			("n-in-small", Some("g-small")),
			("n-in-big", Some("g-big")),
			("n-in-big-2", Some("g-big")),
			("n-straddle", None),
			("n-loose", None),
		];
		let want: Vec<(String, Option<String>)> =
			want.iter().map(|(n, p)| (n.to_string(), p.map(str::to_string))).collect();
		assert_eq!(got, want);
	}

	#[test]
	fn outline_counts_cards_and_colours_per_section() {
		let o = outline(&doc());
		assert_eq!(o["nodeCount"], 7);
		assert_eq!(o["edgeCount"], 2);
		let big = &o["sections"][0];
		assert_eq!(big["id"], "g-big");
		assert_eq!(big["cards"], 2);
		assert_eq!(big["colors"], json!({ "2": 1, "none": 1 }));
		let small = &o["sections"][1];
		assert_eq!(small["parent"], "g-big");
		assert_eq!(small["colors"], json!({ "1": 1 }));
		assert_eq!(o["loose"]["count"], 2);
		assert_eq!(o["loose"]["bbox"], json!({ "x": 950, "y": 0, "w": 1150, "h": 160 }));
	}

	#[test]
	fn read_part_by_group_returns_nested_members_and_edges_both_ways() {
		let sel = Selector { group_id: Some("g-small".into()), ..Default::default() };
		let p = read_part(&doc(), &sel).unwrap();
		assert_eq!(p["nodes"].as_array().unwrap().len(), 1);
		let dirs: Vec<(&str, &str)> = p["edges"]
			.as_array()
			.unwrap()
			.iter()
			.map(|e| (e["id"].as_str().unwrap(), e["direction"].as_str().unwrap()))
			.collect();
		assert_eq!(dirs, [("e1", "out"), ("e2", "in")]);

		let big = Selector { group_id: Some("g-big".into()), ..Default::default() };
		let part = read_part(&doc(), &big).unwrap();
		let ids: Vec<&str> =
			part["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
		assert_eq!(ids, ["g-small", "n-in-small", "n-in-big", "n-in-big-2"]);
	}

	#[test]
	fn read_part_needs_a_selector_and_known_ids() {
		assert!(read_part(&doc(), &Selector::default()).unwrap_err().contains("canvasOutline"));
		let sel = Selector { node_ids: vec!["nope".into()], ..Default::default() };
		assert!(read_part(&doc(), &sel).is_err());
	}

	#[test]
	fn free_spot_overlaps_nothing_and_goes_next_to_near() {
		let d = doc();
		let clear = |x: i64, y: i64| {
			let me = Bx { x: x as f64, y: y as f64, w: 200.0, h: 100.0 };
			nodes(&d).iter().filter(|n| !is_group(n)).all(|n| !overlaps(me, bx(n), GRID))
		};
		let (x, y) = free_spot(&d, 200.0, 100.0, None).unwrap();
		assert!(clear(x, y), "({x},{y})");

		let (x, y) = free_spot(&d, 200.0, 100.0, Some("g-small")).unwrap();
		assert!(clear(x, y), "({x},{y})");
		// g-small spans x 100..500, y 100..400; the spot hugs it.
		assert!((100..=540).contains(&x) && (100..=440).contains(&y), "({x},{y})");
		assert!(free_spot(&d, 10.0, 10.0, Some("n-loose")).is_err());
	}

	#[test]
	fn free_spot_on_an_empty_canvas_is_the_origin() {
		assert_eq!(free_spot(&json!({}), 100.0, 100.0, None).unwrap(), (0, 0));
	}

	#[test]
	fn apply_ops_keeps_unknown_fields_and_marks_the_author() {
		let mut d = doc();
		d["x-future"] = json!({ "keep": true });
		d["nodes"][2]["x-note"] = json!("mine");
		let ops = json!([
			{ "op": "addNode", "node": { "id": "new", "x": 1.0, "y": 2, "text": "hi", "x-author": "human", "x-extra": [1] } },
			{ "op": "updateNode", "id": "n-in-small", "fields": { "text": "changed", "color": null } },
			{ "op": "addEdge", "edge": { "id": "e3", "fromNode": "new", "toNode": "n-loose", "label": "x" } },
		]);
		let out = apply_ops(&d, &ops).unwrap();
		assert_eq!(out["x-future"], json!({ "keep": true }));
		let n = &out["nodes"][2];
		assert_eq!((n["text"].as_str(), n["x-note"].as_str()), (Some("changed"), Some("mine")));
		assert!(n.get("color").is_none());
		let added = out["nodes"].as_array().unwrap().last().unwrap();
		assert_eq!(added["x-author"], "agent");
		assert_eq!(added["x-extra"], json!([1]));
		assert_eq!((added["type"].as_str(), added["width"].as_i64()), (Some("text"), Some(160)));
		assert!(to_text(&out).contains("\"x\": 1,"), "whole floats print as integers");
		assert_eq!(out["edges"].as_array().unwrap().len(), 3);
	}

	#[test]
	fn a_failing_batch_names_the_op_and_changes_nothing() {
		let d = doc();
		let before = d.clone();
		let ops = json!([
			{ "op": "removeNode", "id": "n-loose" },
			{ "op": "updateNode", "id": "ghost", "fields": {} },
		]);
		let err = apply_ops(&d, &ops).unwrap_err();
		assert!(err.starts_with("op 1 (updateNode)") && err.contains("ghost"), "{err}");
		assert_eq!(d, before);
		for bad in [
			json!([{ "op": "addNode", "node": { "id": "n-loose", "x": 0, "y": 0 } }]),
			json!([{ "op": "addEdge", "edge": { "id": "z", "fromNode": "n-loose", "toNode": "nope" } }]),
			json!([{ "op": "updateNode", "id": "n-loose", "fields": { "id": "other" } }]),
			json!([{ "op": "frobnicate" }]),
		] {
			assert!(apply_ops(&d, &bad).is_err(), "{bad}");
		}
	}

	#[test]
	fn update_edge_null_endpoint_fails_batch() {
		let ops = json!([{ "op": "updateEdge", "id": "e1", "fields": { "fromNode": null } }]);
		let err = apply_ops(&doc(), &ops).unwrap_err();
		assert!(err.starts_with("op 0 (updateEdge)") && err.contains("fromNode"), "{err}");
	}

	#[test]
	fn update_edge_numeric_endpoint_fails_batch() {
		let ops = json!([{ "op": "updateEdge", "id": "e1", "fields": { "toNode": 7 } }]);
		assert!(apply_ops(&doc(), &ops).unwrap_err().contains("toNode"));
	}

	#[test]
	fn update_node_null_x_fails_batch() {
		for fields in [json!({ "x": null }), json!({ "height": "tall" }), json!({ "type": null })] {
			let ops = json!([{ "op": "updateNode", "id": "n-loose", "fields": fields }]);
			assert!(apply_ops(&doc(), &ops).is_err(), "{fields}");
		}
	}

	#[test]
	fn a_non_array_nodes_is_an_error_not_an_empty_board() {
		let d = json!({ "nodes": "oops", "edges": [] });
		assert!(check_arrays(&d).is_err());
		assert!(apply_ops(&d, &json!([])).unwrap_err().contains("nodes"));
		assert!(check_arrays(&json!({})).is_ok());
	}

	#[test]
	fn read_loose_returns_ungrouped_nodes() {
		let sel = Selector { loose: true, ..Default::default() };
		let p = read_part(&doc(), &sel).unwrap();
		let ids: Vec<&str> =
			p["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
		assert_eq!(ids, ["n-straddle", "n-loose"]);
		assert!(p["edges"].as_array().unwrap().iter().any(|e| e["id"] == "e2"));
	}

	#[test]
	fn remove_node_removes_its_edges() {
		let out = apply_ops(&doc(), &json!([{ "op": "removeNode", "id": "n-in-small" }])).unwrap();
		assert!(out["edges"].as_array().unwrap().is_empty());
		assert_eq!(out["nodes"].as_array().unwrap().len(), 6);
	}

	#[test]
	fn text_matches_the_editors_serialization() {
		// Round trip is the identity on a file the editor wrote: no spurious diff.
		let text = FIXTURE.to_string();
		let reparsed = to_text(&serde_json::from_str::<Value>(&text).unwrap());
		assert_eq!(to_text(&serde_json::from_str::<Value>(&reparsed).unwrap()), reparsed);
		assert!(reparsed.starts_with("{\n  \"title\": \"Containment\",\n  \"nodes\": [\n    {\n"));
		assert!(reparsed.ends_with("}\n"));
	}
}
