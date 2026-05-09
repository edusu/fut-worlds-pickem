-- Owner: services/api
-- Table: predictions
-- Evolves the prediction shape from "two integers per match" to the full
-- knockout-aware payload: regulation 90' goals plus optional advancement
-- winner, optional pens winner, plus a was_changed flag driving the
-- re-pick penalty at scoring time. Also makes predictions per-pickem so
-- a user playing in multiple pickems gets one row per (pickem, match);
-- the schema previously assumed a single global prediction per (user,
-- match), which conflicted with per-pickem scoring rules and
-- per-pickem points_awarded.

ALTER TABLE predictions
    RENAME COLUMN predicted_home TO reg_home;
ALTER TABLE predictions
    RENAME COLUMN predicted_away TO reg_away;

ALTER TABLE predictions
    ADD COLUMN advancement_winner_team_id  UUID REFERENCES teams(id),
    ADD COLUMN pens_winner_team_id         UUID REFERENCES teams(id),
    ADD COLUMN was_changed                 BOOLEAN NOT NULL DEFAULT FALSE;

-- Per-pickem predictions. The original 0003 declared
-- `UNIQUE (user_id, match_id)` (auto-named `predictions_user_id_match_id_key`),
-- which made a prediction global across pickems. With per-pickem scoring
-- rules and a single `points_awarded` column, that's incoherent: the
-- same row would have to earn different points in different pickems.
-- We add `pickem_group_id` (FK to `groups`, CASCADE on delete so dropping
-- a pickem cleans up its predictions) and swap the UNIQUE so each
-- (user, pickem, match) is its own row.
ALTER TABLE predictions
    ADD COLUMN pickem_group_id UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE;

ALTER TABLE predictions DROP CONSTRAINT predictions_user_id_match_id_key;
ALTER TABLE predictions
    ADD CONSTRAINT predictions_user_pickem_match_unique
    UNIQUE (user_id, pickem_group_id, match_id);

-- Hot-path indexes. The pickem index serves Mini App reads
-- ("predictions in this pickem") and ranking aggregates; the match
-- index serves the scorer ("predictions across pickems for a finished
-- match"). The UNIQUE above already covers (user_id, pickem_group_id,
-- match_id) lookups so no extra index is needed for that path.
CREATE INDEX predictions_pickem_idx ON predictions (pickem_group_id);
CREATE INDEX predictions_match_idx  ON predictions (match_id);

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
