-- Owner: services/events
-- Table: scoring_rules
-- Wires the FK from groups.scoring_rule_id and seeds a 'default' rule that
-- every new group falls back to until its owner customizes it.

CREATE TABLE scoring_rules (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                TEXT NOT NULL,
    exact_score_points  INTEGER NOT NULL DEFAULT 5,
    correct_diff_points INTEGER NOT NULL DEFAULT 3,
    correct_sign_points INTEGER NOT NULL DEFAULT 1,
    miss_points         INTEGER NOT NULL DEFAULT 0
);

ALTER TABLE groups
    ADD CONSTRAINT fk_groups_scoring_rule
    FOREIGN KEY (scoring_rule_id) REFERENCES scoring_rules(id);

INSERT INTO scoring_rules (name) VALUES ('default');
