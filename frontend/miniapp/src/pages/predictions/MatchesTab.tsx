/**
 * "Matches" tab — group-stage and knockout regulation predictions.
 *
 * Renders one section per active parent (tournament_group or
 * knockout_phase). Each section maintains its own form state and submits
 * its batch via `POST /api/predictions/matches`. The parent's `kind`
 * decides whether the knockout-specific controls (draw → advancement
 * winner + pens flag) are shown.
 *
 * State is split into two layers so background refetches do not clobber
 * in-progress edits: API data feeds `defaults`, user keystrokes live in
 * `edits`, and what is rendered is the per-match merge `defaults | edits`.
 * On successful submit we clear `edits` and invalidate the cache so the
 * next refetch's `defaults` reflect the saved values.
 */

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';

import {
  fetchActiveKnockouts,
  fetchGroupMatches,
  fetchKnockoutMatches,
  submitMatches,
} from '../../lib/api-client';
import type {
  Match,
  MatchPredictionInput,
  MatchWithPrediction,
  ParentRef,
  Team,
} from '../../lib/api-types';

import { useActiveGroups, useTeamsById } from './shared';
import { MutationStatus, Section } from './ui';

interface Props {
  pickemId: string;
}

interface ParentDescriptor {
  parent: ParentRef;
  title: string;
  deadline: string;
  isKnockout: boolean;
  queryKey: readonly unknown[];
  queryFn: () => Promise<MatchWithPrediction[]>;
}

