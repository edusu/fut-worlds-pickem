/**
 * Shared utilities + small UI primitives used by all three prediction
 * tabs. Reusable hooks live here so the React Query cache for `active
 * groups` is shared across tabs (the response embeds each group's four
 * teams, which we use everywhere).
 *
 * Note: this file is `.ts` rather than `.tsx` because it doesn't render
 * JSX — UI primitives that need JSX live in `shared.tsx`.
 */

import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import { useMemo } from 'react';

import { fetchActiveGroups } from '../../lib/api-client';
import type { ActiveGroup, Team } from '../../lib/api-types';

const ACTIVE_GROUPS_KEY = ['active', 'groups'] as const;

export function useActiveGroups(): UseQueryResult<ActiveGroup[], Error> {
  return useQuery({
    queryKey: ACTIVE_GROUPS_KEY,
    queryFn: fetchActiveGroups,
  });
}

export function useTeamsById(): Map<string, Team> {
  const { data } = useActiveGroups();
  return useMemo(() => {
    const map = new Map<string, Team>();
    for (const g of data ?? []) {
      for (const t of g.teams) {
        map.set(t.id, t);
      }
    }
    return map;
  }, [data]);
}
