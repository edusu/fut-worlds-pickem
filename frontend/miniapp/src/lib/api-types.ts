/**
 * Runtime-validated types for the FutWorldsPickem HTTP API.
 *
 * Every response shape is declared as a zod schema so we get a single
 * source of truth for both compile-time TypeScript types (via
 * `z.infer`) and runtime validation (via `schema.parse(...)` after the
 * fetch). Keeps the FE honest if the backend ever drifts.
 */

import { z } from 'zod';

// ---------------------------------------------------------------------------
// Common primitives
// ---------------------------------------------------------------------------

export const uuidSchema = z.string().uuid();

export const teamSchema = z.object({
  id: uuidSchema,
  name: z.string(),
  flag_emoji: z.string(),
  country_code: z.string(),
});
export type Team = z.infer<typeof teamSchema>;

const tournamentGroupStateSchema = z.enum(['open', 'closed', 'scored']);
export type TournamentGroupState = z.infer<typeof tournamentGroupStateSchema>;

const knockoutPhaseStateSchema = z.enum(['open', 'closed', 'scored']);
export type KnockoutPhaseState = z.infer<typeof knockoutPhaseStateSchema>;

const knockoutStageSchema = z.enum([
  'LAST_32',
  'LAST_16',
  'QUARTER_FINALS',
  'SEMI_FINALS',
  'THIRD_PLACE',
  'FINAL',
]);
export type KnockoutStage = z.infer<typeof knockoutStageSchema>;

const matchStatusSchema = z.enum([
  'scheduled',
  'timed',
  'in_play',
  'paused',
  'finished',
  'suspended',
  'postponed',
  'cancelled',
  'awarded',
]);

export const matchSchema = z.object({
  id: uuidSchema,
  external_id: z.string(),
  tournament_group_id: uuidSchema.nullable(),
  knockout_phase_id: uuidSchema.nullable(),
  home_team_id: uuidSchema.nullable(),
  away_team_id: uuidSchema.nullable(),
  kickoff_at: z.string(),
  home_score: z.number().int().nullable(),
  away_score: z.number().int().nullable(),
  et_home_score: z.number().int().nullable(),
  et_away_score: z.number().int().nullable(),
  pens_winner_team_id: uuidSchema.nullable(),
  status: matchStatusSchema,
});
export type Match = z.infer<typeof matchSchema>;

export const myPredictionSchema = z.object({
  reg_home: z.number().int(),
  reg_away: z.number().int(),
  advancement_winner_team_id: uuidSchema.nullable(),
  pens_winner_team_id: uuidSchema.nullable(),
  was_changed: z.boolean(),
});
export type MyPrediction = z.infer<typeof myPredictionSchema>;

export const matchWithPredictionSchema = z.object({
  match: matchSchema,
  my_prediction: myPredictionSchema.nullable(),
});
export type MatchWithPrediction = z.infer<typeof matchWithPredictionSchema>;

// ---------------------------------------------------------------------------
// GET /api/tournament-groups/active
// ---------------------------------------------------------------------------

export const activeGroupSchema = z.object({
  id: uuidSchema,
  name: z.string(),
  deadline_at: z.string(),
  state: tournamentGroupStateSchema,
  teams: z.array(teamSchema),
});
export type ActiveGroup = z.infer<typeof activeGroupSchema>;
export const activeGroupsSchema = z.array(activeGroupSchema);

// ---------------------------------------------------------------------------
// GET /api/knockouts/active
// ---------------------------------------------------------------------------

export const activeKnockoutSchema = z.object({
  id: uuidSchema,
  stage: knockoutStageSchema,
  position: z.number().int(),
  display_name: z.string(),
  deadline_at: z.string(),
  state: knockoutPhaseStateSchema,
});
export type ActiveKnockout = z.infer<typeof activeKnockoutSchema>;
export const activeKnockoutsSchema = z.array(activeKnockoutSchema);

// ---------------------------------------------------------------------------
// GET /api/{tournament-groups|knockouts}/{id}/matches?pickem=<uuid>
// ---------------------------------------------------------------------------

export const matchesResponseSchema = z.array(matchWithPredictionSchema);

// ---------------------------------------------------------------------------
// POST /api/predictions/matches
// ---------------------------------------------------------------------------

export type ParentRef =
  | { kind: 'tournament_group'; id: string }
  | { kind: 'knockout_phase'; id: string };

export interface MatchPredictionInput {
  match_id: string;
  reg_home: number;
  reg_away: number;
  advancement_winner_team_id?: string | null;
  pens_winner_team_id?: string | null;
}

export interface SubmitMatchesRequest {
  pickem_group_id: string;
  parent: ParentRef;
  predictions: MatchPredictionInput[];
}

export const acceptedResponseSchema = z.object({ accepted: z.number().int() });
export type AcceptedResponse = z.infer<typeof acceptedResponseSchema>;

// ---------------------------------------------------------------------------
// POST /api/predictions/standings
// ---------------------------------------------------------------------------

export interface SubmitStandingsRequest {
  pickem_group_id: string;
  tournament_group_id: string;
  pos1_team_id: string;
  pos2_team_id: string;
  pos3_team_id: string;
  pos4_team_id: string;
}

// ---------------------------------------------------------------------------
// POST /api/predictions/best-thirds
// ---------------------------------------------------------------------------

export interface SubmitBestThirdsRequest {
  pickem_group_id: string;
  team_ids: string[];
}

export const okResponseSchema = z.object({ ok: z.boolean() });
export type OkResponse = z.infer<typeof okResponseSchema>;
