# Project Intelligence fixtures

Synthetic transcripts for the evidence extractor and the analysis tests
(`services/intelligence`, F33). **Hand-written; nothing here comes from a real
session**, and none of it may be replaced by one: real transcripts carry paths,
code and secrets.

- `claude-session-a.jsonl` — a Claude Code session in the shape the indexer
  reads (`user` / `assistant` records, `tool_use` and `tool_result` blocks).
  Holds: one real human correction; a tool result that talks like a person; a
  failing `pnpm test` and its passing re-run; a failing `cat` that is not a
  check; a repeated instruction; and the records that look like a user turn and
  are not (a sidechain prompt, a meta caveat, a slash command, an interrupt).
- `claude-session-b.jsonl` — a second session correcting the same thing in other
  words, with a prompt-injection attempt in the correction text.
- `codex-rollout-a.jsonl` — a Codex rollout in the shape `agents/codex.rs`
  reads, plus the tool-call records (`function_call`, `function_call_output`,
  `exec_command_end`) that reader drops. The tool shapes are modelled on how the
  CLI is documented to write them and are **not** checked against a live
  rollout (F33 § "Not verified").
