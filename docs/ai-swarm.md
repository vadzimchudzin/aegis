# Screening swarm

The AI path has two shapes. By default one model is asked one question per round
(`ai::request_decision`). With `swarm_scouts` set to two or more, a round becomes a
small swarm: several scouts screen the market through different lenses, and a panel of
voters decides whether any of their proposals is worth acting on.

The swarm is advisory. It produces an ordinary `Decision`, which still goes through the
paper engine or the live risk gates. Those stay the only authority on size, stops and
exposure.

## A round

```
  scouts (parallel)            filter                panel (parallel)          decision
  ─────────────────            ──────                ────────────────          ────────
  trend                        stop must be a        skeptic                    one clear winner
  structure        ─────────►  real invalidation ──► risk-manager   ────────►   + vote threshold
  momentum                     level, inside        trend-follower             + confidence floor
  mean-reversion               0.01% … 5%           contrarian                 + risk budget room
  volatility                   ──                     liquidity                 ↓
  risk                         merge duplicates                                 Open or stand down
                               apply quorum
```

1. **Scouts.** Each gets one lens and the same market evidence (quote, candles, trimmed
   journal, risk limits). A lens is a narrow question — "is the series making progress in
   one direction" — not "find a setup", because a broad question invites invention.
2. **Filtering.** A report is dropped unless `setup` is `long` or `short`, `confidence`
   is within 0…100, and `stop` is a real invalidation level: strictly on the loss side of
   the entry, at least 0.01% and at most 5% away. Entry is never taken from the model —
   it is the current ask for a long and the current bid for a short, so a scout cannot
   invent a better price.
3. **Candidates.** Proposals with the same side and a stop within 0.05% count as the same
   setup: they merge and record how many scouts back them.
4. **Quorum.** `swarm_quorum` supporters are required before the panel sees a candidate.
   A single opinion never reaches a vote.
5. **Panel.** `swarm_voters` voters, each with its own mandate. The panel sees the
   candidates labelled A, B, C with their stop, support count and thesis.
6. **Decision.** Exactly one clear winner is required — a tie stands the round down.
   The winner needs `swarm_vote_threshold` votes and `swarm_min_confidence` mean
   confidence among the voters that backed it.
7. **Size.** `size_quantity` spends what is left of `max_risk_pct` on the distance to the
   winning stop, capped by the remaining `max_leverage` room. If either budget is
   exhausted the round stands down instead of proposing an unsized entry.

## Settings

| Field | Default | Meaning |
|---|---|---|
| `swarm_scouts` | `0` | `0` or `1` keeps the single-request path. Max 6. |
| `swarm_voters` | `3` | Panel size, 1…5. |
| `swarm_quorum` | `2` | Scouts that must back a candidate before voting. |
| `swarm_vote_threshold` | `2` | Votes the winner needs. |
| `swarm_min_confidence` | `55` | Mean confidence of the winning voters, percent. |

`AiSettings::validate` rejects a configuration where the quorum exceeds the scout count,
the threshold exceeds the panel size, or the confidence floor is outside 0…100.

## Cost

One round makes up to `swarm_scouts + swarm_voters` requests, in parallel. Two
consequences:

- The cloud budget reservation is multiplied by that count (`round_requests` in
  `app/src-tauri/src/ai_cmd.rs`). Without it a swarm would pass the pre-flight check and
  then overspend the daily or monthly budget.
- The round returns the summed reported cost, and the caller settles the difference
  against the reservation exactly as it does for a single request.

Scouts and voters run in a `tokio::task::JoinSet`, so dropping the round — Stop or the
epoch cancel — aborts the tasks rather than leaving paid requests in flight. A request
already accepted by the provider may still be billed; that limitation is the same one the
single-request path documents.

## Why the panel is mandates, not samples

With one endpoint and `temperature: 0`, N identical prompts return N identical answers, so
"voting" over them would measure nothing. Each voter therefore applies a different
standard (skeptic, risk-manager, trend-follower, contrarian, liquidity). The votes are
honestly an ensemble of framings, not independent samples; with several models behind the
endpoint it would be both.

## Tests

`crates/aegis-core/src/ai_swarm.rs` covers the pure logic: stop-side validation,
implausible and missing stops, `none` rejection, merging and support counts, opposite
sides staying separate, quorum, ties, votes outside the board, sizing against both
budgets, fail-closed sizing, fenced and extra-key reply parsing, local-vs-cloud stand-down
and cost summation.

The network path is not mocked separately: scouts and voters call `ai::post_chat`, which
is covered by the existing mock-endpoint tests in `ai.rs`. That is intentional — one
transport, one set of tests.

## Known limits

- No live-model or live-broker run yet. Enable the swarm on Paper first.
- The scout lenses are prompts, not indicators; they reason over the same candles the
  single-request path receives, with no order-book depth or flow.
- A cloud round that finds nothing returns `Wait`; only a local round escalates with
  `Consult`, because there is nothing above the cloud model to escalate to.
