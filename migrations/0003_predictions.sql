-- Owner: services/api
-- Tables: predictions
-- The HTTP API writes predictions submitted from the Mini App. The events
-- scorer updates `points_awarded` once a match is finished.

CREATE TABLE predictions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         BIGINT NOT NULL REFERENCES users(telegram_id),
    match_id        UUID NOT NULL REFERENCES matches(id),
    predicted_home  INTEGER NOT NULL,
    predicted_away  INTEGER NOT NULL,
    points_awarded  INTEGER,
    submitted_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, match_id)
);
-- No explicit (user_id, match_id) index: the UNIQUE constraint above
-- already builds a btree on those columns. Hot-path indexes for the
-- per-pickem access patterns land in 0007 once `pickem_group_id` exists.
