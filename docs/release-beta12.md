# AEGIS 0.7.0-beta.12

## Screening swarm

The AI path can now screen the market with a swarm instead of a single request.

- **Scouts.** Up to six scouts read the same candles through different lenses — trend,
  structure, momentum, mean-reversion, volatility, risk — and each proposes a setup or
  declines.
- **Candidates.** A proposal is dropped unless its stop is a real invalidation level: it
  must clear the far side of the spread and sit 0.01%…5% from entry. Proposals describing
  the same setup merge and record how many scouts back them.
- **Quorum and vote.** A candidate needs `swarm_quorum` scouts before a panel of voters
  (skeptic, risk-manager, trend-follower, contrarian, liquidity) sees it, then
  `swarm_vote_threshold` votes and `swarm_min_confidence` mean confidence to act.
- **Sizing.** Quantity is derived from `max_risk_pct` and `max_leverage` against current
  exposure, so a swarm entry reaches the risk gates already legal.

`swarm_scouts` defaults to `0`: an existing installation keeps the single-request path
until the swarm is switched on in Settings → AI.

## Budget and cancellation

- The cloud reservation now covers the whole round (scouts plus voters). A multi-request
  round could previously pass the pre-flight check and then overspend the daily or
  monthly budget.
- Scouts and voters run in a `JoinSet`, so Stop aborts them instead of leaving paid
  requests in flight.

## Under the hood

- Model transport moved into `ai::post_chat`; the swarm and the single-request path share
  one policy (no redirects, no proxy for a local model, bounded responses).
- `Snapshot::validate` is available inside the crate so the swarm runs the same snapshot
  gate as the single request.

## Known limits

- The swarm has not been run against a live model or a live broker. Enable it on Paper
  first.
- Without a local model, a swarm round costs up to `swarm_scouts + swarm_voters` cloud
  requests instead of one.

Full details: `docs/ai-swarm.md`. See `CHANGELOG.md` for earlier changes.
