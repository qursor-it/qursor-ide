# ADR-0106 — Production servers wear a colour nothing else uses

**Date.** 2026-11-19
**Status.** Accepted. Adds a signal colour to `DESIGN.md`. Arises from F44.

## Context

A server marked production must be recognisable at a glance in the address
book, on its tab and around its terminal — that is where a wrong command costs
most. Every existing signal colour already has its one job: Build Violet is the
dev build, Usage Amber a plan limit getting close, Destructive Red destroying
something, Qursor Blue the human's turn, Session Green a working agent.

## Decision

**Production Rose** (`--production`, hue 350) is a new token whose single job is
"this server is production": the `PROD` word on its row and tab, a 2px rule on
top of its tab, the border of its card. It never fills a row or a background.

## Consequences

- Agent policy for production servers starts at "confirm every command" (F45);
  the colour says where you are, the policy decides what an agent may do.
