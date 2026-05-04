-- Owner: services/bot
-- Tables: users, groups, group_members
-- The bot service writes to all three when handling /start, /new_pickem,
-- /join. Other services may READ but should not write.

CREATE TABLE users (
    telegram_id    BIGINT PRIMARY KEY,
    username       TEXT,
    first_name     TEXT NOT NULL,
    language_code  TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE groups (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    telegram_chat_id BIGINT UNIQUE NOT NULL,
    name             TEXT NOT NULL,
    owner_id         BIGINT NOT NULL REFERENCES users(telegram_id),
    scoring_rule_id  UUID NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE group_members (
    group_id  UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id   BIGINT NOT NULL REFERENCES users(telegram_id),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, user_id)
);

-- The composite PK indexes (group_id, user_id) which serves group→members
-- lookups (`/ranking`). The reverse direction — "what pickems is this user
-- in?" — needs an explicit index on user_id alone.
CREATE INDEX group_members_user_id_idx ON group_members (user_id);
