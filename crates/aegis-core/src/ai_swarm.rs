//! Screening swarm: several scouts scan the market through different lenses in parallel, the
//! surviving setups are grouped into candidates, and a panel of voters decides whether any of them
//! is worth acting on.
//!
//! The swarm is advisory only. It emits an ordinary [`Decision`], which still travels through the
//! paper/live risk gates; those remain the only authority on size, stops and exposure. Every
//! network call goes through [`crate::ai::post_chat`], so the swarm inherits the same transport
//! policy (no redirects, no proxy for a local model, bounded responses) as the single-shot path.

use crate::ai::{
    chat_payload, current_risk_total, market_context, position_notional_total, post_chat,
    AiSettings, Decision, PaperState, Snapshot,
};
use serde::Deserialize;

/// Upper bound on parallel scouts. Each scout is one request, so this is also the per-round cost
/// multiplier the caller has to reserve budget for.
pub const MAX_SCOUTS: usize = 6;

/// Upper bound on the voting panel.
pub const MAX_VOTERS: usize = 5;

/// A stop closer than this to entry would be noise from the quote spread, not a level.
const MIN_STOP_DISTANCE_PCT: f64 = 0.01;

/// A stop further than this from entry is not an invalidation level, it is a guess.
const MAX_STOP_DISTANCE_PCT: f64 = 5.0;

/// Two scouts describing "the same setup" may differ in the exact stop by this fraction.
const STOP_AGREEMENT_TOLERANCE: f64 = 0.0005;

const LABELS: [&str; 3] = ["A", "B", "C"];

struct Lens {
    name: &'static str,
    brief: &'static str,
}

/// Each scout reads the same candles through a different question. The briefs are deliberately
/// narrow: a scout that is asked to "find a setup" invents one, a scout that is asked to check a
/// specific property has to answer that property.
const SCOUT_LENSES: [Lens; MAX_SCOUTS] = [
    Lens {
        name: "trend",
        brief: "Is the recent series making consistent progress in one direction, measured on candle closes rather than single wicks? A setup needs most of the recent candles to agree on direction.",
    },
    Lens {
        name: "structure",
        brief: "Locate the recent swing highs and swing lows. Does price sit near an edge of that range, or in the middle? A setup near the middle of a range has no invalidation level worth using.",
    },
    Lens {
        name: "momentum",
        brief: "Compare the size of recent up candles with recent down candles. Is directional pressure building or fading? Fading pressure argues against a fresh entry in the direction of the last move.",
    },
    Lens {
        name: "mean-reversion",
        brief: "Is price extended away from the recent average of closes, or already back at it? Only an extension can justify fading; at the average there is nothing to fade.",
    },
    Lens {
        name: "volatility",
        brief: "Measure the typical candle range over the recent window. Is the current range wide enough that a stop outside the noise is still within a sane distance, or so wide that any stop is too expensive?",
    },
    Lens {
        name: "risk",
        brief: "Ignore direction quality and ask instead: where would a stop go so that being wrong is cheap and unambiguous? Reject any idea whose invalidation level is unclear or sits inside the recent noise.",
    },
];

