-- Owner: services/events
-- Tables: rounds, matches
-- The events service ingests fixtures from the upstream provider and creates
-- rounds + matches. The bot may READ to list active rounds; the api may READ
-- to render fixtures in the Mini App.

CREATE TABLE rounds (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id     UUID NOT NULL REFERENCES groups(id),
    name         TEXT NOT NULL,
    deadline_at  TIMESTAMPTZ NOT NULL,
    state        TEXT NOT NULL DEFAULT 'open',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
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
