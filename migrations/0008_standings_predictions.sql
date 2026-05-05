-- Owner: services/api
-- Tables: group_standings_predictions, best_thirds_predictions,
--         best_thirds_scoring
-- Group-standings predictions: per WC group, the user submits an ordered
-- 4-tuple (1st..4th). Stored as four FK columns rather than an array so
-- DB-level FK integrity catches typos / stale team ids.
--
-- Best thirds: per pickem, the user submits an unordered set of 8 teams.
-- Stored as a thin junction table with a CHECK enforcing 0 ≤ count ≤ 8 at
-- write time; the api enforces "exactly 8 at lock time".
--
-- Scoring is computed once at end of group stage and stored on each row
-- (positions) plus a per-user aggregate row for best thirds.

CREATE TABLE group_standings_predictions (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id              BIGINT NOT NULL REFERENCES users(telegram_id),
    pickem_group_id      UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    tournament_group_id  UUID NOT NULL REFERENCES tournament_groups(id),
    pos1_team_id         UUID NOT NULL REFERENCES teams(id),
    pos2_team_id         UUID NOT NULL REFERENCES teams(id),
    pos3_team_id         UUID NOT NULL REFERENCES teams(id),
    pos4_team_id         UUID NOT NULL REFERENCES teams(id),
    points_awarded       INTEGER,
    submitted_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, pickem_group_id, tournament_group_id),
    -- The four positions must be four different teams.
    CONSTRAINT group_standings_distinct_teams CHECK (
        pos1_team_id <> pos2_team_id AND pos1_team_id <> pos3_team_id
        AND pos1_team_id <> pos4_team_id AND pos2_team_id <> pos3_team_id
        AND pos2_team_id <> pos4_team_id AND pos3_team_id <> pos4_team_id
    )
);

CREATE INDEX idx_group_standings_pickem ON group_standings_predictions(pickem_group_id);

-- Best-thirds picks: 8 rows per (user, pickem) at lock time. Modelled as a
-- junction table so adding/removing a single team is a one-row insert/delete
-- without rewriting an array.
CREATE TABLE best_thirds_predictions (
    user_id          BIGINT NOT NULL REFERENCES users(telegram_id),
    pickem_group_id  UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    team_id          UUID NOT NULL REFERENCES teams(id),
    submitted_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, pickem_group_id, team_id)
);

CREATE INDEX idx_best_thirds_pickem ON best_thirds_predictions(pickem_group_id);

-- Per-(pickem, user) aggregate written by the scorer at end of group stage.
-- Kept separate from individual rows so the points figure is a single
-- source of truth for the ranking aggregation, even if a row in
-- best_thirds_predictions is later edited (which the api will refuse, but
-- defense in depth).
CREATE TABLE best_thirds_scoring (
    pickem_group_id  UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id          BIGINT NOT NULL REFERENCES users(telegram_id),
    points_awarded   INTEGER NOT NULL,
    scored_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (pickem_group_id, user_id)
);
