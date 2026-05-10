-- Owner: services/events
-- Append-only audit trail: one row per successful scoring run, regardless
-- of which prediction shape (match / group standings / best thirds) was
-- scored. The discriminator `audit_kind` plus the `prediction_id` column
-- identifies the source row in the corresponding table. No FK is declared
-- across the three target tables because Postgres does not support
-- polymorphic FKs and acoplar a la columna a una sola tabla restaría
-- generalidad — la consistencia se garantiza desde el código del scorer,
-- que sólo escribe filas justo después de un INSERT/UPDATE en la tabla
-- correspondiente.

CREATE TABLE scoring_audit (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scored_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    audit_kind        TEXT NOT NULL
                          CHECK (audit_kind IN ('match', 'group_standings', 'best_thirds')),
    prediction_id     UUID NOT NULL,
    pickem_group_id   UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id           BIGINT NOT NULL REFERENCES users(telegram_id),
    points_awarded    INT NOT NULL,
    scoring_rule_id   UUID NOT NULL REFERENCES scoring_rules(id),
    was_changed       BOOLEAN NOT NULL
);

-- Lookups by prediction (audit timeline of a single pick).
CREATE INDEX scoring_audit_prediction_idx
    ON scoring_audit(prediction_id);

-- Lookups by (pickem, user): "give me Alice's full audit in this pickem".
CREATE INDEX scoring_audit_pickem_user_idx
    ON scoring_audit(pickem_group_id, user_id);
