-- Owner: services/api
-- Table: predictions
-- Evolves the prediction shape from "two integers per match" to the full
-- knockout-aware payload: regulation 90' goals plus optional advancement
-- winner, optional pens winner, plus a was_changed flag driving the
-- re-pick penalty at scoring time.

ALTER TABLE predictions
    RENAME COLUMN predicted_home TO reg_home;
ALTER TABLE predictions
    RENAME COLUMN predicted_away TO reg_away;

ALTER TABLE predictions
    ADD COLUMN advancement_winner_team_id  UUID REFERENCES teams(id),
    ADD COLUMN pens_winner_team_id         UUID REFERENCES teams(id),
    ADD COLUMN was_changed                 BOOLEAN NOT NULL DEFAULT FALSE;

-- Knockout consistency rules enforced at the DB layer. Group-stage rows
-- (where advancement / pens are NULL) trivially satisfy these. The Rust
-- layer also enforces the same rules via Prediction::is_consistent() to
-- give callers a clean error before the DB round-trip.
--
--   1. If pens_winner is set, advancement_winner must be set and equal to it
--      (you can't say "Croatia advances" but "Brazil wins on pens").
--   2. If pens_winner is set, the reg score must be a draw (a pens shootout
--      can only happen after a draw at the end of ET, which itself only
--      happens after a draw in regulation).
--   3. If reg is non-draw and advancement_winner is set, advancement_winner
--      must match the reg winner. (For non-draw regs the api will normally
--      leave advancement_winner NULL and derive it implicitly.)
ALTER TABLE predictions
    ADD CONSTRAINT predictions_knockout_consistency
    CHECK (
        -- rule 1
        (pens_winner_team_id IS NULL OR pens_winner_team_id = advancement_winner_team_id)
        -- rule 2
        AND (pens_winner_team_id IS NULL OR reg_home = reg_away)
    );