/// The panel is an ensemble of mandates, not of models: with a single endpoint and temperature 0,
/// six identical prompts would return six identical votes. Each mandate is a different standard the
/// candidate has to clear.
const VOTER_LENSES: [Lens; MAX_VOTERS] = [
    Lens {
        name: "skeptic",
        brief: "Vote only for a setup whose invalidation level is unambiguous and whose direction is supported by most of the recent candles. Reject when the evidence is thin or mixed.",
    },
    Lens {
        name: "risk-manager",
        brief: "Judge only the risk side: is the stop outside the recent noise but close enough that the trade is worth its cost? Reject setups whose stop is inside the noise or unreasonably far.",
    },
    Lens {
        name: "trend-follower",
        brief: "Vote for a setup that continues the dominant direction of the recent candles. Reject counter-trend setups and setups that contradict the last candles.",
    },
    Lens {
        name: "contrarian",
        brief: "Vote for a setup only when price is genuinely extended from the recent average and the trade fades that extension. Reject continuation setups that are not at an extreme.",
    },
    Lens {
        name: "liquidity",
        brief: "Vote only for a setup whose stop sits beyond a level other participants would also defend, such as the recent swing, rather than at an arbitrary distance.",
    },
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScoutReport {
    setup: String,
    confidence: f64,
    #[serde(default)]
    stop: Option<f64>,
    /// Part of the reply schema, so a model that reports one is not rejected, but the decision
    /// schema has no target field and nothing downstream consumes it.
    #[serde(default)]
    #[allow(dead_code)]
    target: Option<f64>,
    thesis: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VoteReport {
    vote: String,
    confidence: f64,
    /// Kept because the panel is asked to justify its vote and a reply carrying a reason must parse.
    #[allow(dead_code)]
    reason: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    side: &'static str,
    stop: f64,
    support: usize,
    confidence: f64,
    thesis: String,
}

/// What one swarm round produced, including the evidence behind it so the caller can show why a
/// decision was taken rather than only what it was.
#[derive(Clone, Debug)]
pub struct SwarmResult {
    pub decision: Decision,
    pub cost: Option<f64>,
    pub scouts_run: usize,
    pub scouts_failed: usize,
    pub candidates: usize,
    pub winner_votes: usize,
    pub voters_run: usize,
}

/// Everything one round shares with its tasks. Cloned per task so each request owns its inputs.
#[derive(Clone)]
struct Round {
    settings: AiSettings,
    snapshot: Snapshot,
    state: PaperState,
    api_key: Option<String>,
    cloud: bool,
}

/// Run one screening round. `cloud` selects the endpoint exactly as the single-shot path does, and
/// the returned cost is the sum the round actually reported so the caller can settle its budget.
pub async fn request_swarm_decision(
    settings: &AiSettings,
    snapshot: &Snapshot,
    state: &PaperState,
    api_key: Option<&str>,
    cloud: bool,
) -> Result<SwarmResult, String> {
    settings.validate()?;
    snapshot.validate()?;
    let scouts = settings.swarm_scouts.min(MAX_SCOUTS);
    if scouts < 2 {
        return Err("the swarm needs at least two scouts".into());
    }

    let mut spend = Costs::default();
    let mut failed = 0usize;
    let mut candidates: Vec<Candidate> = Vec::new();

    let round = Round {
        settings: settings.clone(),
        snapshot: snapshot.clone(),
        state: state.clone(),
        api_key: api_key.map(str::to_owned),
        cloud,
    };
    let mut handles = tokio::task::JoinSet::new();
    for lens in SCOUT_LENSES.iter().take(scouts) {
        let round = round.clone();
        handles.spawn(async move { scout_once(&round, lens.name, lens.brief).await });
    }
    while let Some(outcome) = handles.join_next().await {
        match outcome {
            Ok(Ok((report, cost))) => {
                spend.add(cost);
                if let Some(candidate) = candidate_from(&report, snapshot) {
                    merge_candidate(&mut candidates, candidate);
                }
            }
            _ => failed += 1,
        }
    }

    let quorum = settings.swarm_quorum.clamp(1, scouts);
    apply_quorum(&mut candidates, quorum);
    if candidates.is_empty() {
        let reason = format!("swarm: no setup met the {quorum}-of-{scouts} scout quorum");
        return Ok(stand_down(cloud, snapshot.id, reason, spend, scouts, failed, 0, 0));
    }

    candidates.truncate(LABELS.len());
    let board = candidate_board(&candidates);

    let voters = settings.swarm_voters.clamp(1, MAX_VOTERS);
    let mut panel = tokio::task::JoinSet::new();
    for lens in VOTER_LENSES.iter().take(voters) {
        let round = round.clone();
        let board = board.clone();
        panel.spawn(async move { vote_once(&round, lens.name, lens.brief, &board).await });
    }

    let mut counts = vec![0usize; candidates.len()];
    let mut confidence = vec![0.0f64; candidates.len()];
    let mut voters_run = 0usize;
    while let Some(outcome) = panel.join_next().await {
        if let Ok(Ok((report, cost))) = outcome {
            voters_run += 1;
            spend.add(cost);
            if let Some(index) = vote_index(&report.vote, candidates.len()) {
                counts[index] += 1;
                confidence[index] += if report.confidence.is_finite() {
                    report.confidence.clamp(0.0, 100.0)
                } else {
                    0.0
                };
            }
        }
    }

    let Some(winner) = pick_winner(&counts) else {
        let reason = format!("swarm: panel split, {voters_run} of {voters} votes counted");
        return Ok(stand_down(cloud, snapshot.id, reason, spend, scouts, failed, candidates.len(), 0));
    };
    let votes = counts[winner];
    let mean_confidence = confidence[winner] / votes as f64;
    let threshold = settings.swarm_vote_threshold.clamp(1, voters);
    if votes < threshold {
        let reason = format!("swarm: candidate {} got {votes} of {voters} votes; {threshold} required", LABELS[winner]);
        return Ok(stand_down(cloud, snapshot.id, reason, spend, scouts, failed, candidates.len(), votes));
    }
    if mean_confidence < settings.swarm_min_confidence {
        let reason = format!("swarm: candidate {} won on {mean_confidence:.0}% confidence", LABELS[winner]);
        return Ok(stand_down(cloud, snapshot.id, reason, spend, scouts, failed, candidates.len(), votes));
    }

    let winner_candidate = &candidates[winner];
    let sized = size_quantity(settings, snapshot, state, winner_candidate.side, winner_candidate.stop);
    let Some(quantity) = sized else {
        let reason = format!("swarm: candidate {} won but the risk budget is exhausted", LABELS[winner]);
        return Ok(stand_down(cloud, snapshot.id, reason, spend, scouts, failed, candidates.len(), votes));
    };

    let reason = format!(
        "swarm: {votes}/{voters} voters backed {} ({:.0}% mean confidence): {}",
        LABELS[winner],
        mean_confidence,
        winner_candidate.thesis
    );
    Ok(SwarmResult {
        decision: Decision::Open {
            snapshot_id: snapshot.id,
            side: winner_candidate.side.to_string(),
            quantity,
            stop: winner_candidate.stop,
            reason: truncate(&reason, 2_000),
        },
        cost: spend.total(),
        scouts_run: scouts,
        scouts_failed: failed,
        candidates: candidates.len(),
        winner_votes: votes,
        voters_run,
    })
}

#[derive(Default)]
struct Costs {
    sum: f64,
    seen: bool,
}

impl Costs {
    fn add(&mut self, cost: Option<f64>) {
        if let Some(cost) = cost.filter(|c| c.is_finite() && *c >= 0.0) {
            self.sum += cost;
            self.seen = true;
        }
    }

    fn total(&self) -> Option<f64> {
        self.seen.then_some(self.sum)
    }
}

#[allow(clippy::too_many_arguments)]
fn stand_down(
    cloud: bool,
    snapshot_id: u64,
    reason: String,
    cost: Costs,
    scouts_run: usize,
    scouts_failed: usize,
    candidates: usize,
    winner_votes: usize,
) -> SwarmResult {
    let reason = truncate(&reason, 2_000);
    // A local round that finds nothing is the existing signal to escalate to the cloud model; a
    // cloud round cannot escalate further and simply stands aside.
    let decision = if cloud {
        Decision::Wait {
            snapshot_id,
            reason,
        }
    } else {
        Decision::Consult {
            snapshot_id,
            reason,
        }
    };
    SwarmResult {
        decision,
        cost: cost.total(),
        scouts_run,
        scouts_failed,
        candidates,
        winner_votes,
        voters_run: 0,
    }
}

async fn scout_once(
    round: &Round,
    name: &str,
    brief: &str,
) -> Result<(ScoutReport, Option<f64>), String> {
    let system = scout_prompt(name, brief, round.cloud);
    let mut user = market_context(&round.settings, &round.snapshot, &round.state);
    user["swarm"] = serde_json::json!({
        "role": "scout",
        "lens": name,
        "instruction": brief,
        "entry": { "long": round.snapshot.ask, "short": round.snapshot.bid },
    });
    let model = if round.cloud {
        &round.settings.cloud_model
    } else {
        &round.settings.local_model
    };
    let body = chat_payload(model, &system, user, !round.cloud)?;
    let key = round.api_key.as_deref();
    let (text, cost) = post_chat(&round.settings, body, key, round.cloud).await?;
    let report: ScoutReport = parse_json(&text)?;
    Ok((report, cost))
}

async fn vote_once(
    round: &Round,
    name: &str,
    mandate: &str,
    board: &serde_json::Value,
) -> Result<(VoteReport, Option<f64>), String> {
    let system = voter_prompt(name, mandate, round.cloud);
    let mut user = market_context(&round.settings, &round.snapshot, &round.state);
    user["swarm"] = serde_json::json!({
        "role": "voter",
        "mandate": name,
        "standard": mandate,
        "candidates": board,
    });
    let model = if round.cloud {
        &round.settings.cloud_model
    } else {
        &round.settings.local_model
    };
    let body = chat_payload(model, &system, user, !round.cloud)?;
    let key = round.api_key.as_deref();
    let (text, cost) = post_chat(&round.settings, body, key, round.cloud).await?;
    let report: VoteReport = parse_json(&text)?;
    Ok((report, cost))
}

fn scout_prompt(name: &str, brief: &str, cloud: bool) -> String {
    format!(
        "{}You are the \"{}\" scout on a paper-trading desk. PAPER TRADING ONLY: you suggest setups, you never claim to place orders.\n\
         Your lens: {}\n\
         Available market data is limited to bid/ask and OHLCV candles. There is no order-book depth, no order flow, no news and no indicator series beyond what you can compute from those candles, so do not invent data or cite indicators you cannot derive.\n\
         Answer with exactly one JSON object and nothing else, with no extra keys:\n\
         {{\"setup\":\"long\" or \"short\" or \"none\",\"confidence\":number 0-100,\"stop\":number,\"target\":number,\"thesis\":string up to 240 characters}}\n\
         \"stop\" is the absolute price at which the idea is invalidated: strictly below entry for long, strictly above it for short, at least 0.01% and at most 5% away from entry. \"target\" is the price you expect price to reach.\n\
         Return setup \"none\" with confidence 0 when your lens does not support a trade. Never force a setup to be helpful.",
        if cloud { "" } else { "/no_think\n" },
        name,
        brief
    )
}

fn voter_prompt(name: &str, mandate: &str, cloud: bool) -> String {
    format!(
        "{}You are the \"{}\" voter on a paper-trading desk reviewing setups proposed by other scouts. PAPER TRADING ONLY: you never claim to place orders.\n\
         Your standard: {}\n\
         The candidates were produced from bid/ask and OHLCV candles only. There is no order-book depth, no order flow and no news, so do not assume information that is not in the data.\n\
         Answer with exactly one JSON object and nothing else, with no extra keys:\n\
         {{\"vote\":\"A\" or \"B\" or \"C\" or \"reject\",\"confidence\":number 0-100,\"reason\":string up to 240 characters}}\n\
         Vote for the single label of the candidate that best clears your standard. Vote \"reject\" when no candidate does. Do not vote for a label that is not on the board.",
        if cloud { "" } else { "/no_think\n" },
        name,
        mandate
    )
}

fn candidate_board(candidates: &[Candidate]) -> serde_json::Value {
    serde_json::Value::Array(
        candidates
            .iter()
            .take(LABELS.len())
            .enumerate()
            .map(|(index, candidate)| {
                serde_json::json!({
                    "label": LABELS[index],
                    "side": candidate.side,
                    "stop": candidate.stop,
                    "supporting_scouts": candidate.support,
                    "thesis": candidate.thesis,
                })
            })
            .collect(),
    )
}

fn candidate_from(report: &ScoutReport, snapshot: &Snapshot) -> Option<Candidate> {
    let side = match report.setup.trim() {
        "long" => "long",
        "short" => "short",
        _ => return None,
    };
    if !report.confidence.is_finite() || !(0.0..=100.0).contains(&report.confidence) {
        return None;
    }
    let stop = report.stop?;
    if !stop.is_finite() || stop <= 0.0 {
        return None;
    }
    let entry = if side == "long" { snapshot.ask } else { snapshot.bid };
    let distance_pct = if side == "long" {
        if stop >= entry {
            return None;
        }
        (entry - stop) / entry * 100.0
    } else {
        if stop <= entry {
            return None;
        }
        (stop - entry) / entry * 100.0
    };
    if !(MIN_STOP_DISTANCE_PCT..=MAX_STOP_DISTANCE_PCT).contains(&distance_pct) {
        return None;
    }
    Some(Candidate {
        side,
        stop,
        support: 1,
        confidence: report.confidence,
        thesis: truncate(&report.thesis, 240),
    })
}

fn merge_candidate(candidates: &mut Vec<Candidate>, incoming: Candidate) {
    for existing in candidates.iter_mut() {
        if existing.side == incoming.side
            && (existing.stop - incoming.stop).abs() <= existing.stop * STOP_AGREEMENT_TOLERANCE
        {
            existing.support += 1;
            if incoming.confidence > existing.confidence {
                existing.confidence = incoming.confidence;
                existing.thesis = incoming.thesis;
            }
            return;
        }
    }
    candidates.push(incoming);
}

fn apply_quorum(candidates: &mut Vec<Candidate>, quorum: usize) {
    candidates.retain(|candidate| candidate.support >= quorum);
    candidates.sort_by(|a, b| {
        b.support
            .cmp(&a.support)
            .then(b.confidence.total_cmp(&a.confidence))
    });
}

fn vote_index(vote: &str, candidates: usize) -> Option<usize> {
    LABELS
        .iter()
        .take(candidates)
        .position(|label| label.eq_ignore_ascii_case(vote.trim()))
}

/// The winner is the single candidate with the most votes; a tie is not a decision.
fn pick_winner(counts: &[usize]) -> Option<usize> {
    let best = counts.iter().copied().max().unwrap_or(0);
    if best == 0 {
        return None;
    }
    let leaders = counts.iter().filter(|count| **count == best).count();
    if leaders != 1 {
        return None;
    }
    counts.iter().position(|count| *count == best)
}

/// Size the position from the settings the risk gates use, so a swarm entry arrives already legal
/// instead of being rejected by the engine for the size its own scouts implied.
fn size_quantity(
    settings: &AiSettings,
    snapshot: &Snapshot,
    state: &PaperState,
    side: &str,
    stop: f64,
) -> Option<f64> {
    let entry = if side == "long" { snapshot.ask } else { snapshot.bid };
    if !entry.is_finite() || entry <= 0.0 {
        return None;
    }
    let per_unit = if side == "long" { entry - stop } else { stop - entry };
    if !per_unit.is_finite() || per_unit <= 0.0 {
        return None;
    }
    let equity = state.equity;
    if !equity.is_finite() || equity <= 0.0 {
        return None;
    }
    let risk_budget = equity * settings.max_risk_pct / 100.0 - current_risk_total(state);
    let leverage_room = equity * settings.max_leverage - position_notional_total(state, Some(snapshot));
    if !risk_budget.is_finite() || !leverage_room.is_finite() || risk_budget <= 0.0 || leverage_room <= 0.0
    {
        return None;
    }
    let quantity = (risk_budget / per_unit).min(leverage_room / entry);
    (quantity.is_finite() && quantity > 0.0).then_some(quantity)
}

fn parse_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<T>(trimmed) {
        return Ok(value);
    }
    // Some endpoints wrap the object in a markdown fence despite the instruction against it.
    let start = trimmed.find('{').ok_or_else(|| "swarm reply has no JSON object".to_string())?;
    let end = trimmed.rfind('}').ok_or_else(|| "swarm reply has no JSON object".to_string())?;
    if end <= start {
        return Err("swarm reply has no JSON object".into());
    }
    serde_json::from_str::<T>(&trimmed[start..=end])
        .map_err(|_| "swarm reply is not valid strict JSON".to_string())
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(ask: f64, bid: f64) -> Snapshot {
        Snapshot {
            id: 7,
            broker: "paper".into(),
            symbol: "XAUUSD".into(),
            time: 1,
            bid,
            ask,
            candles: Vec::new(),
        }
    }

    fn scout(setup: &str, confidence: f64, stop: Option<f64>) -> ScoutReport {
        ScoutReport {
            setup: setup.into(),
            confidence,
            stop,
            target: Some(110.0),
            thesis: "recent closes agree".into(),
        }
    }

    #[test]
    fn a_stop_on_the_wrong_side_is_not_a_setup() {
        let snap = snapshot(100.5, 100.0);
        assert!(candidate_from(&scout("long", 70.0, Some(101.0)), &snap).is_none());
        assert!(candidate_from(&scout("short", 70.0, Some(99.0)), &snap).is_none());
        assert!(candidate_from(&scout("long", 70.0, Some(99.0)), &snap).is_some());
        assert!(candidate_from(&scout("short", 70.0, Some(101.0)), &snap).is_some());
    }

    #[test]
    fn an_implausible_or_missing_stop_is_refused() {
        let snap = snapshot(100.5, 100.0);
        assert!(candidate_from(&scout("long", 70.0, None), &snap).is_none());
        // Four cents on a hundred-dollar entry is inside the spread, not an invalidation level.
        assert!(candidate_from(&scout("long", 70.0, Some(99.996)), &snap).is_none());
        // Twenty percent away is a guess, not a level.
        assert!(candidate_from(&scout("long", 70.0, Some(80.0)), &snap).is_none());
    }

    #[test]
    fn a_none_setup_never_becomes_a_candidate() {
        let snap = snapshot(100.5, 100.0);
        assert!(candidate_from(&scout("none", 0.0, None), &snap).is_none());
        assert!(candidate_from(&scout("maybe", 80.0, Some(99.0)), &snap).is_none());
        assert!(candidate_from(&scout("long", 120.0, Some(99.0)), &snap).is_none());
    }

    #[test]
    fn scouts_describing_the_same_setup_merge_and_record_support() {
        let snap = snapshot(100.5, 100.0);
        let mut candidates = Vec::new();
        for (confidence, stop) in [(60.0, 99.0), (75.0, 99.01), (55.0, 99.0)] {
            let candidate = candidate_from(&scout("long", confidence, Some(stop)), &snap).unwrap();
            merge_candidate(&mut candidates, candidate);
        }
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].support, 3);
        assert_eq!(candidates[0].confidence, 75.0);
    }

    #[test]
    fn opposite_sides_stay_separate_candidates() {
        let snap = snapshot(100.5, 100.0);
        let mut candidates = Vec::new();
        merge_candidate(&mut candidates, candidate_from(&scout("long", 70.0, Some(99.0)), &snap).unwrap());
        merge_candidate(&mut candidates, candidate_from(&scout("short", 70.0, Some(101.0)), &snap).unwrap());
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn quorum_drops_the_lonely_scout() {
        let snap = snapshot(100.5, 100.0);
        let mut candidates = Vec::new();
        merge_candidate(&mut candidates, candidate_from(&scout("long", 70.0, Some(99.0)), &snap).unwrap());
        merge_candidate(&mut candidates, candidate_from(&scout("short", 70.0, Some(101.0)), &snap).unwrap());
        merge_candidate(&mut candidates, candidate_from(&scout("short", 65.0, Some(101.0)), &snap).unwrap());
        apply_quorum(&mut candidates, 2);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].side, "short");
        assert_eq!(candidates[0].support, 2);
    }

    #[test]
    fn a_tie_is_not_a_winner() {
        assert_eq!(pick_winner(&[2, 2]), None);
        assert_eq!(pick_winner(&[0, 0, 0]), None);
        assert_eq!(pick_winner(&[1, 3, 0]), Some(1));
    }

    #[test]
    fn votes_outside_the_board_are_ignored() {
        assert_eq!(vote_index("A", 1), Some(0));
        assert_eq!(vote_index(" b ", 3), Some(1));
        assert_eq!(vote_index("C", 2), None);
        assert_eq!(vote_index("reject", 3), None);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn sizing_respects_the_risk_budget_and_the_leverage_room() {
        let mut settings = AiSettings::default();
        settings.max_risk_pct = 1.0;
        let mut state = PaperState::default();
        state.equity = 10_000.0;
        let snap = snapshot(100.5, 100.0);
        // A $2 stop against a $100 risk budget is 50 units, well inside the leverage room.
        let quantity = size_quantity(&settings, &snap, &state, "long", 98.5).unwrap();
        assert!((quantity - 50.0).abs() < 1e-6, "quantity was {quantity}");

        // A $24.50 stop wants only 4 units from the risk budget, so the risk side still binds.
        let quantity = size_quantity(&settings, &snap, &state, "long", 76.0).unwrap();
        assert!((quantity - 100.0 / 24.5).abs() < 1e-6, "quantity was {quantity}");

        // With the whole equity allowed as risk, the leverage room becomes the binding limit.
        settings.max_risk_pct = 100.0;
        let quantity = size_quantity(&settings, &snap, &state, "long", 76.0).unwrap();
        assert!((quantity - 10_000.0 / 100.5).abs() < 1e-6, "quantity was {quantity}");
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn sizing_fails_closed_when_nothing_is_left() {
        let mut settings = AiSettings::default();
        settings.max_risk_pct = 0.0;
        let mut state = PaperState::default();
        state.equity = 10_000.0;
        let snap = snapshot(100.5, 100.0);
        assert!(size_quantity(&settings, &snap, &state, "long", 99.0).is_none());
    }

    #[test]
    fn fenced_replies_are_still_read_and_extra_keys_are_not() {
        let fenced = "```json\n{\"vote\":\"A\",\"confidence\":80,\"reason\":\"clean\"}\n```";
        let report: VoteReport = parse_json(fenced).unwrap();
        assert_eq!(report.vote, "A");
        let padded = "Sure thing: {\"setup\":\"none\",\"confidence\":0,\"thesis\":\"no edge\"}";
        let report: ScoutReport = parse_json(padded).unwrap();
        assert_eq!(report.setup, "none");
        let extra = "{\"setup\":\"none\",\"confidence\":0,\"thesis\":\"x\",\"sneaky\":true}";
        assert!(parse_json::<ScoutReport>(extra).is_err());
    }

    #[test]
    fn a_cloud_round_stands_aside_instead_of_escalating() {
        let local = stand_down(
            false,
            7,
            "no candidate".into(),
            Costs::default(),
            3,
            0,
            0,
            0,
        );
        assert!(matches!(local.decision, Decision::Consult { .. }));
        let cloud = stand_down(
            true,
            7,
            "no candidate".into(),
            Costs::default(),
            3,
            0,
            0,
            0,
        );
        assert!(matches!(cloud.decision, Decision::Wait { .. }));
    }

    #[test]
    fn costs_are_summed_only_when_a_provider_reported_them() {
        let mut costs = Costs::default();
        assert_eq!(costs.total(), None);
        costs.add(None);
        assert_eq!(costs.total(), None);
        costs.add(Some(0.25));
        costs.add(Some(0.5));
        costs.add(Some(f64::NAN));
        assert_eq!(costs.total(), Some(0.75));
    }
}
