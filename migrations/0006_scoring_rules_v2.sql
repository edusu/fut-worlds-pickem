-- Owner: services/events
-- Table: scoring_rules
-- Replaces the legacy 4-column scoring rule (exact / diff / sign / miss)
-- with the World Cup pickem point table:
--   * group-stage match: 5-bucket multiplicative-feel table
--   * knockout match: advancement winner + reg-bucket + ET/PK flags + combo
--   * standings: per-position points + full-group combo + best-thirds set
--   * re-pick penalty as a percentage applied at scoring time
-- Safe to ALTER in place because no production data exists: every consumer
-- of the previous columns is a `todo!()` stub at the time of this migration.

ALTER TABLE scoring_rules
    DROP COLUMN exact_score_points,
    DROP COLUMN correct_diff_points,
    DROP COLUMN correct_sign_points,
    DROP COLUMN miss_points;

-- Group-stage match (also reused for the regulation 90' bucket of knockouts)
ALTER TABLE scoring_rules
    ADD COLUMN gs_miss_points          INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN gs_one_team_points      INTEGER NOT NULL DEFAULT 4,
    ADD COLUMN gs_sign_only_points     INTEGER NOT NULL DEFAULT 10,
    ADD COLUMN gs_sign_plus_one_points INTEGER NOT NULL DEFAULT 16,
    ADD COLUMN gs_exact_points         INTEGER NOT NULL DEFAULT 24;

-- Knockout-only components. The reg-90' bucket reuses the gs_* columns.
ALTER TABLE scoring_rules
    ADD COLUMN ko_adv_points        INTEGER NOT NULL DEFAULT 12,
    ADD COLUMN ko_et_flag_points    INTEGER NOT NULL DEFAULT 2,
    ADD COLUMN ko_pk_flag_points    INTEGER NOT NULL DEFAULT 2,
    ADD COLUMN ko_combo_points      INTEGER NOT NULL DEFAULT 10;

-- Group-standings (1st..4th of each WC group) and best-thirds set.
ALTER TABLE scoring_rules
    ADD COLUMN st_pos1_points         INTEGER NOT NULL DEFAULT 6,
    ADD COLUMN st_pos2_points         INTEGER NOT NULL DEFAULT 4,
    ADD COLUMN st_pos3_points         INTEGER NOT NULL DEFAULT 2,
    ADD COLUMN st_pos4_points         INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN st_full_combo_points   INTEGER NOT NULL DEFAULT 5,
    ADD COLUMN st_best_third_points   INTEGER NOT NULL DEFAULT 3;

-- Re-pick penalty (percentage off the awarded points when was_changed is
-- true). 25 → multiply by 0.75, floor.
ALTER TABLE scoring_rules
    ADD COLUMN repick_penalty_pct  INTEGER NOT NULL DEFAULT 25
        CHECK (repick_penalty_pct BETWEEN 0 AND 100);

-- The seeded 'default' row already exists from migration 0004 with NULL
-- legacy columns dropped. The new columns picked up their DEFAULTs above,
-- so the seed row is already correct. No UPDATE needed.
