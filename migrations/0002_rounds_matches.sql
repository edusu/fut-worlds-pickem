-- Owner: services/events
-- Tables: tournaments, teams, tournament_groups, knockout_phases,
--         tournament_group_teams, matches
--
-- The events service ingests fixtures from the upstream provider and seeds
-- the entire tournament structure: the tournament row, the team catalog,
-- the 12 group-stage groups, the 6 knockout phases (Round of 32 through
-- Final), the team-to-group assignments, and the matches themselves. The
-- bot may READ to display fixtures and rankings; the api may READ to render
-- the Mini App. No service other than events writes here.
--
-- The historical migration 0005_tournament_data.sql added these tables
-- piecemeal on top of an earlier rounds-based shape. Migration 0005 is now
-- a no-op placeholder; everything has been consolidated here so the schema
-- is self-contained and the table ordering reflects FK dependencies
-- (tournaments → teams → tournament_groups → knockout_phases →
-- tournament_group_teams → matches).

-- A tournament is the top-level competition we ingest fixtures for, e.g.
-- "Mundial 2026". `external_id` is the upstream provider's competition
-- code (football-data.org uses "WC", "CL", ...) and is the natural key the
-- ingester uses to look the row up at boot. v1 only ever seeds a single
-- tournament, but the table is shaped to grow into multiple.
CREATE TABLE tournaments (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT NOT NULL,
    external_id  TEXT NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Teams are global: one row per country regardless of how many tournaments
-- they participate in. Spain is Spain whether the row is referenced from a
-- World Cup match or a Euros match. Tournament-specific membership is
-- expressed via `tournament_group_teams`, not via columns on this table.
CREATE TABLE teams (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT NOT NULL,
    flag_emoji    TEXT NOT NULL,
    country_code  TEXT NOT NULL UNIQUE
);

-- A tournament group is the structural parent of group-stage matches
-- ("Group A", "Group B", ...). Each group carries its own `deadline_at`
-- and `state`: in v1 all 12 groups in a given tournament share the same
-- timestamp value (the first group-stage kickoff), but the schema is
-- shaped so per-group deadlines could be set later without migration.
--
-- The `UNIQUE (tournament_id, id)` is required for the composite FK from
-- `tournament_group_teams (tournament_id, tournament_group_id)`. Postgres
-- needs a UNIQUE on the parent's referenced columns; the PK alone is not
-- enough because the composite FK references a (tournament_id, id) tuple.
CREATE TABLE tournament_groups (
    id            UUID NOT NULL DEFAULT gen_random_uuid(),
    tournament_id UUID NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    deadline_at   TIMESTAMPTZ NOT NULL,
    state         TEXT NOT NULL DEFAULT 'open'
        CHECK (state IN ('open', 'closed', 'scored')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (id),
    UNIQUE (tournament_id, name),
    UNIQUE (tournament_id, id)
);

-- Knockout phases are the structural parent of knockout matches: one row
-- per stage (Round of 32 through Final). The `stage` column carries the
-- upstream code so it is a stable internal identifier; `display_name` is
-- the localised string the Mini App renders. `position` enables cheap
-- bracket rendering via `ORDER BY position` instead of a CASE on stage.
--
-- v1 sets all 6 phases' `deadline_at` to the same value (the first
-- knockout kickoff), matching the previous "single shared knockout
-- deadline" behaviour. The schema supports per-stage deadlines without
-- migration the day product wants them.
CREATE TABLE knockout_phases (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tournament_id UUID NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
    stage         TEXT NOT NULL
        CHECK (stage IN ('LAST_32', 'LAST_16', 'QUARTER_FINALS',
                         'SEMI_FINALS', 'THIRD_PLACE', 'FINAL')),
    position      SMALLINT NOT NULL CHECK (position BETWEEN 1 AND 99),
    display_name  TEXT NOT NULL,
    deadline_at   TIMESTAMPTZ NOT NULL,
    state         TEXT NOT NULL DEFAULT 'open'
        CHECK (state IN ('open', 'closed', 'scored')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tournament_id, stage),
    UNIQUE (tournament_id, position)
);

-- Many-to-many association between tournament groups and teams. The
-- `tournament_id` column is denormalised from the parent group so the
-- primary key `(tournament_id, team_id)` enforces "each team appears in
-- exactly one group per tournament" at the DB level. The composite FK
-- `(tournament_id, tournament_group_id) → tournament_groups(tournament_id, id)`
-- prevents inconsistent denormalisation: you cannot point at a group that
-- belongs to a different tournament.
CREATE TABLE tournament_group_teams (
    tournament_id        UUID NOT NULL,
    tournament_group_id  UUID NOT NULL,
    team_id              UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    PRIMARY KEY (tournament_id, team_id),
    FOREIGN KEY (tournament_id, tournament_group_id)
        REFERENCES tournament_groups (tournament_id, id) ON DELETE CASCADE
);

-- A match belongs to exactly one structural parent: either a
-- `tournament_group` (group-stage match) or a `knockout_phase` (knockout
-- match). The XOR check enforces this at the DB level. Both team
-- references are nullable to represent unresolved knockouts (the upstream
-- ships fixtures without participants until the bracket resolves); the
-- ingester populates the FKs once the participants are known.
--
-- Score columns model the regulation result; ET / pens columns extend it
-- for knockout matches that go beyond 90'. Group-stage matches leave the
-- ET / pens columns NULL (the existing pair-consistency CHECKs make that
-- the only valid shape for them).
--
-- `external_id` is the upstream's match id. football-data.org uses
-- globally-unique integer ids so a global UNIQUE is sufficient here.
CREATE TABLE matches (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id          TEXT NOT NULL UNIQUE,
    tournament_group_id  UUID
        REFERENCES tournament_groups(id) ON DELETE RESTRICT,
    knockout_phase_id    UUID
        REFERENCES knockout_phases(id) ON DELETE RESTRICT,
    home_team_id         UUID
        REFERENCES teams(id) ON DELETE RESTRICT,
    away_team_id         UUID
        REFERENCES teams(id) ON DELETE RESTRICT,
    kickoff_at           TIMESTAMPTZ NOT NULL,
    home_score           INTEGER,
    away_score           INTEGER,
    et_home_score        INTEGER,
    et_away_score        INTEGER,
    pens_winner_team_id  UUID
        REFERENCES teams(id) ON DELETE RESTRICT,
    status               TEXT NOT NULL DEFAULT 'scheduled',

    -- Exactly one structural parent must be set: every match is either a
    -- group-stage match or a knockout match, never both, never neither.
    CONSTRAINT matches_parent_xor CHECK (
        (tournament_group_id IS NOT NULL AND knockout_phase_id IS NULL)
        OR
        (tournament_group_id IS NULL AND knockout_phase_id IS NOT NULL)
    ),

    -- Status mirrors the upstream `MatchStatusDto` value set 1:1.
    CONSTRAINT matches_status_valid CHECK (
        status IN ('scheduled', 'timed', 'in_play', 'paused', 'finished',
                   'suspended', 'postponed', 'cancelled', 'awarded')
    ),

    -- Home and away cannot be the same team. NULL on either side (TBD)
    -- trivially satisfies the check.
    CONSTRAINT matches_distinct_teams CHECK (
        home_team_id IS NULL
        OR away_team_id IS NULL
        OR home_team_id <> away_team_id
    ),

    -- ET scores travel as a pair: either both set or both NULL.
    CONSTRAINT matches_et_pair_consistent CHECK (
        (et_home_score IS NULL AND et_away_score IS NULL)
        OR (et_home_score IS NOT NULL AND et_away_score IS NOT NULL)
    ),

    -- Penalties only follow a draw at the end of extra time. We can't
    -- enforce "matches the actual ET tie" here without coupling to score
    -- columns, but we can at least require ET data when a pens winner is
    -- recorded.
    CONSTRAINT matches_pens_requires_et CHECK (
        pens_winner_team_id IS NULL OR et_home_score IS NOT NULL
    )
);

-- Indexes on FK columns. Postgres does not auto-create them; we add
-- explicit ones for every column the application filters on. Partial
-- indexes on nullable parent / team FKs skip the half of the rowset that
-- can't match.
CREATE INDEX idx_tournament_groups_tournament      ON tournament_groups(tournament_id);
CREATE INDEX idx_knockout_phases_tournament        ON knockout_phases(tournament_id);
CREATE INDEX idx_tournament_group_teams_tournament ON tournament_group_teams(tournament_id);
CREATE INDEX idx_tournament_group_teams_team       ON tournament_group_teams(team_id);
CREATE INDEX idx_matches_tournament_group
    ON matches(tournament_group_id) WHERE tournament_group_id IS NOT NULL;
CREATE INDEX idx_matches_knockout_phase
    ON matches(knockout_phase_id)   WHERE knockout_phase_id   IS NOT NULL;
CREATE INDEX idx_matches_home_team
    ON matches(home_team_id)        WHERE home_team_id        IS NOT NULL;
CREATE INDEX idx_matches_away_team
    ON matches(away_team_id)        WHERE away_team_id        IS NOT NULL;
CREATE INDEX idx_matches_status_kickoff ON matches(status, kickoff_at);
