/**
 * Typed wrappers over the raw `api` fetch helper. Every read method runs
 * the response through its zod schema so the rest of the app can rely on
 * runtime-validated shapes — drift between backend and frontend surfaces
 * as a parse error rather than a downstream type confusion.
 */

import type { ZodType } from 'zod';

import { api } from './api';
import {
  acceptedResponseSchema,
  activeGroupsSchema,
  activeKnockoutsSchema,
  matchesResponseSchema,
  okResponseSchema,
} from './api-types';
import type {
  AcceptedResponse,
  ActiveGroup,
  ActiveKnockout,
  MatchWithPrediction,
  OkResponse,
  SubmitBestThirdsRequest,
  SubmitMatchesRequest,
  SubmitStandingsRequest,
} from './api-types';

async function request<T>(
  method: 'get' | 'post',
  path: string,
  schema: ZodType<T>,
  body?: unknown,
): Promise<T> {
  const data =
    method === 'get'
      ? await api.get<unknown>(path)
      : await api.post<unknown>(path, body);
  return schema.parse(data);
}

export const fetchActiveGroups = (): Promise<ActiveGroup[]> =>
  request('get', '/api/tournament-groups/active', activeGroupsSchema);

export const fetchActiveKnockouts = (): Promise<ActiveKnockout[]> =>
  request('get', '/api/knockouts/active', activeKnockoutsSchema);

export const fetchGroupMatches = (
  groupId: string,
  pickemId: string,
): Promise<MatchWithPrediction[]> =>
  request(
    'get',
    `/api/tournament-groups/${groupId}/matches?pickem=${encodeURIComponent(pickemId)}`,
    matchesResponseSchema,
  );

export const fetchKnockoutMatches = (
  phaseId: string,
  pickemId: string,
): Promise<MatchWithPrediction[]> =>
  request(
    'get',
    `/api/knockouts/${phaseId}/matches?pickem=${encodeURIComponent(pickemId)}`,
    matchesResponseSchema,
  );

export const submitMatches = (body: SubmitMatchesRequest): Promise<AcceptedResponse> =>
  request('post', '/api/predictions/matches', acceptedResponseSchema, body);

export const submitStandings = (body: SubmitStandingsRequest): Promise<OkResponse> =>
  request('post', '/api/predictions/standings', okResponseSchema, body);

export const submitBestThirds = (body: SubmitBestThirdsRequest): Promise<OkResponse> =>
  request('post', '/api/predictions/best-thirds', okResponseSchema, body);
