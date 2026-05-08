-- Owner: services/events
-- Tables: tournaments, rounds, matches
-- The events service ingests fixtures from the upstream provider and creates
-- rounds + matches under a parent tournament. The bot may READ to list active
-- rounds; the api may READ to render fixtures in the Mini App.

-- A tournament is the top-level competition we ingest fixtures for, e.g.
-- "World Cup 2026". `external_id` is the upstream provider's competition
-- code (football-data.org uses "WC", "CL", ...) and is the natural key the
-- ingester uses to look the row up at boot. v1 only ever seeds a single
-- tournament, but the table is shaped to grow into multiple.
CREATE TABLE tournaments (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT NOT NULL,
    external_id  TEXT NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A round groups one or more matches under a single submission deadline.
-- Rounds belong to a tournament, not to a Telegram pickem: every pickem in
-- the system shares the same set of rounds for a given tournament. The
-- `(tournament_id, name)` UNIQUE pair makes "Group stage 2026" / "Knockouts
-- 2026" idempotent at the DB level so the bootstrap can re-run safely.
CREATE TABLE rounds (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tournament_id UUID NOT NULL REFERENCES tournaments(id),
    name          TEXT NOT NULL,
    deadline_at   TIMESTAMPTZ NOT NULL,
    state         TEXT NOT NULL DEFAULT 'open',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tournament_id, name)
);

CREATE TABLE matches (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    round_id     UUID NOT NULL REFERENCES rounds(id),
    external_id  TEXT NOT NULL,
    home_team    TEXT NOT NULL,
    away_team    TEXT NOT NULL,
    home_flag    TEXT NOT NULL,
    away_flag    TEXT NOT NULL,
    kickoff_at   TIMESTAMPTZ NOT NULL,
    home_score   INTEGER,
    away_score   INTEGER,
    status       TEXT NOT NULL DEFAULT 'scheduled',
    UNIQUE (round_id, external_id)
);

CREATE INDEX idx_matches_round_status ON matches(round_id, status);