export function MatchesTab({ pickemId }: Props) {
  const groupsQ = useActiveGroups();
  const knockoutsQ = useQuery({
    queryKey: ['active', 'knockouts'] as const,
    queryFn: fetchActiveKnockouts,
  });

  if (groupsQ.isLoading || knockoutsQ.isLoading) {
    return <p>Cargando…</p>;
  }
  if (groupsQ.error) {
    return <p style={{ color: 'tomato' }}>Error: {groupsQ.error.message}</p>;
  }
  if (knockoutsQ.error) {
    return <p style={{ color: 'tomato' }}>Error: {knockoutsQ.error.message}</p>;
  }

  const parents: ParentDescriptor[] = [
    ...(groupsQ.data ?? []).map<ParentDescriptor>((g) => ({
      parent: { kind: 'tournament_group', id: g.id },
      title: g.name,
      deadline: g.deadline_at,
      isKnockout: false,
      queryKey: ['matches', 'group', g.id, pickemId],
      queryFn: () => fetchGroupMatches(g.id, pickemId),
    })),
    ...(knockoutsQ.data ?? []).map<ParentDescriptor>((k) => ({
      parent: { kind: 'knockout_phase', id: k.id },
      title: k.display_name,
      deadline: k.deadline_at,
      isKnockout: true,
      queryKey: ['matches', 'knockout', k.id, pickemId],
      queryFn: () => fetchKnockoutMatches(k.id, pickemId),
    })),
  ];

  if (parents.length === 0) return <p>No hay rondas activas ahora mismo.</p>;

  return (
    <div>
      {parents.map((p) => (
        <ParentSection key={p.queryKey.join('/')} pickemId={pickemId} {...p} />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// One section per parent
// ---------------------------------------------------------------------------

function ParentSection({
  parent,
  title,
  deadline,
  isKnockout,
  queryKey,
  queryFn,
  pickemId,
}: ParentDescriptor & { pickemId: string }) {
  const queryClient = useQueryClient();
  const matchesQ = useQuery({ queryKey, queryFn });
  const teamsById = useTeamsById();

  // Defaults derived from API; edits are user keystrokes that override
  // them per-match. The merge happens at render time.
  const defaults = useMemo<Map<string, FormRow>>(() => {
    const map = new Map<string, FormRow>();
    for (const mwp of matchesQ.data ?? []) {
      map.set(mwp.match.id, formRowFromApi(mwp));
    }
    return map;
  }, [matchesQ.data]);
  const [edits, setEdits] = useState<Map<string, Partial<FormRow>>>(new Map());

  const mutation = useMutation({
    mutationFn: submitMatches,
    onSuccess: () => {
      setEdits(new Map());
      queryClient.invalidateQueries({ queryKey });
    },
  });

  if (matchesQ.isLoading) return <Section title={title}>Cargando…</Section>;
  if (matchesQ.error) {
    return (
      <Section title={title}>
        <p style={{ color: 'tomato' }}>Error: {matchesQ.error.message}</p>
      </Section>
    );
  }

  const matches = matchesQ.data ?? [];
  const now = Date.now();
  const rowFor = (matchId: string): FormRow => ({
    ...EMPTY_FORM_ROW,
    ...defaults.get(matchId),
    ...edits.get(matchId),
  });

  const onSubmit = () => {
    const predictions: MatchPredictionInput[] = [];
    for (const { match } of matches) {
      const row = rowFor(match.id);
      const reg_home = parseScore(row.regHome);
      const reg_away = parseScore(row.regAway);
      if (reg_home === null || reg_away === null) continue;
      if (new Date(match.kickoff_at).getTime() <= now) continue;

      const isDraw = reg_home === reg_away;
      let advancement_winner_team_id: string | null = null;
      let pens_winner_team_id: string | null = null;
      if (isKnockout) {
        if (isDraw) {
          advancement_winner_team_id = row.advancementWinnerId ?? null;
          if (row.pensYes && advancement_winner_team_id) {
            pens_winner_team_id = advancement_winner_team_id;
          }
        } else {
          advancement_winner_team_id = winningTeamId(match, reg_home, reg_away);
        }
      }
      predictions.push({
        match_id: match.id,
        reg_home,
        reg_away,
        advancement_winner_team_id,
        pens_winner_team_id,
      });
    }
    if (predictions.length === 0) return;
    mutation.mutate({ pickem_group_id: pickemId, parent, predictions });
  };

  const patchEdit = (matchId: string, patch: Partial<FormRow>) => {
    setEdits((prev) => {
      const next = new Map(prev);
      next.set(matchId, { ...prev.get(matchId), ...patch });
      return next;
    });
  };

  return (
    <Section title={title} subtitle={`Cierra: ${formatDeadline(deadline)}`}>
      {matches.map(({ match }) => (
        <MatchRow
          key={match.id}
          match={match}
          row={rowFor(match.id)}
          isKnockout={isKnockout}
          locked={new Date(match.kickoff_at).getTime() <= now}
          teamLabel={(teamId) => teamLabel(teamsById, teamId)}
          onChange={(patch) => patchEdit(match.id, patch)}
        />
      ))}
      <button onClick={onSubmit} disabled={mutation.isPending}>
        {mutation.isPending ? 'Guardando…' : 'Guardar'}
      </button>
      <MutationStatus
        isSuccess={mutation.isSuccess}
        error={mutation.error}
        successText={
          mutation.data ? `Guardadas ${mutation.data.accepted} predicciones.` : 'Guardado.'
        }
      />
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Per-match row
// ---------------------------------------------------------------------------

interface MatchRowProps {
  match: Match;
  row: FormRow;
  isKnockout: boolean;
  locked: boolean;
  teamLabel: (teamId: string | null) => string;
  onChange: (patch: Partial<FormRow>) => void;
}

function MatchRow({ match, row, isKnockout, locked, teamLabel, onChange }: MatchRowProps) {
  const homeLabel = teamLabel(match.home_team_id);
  const awayLabel = teamLabel(match.away_team_id);
  const home = parseScore(row.regHome);
  const away = parseScore(row.regAway);
  const isDraw = home !== null && away !== null && home === away;

  return (
    <div style={{ borderBottom: '1px solid #333', padding: '8px 0' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{ flex: 1, textAlign: 'right' }}>{homeLabel}</span>
        <input
          type="number"
          min={0}
          max={20}
          value={row.regHome}
          disabled={locked}
          onChange={(e) => onChange({ regHome: e.target.value })}
          style={{ width: 48 }}
        />
        <span>—</span>
        <input
          type="number"
          min={0}
          max={20}
          value={row.regAway}
          disabled={locked}
          onChange={(e) => onChange({ regAway: e.target.value })}
          style={{ width: 48 }}
        />
        <span style={{ flex: 1 }}>{awayLabel}</span>
      </div>
      {isKnockout && isDraw && (
        <div style={{ marginTop: 8, paddingLeft: 16, fontSize: 14 }}>
          <p style={{ margin: '4px 0' }}>¿Quién avanza?</p>
          {match.home_team_id && (
            <label style={{ marginRight: 12 }}>
              <input
                type="radio"
                name={`adv-${match.id}`}
                disabled={locked}
                checked={row.advancementWinnerId === match.home_team_id}
                onChange={() => onChange({ advancementWinnerId: match.home_team_id })}
              />{' '}
              {homeLabel}
            </label>
          )}
          {match.away_team_id && (
            <label>
              <input
                type="radio"
                name={`adv-${match.id}`}
                disabled={locked}
                checked={row.advancementWinnerId === match.away_team_id}
                onChange={() => onChange({ advancementWinnerId: match.away_team_id })}
              />{' '}
              {awayLabel}
            </label>
          )}
          <label style={{ display: 'block', marginTop: 4 }}>
            <input
              type="checkbox"
              disabled={locked}
              checked={row.pensYes}
              onChange={(e) => onChange({ pensYes: e.target.checked })}
            />{' '}
            Se decide en penaltis
          </label>
        </div>
      )}
      {locked && <p style={{ fontSize: 12, opacity: 0.7 }}>El partido ya ha empezado.</p>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Helpers + types
// ---------------------------------------------------------------------------

interface FormRow {
  regHome: string;
  regAway: string;
  advancementWinnerId: string | null;
  pensYes: boolean;
}

const EMPTY_FORM_ROW: FormRow = {
  regHome: '',
  regAway: '',
  advancementWinnerId: null,
  pensYes: false,
};

function formRowFromApi(mwp: MatchWithPrediction): FormRow {
  const p = mwp.my_prediction;
  if (!p) return EMPTY_FORM_ROW;
  return {
    regHome: String(p.reg_home),
    regAway: String(p.reg_away),
    advancementWinnerId: p.advancement_winner_team_id,
    pensYes: p.pens_winner_team_id !== null,
  };
}

function parseScore(value: string): number | null {
  if (value.trim() === '') return null;
  const n = Number(value);
  if (!Number.isInteger(n) || n < 0) return null;
  return n;
}

function winningTeamId(match: Match, regHome: number, regAway: number): string | null {
  if (regHome > regAway) return match.home_team_id;
  if (regAway > regHome) return match.away_team_id;
  return null;
}

function teamLabel(teamsById: Map<string, Team>, teamId: string | null): string {
  if (!teamId) return 'Por determinar';
  const t = teamsById.get(teamId);
  return t ? `${t.flag_emoji} ${t.name}` : '???';
}

function formatDeadline(iso: string): string {
  return new Date(iso).toLocaleString();
}
