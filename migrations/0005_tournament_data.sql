-- Owner: services/events
-- Tables: teams, tournament_groups, tournament_group_teams
-- Plus: phase column on rounds + matches; ET / pens columns on matches.
-- The events service ingests the official tournament structure (12 groups
-- of 4 teams) and the matches that belong to each phase. The bot and api
-- services may READ but should not write.

CREATE TABLE teams (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT NOT NULL,
    flag_emoji    TEXT NOT NULL,
    country_code  TEXT NOT NULL UNIQUE
);

CREATE TABLE tournament_groups (
    id    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name  TEXT NOT NULL UNIQUE
);

-- Many-to-many association: a team belongs to exactly one tournament group
-- in the World Cup, but the table is left open for future tournaments where
-- a team might span groups (preseason cups, etc.). The UNIQUE on team_id
-- enforces single-group membership at the DB level for the World Cup.
CREATE TABLE tournament_group_teams (
    tournament_group_id  UUID NOT NULL REFERENCES tournament_groups(id) ON DELETE CASCADE,
    team_id              UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    PRIMARY KEY (tournament_group_id, team_id),
    UNIQUE (team_id)
);

-- Phase distinguishes group-stage from knockout matches. Drives which
-- scoring function the events scorer dispatches to.
ALTER TABLE rounds
    ADD COLUMN phase TEXT NOT NULL DEFAULT 'group'
        CHECK (phase IN ('group', 'knockout'));

ALTER TABLE matches
    ADD COLUMN phase TEXT NOT NULL DEFAULT 'group'
        CHECK (phase IN ('group', 'knockout'));

-- Knockout result columns. NULL means "not applicable / not reached":
--   - et_*_score is NULL when the match did not go to extra time.
--   - pens_winner_team_id is NULL when no penalty shootout was played.
ALTER TABLE matches
    ADD COLUMN et_home_score        INTEGER,
    ADD COLUMN et_away_score        INTEGER,
    ADD COLUMN pens_winner_team_id  UUID REFERENCES teams(id);

-- ET scores travel as a pair: either both set or both NULL.
ALTER TABLE matches
    ADD CONSTRAINT matches_et_pair_consistent
    CHECK (
        (et_home_score IS NULL AND et_away_score IS NULL)
        OR (et_home_score IS NOT NULL AND et_away_score IS NOT NULL)
    );

-- Penalties only follow a draw at the end of extra time. We can't enforce
-- "matches the actual ET tie" here without coupling to score columns, but
-- we can at least require ET data when a pens winner is recorded.
ALTER TABLE matches
    ADD CONSTRAINT matches_pens_requires_et
    CHECK (
        pens_winner_team_id IS NULL OR et_home_score IS NOT NULL
    );

CREATE INDEX idx_tournament_group_teams_team ON tournament_group_teams(team_id);
