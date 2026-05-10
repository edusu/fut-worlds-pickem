/**
 * "Standings" tab — predict the final 1st..4th ordering for each World
 * Cup group. Reuses the `useActiveGroups` cache (the active-groups
 * response embeds each group's 4 teams) so this tab adds no extra GETs.
 *
 * v1 limitation: existing standings predictions are not preloaded — the
 * GET endpoint for them isn't part of #4. Re-submitting always replaces
 * via `POST /api/predictions/standings` (per group).
 */

import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';

import { submitStandings } from '../../lib/api-client';
import type { ActiveGroup, Team } from '../../lib/api-types';

import { useActiveGroups } from './shared';
import { MutationStatus, Section } from './ui';

interface Props {
  pickemId: string;
}

type Positions = [string, string, string, string];

export function StandingsTab({ pickemId }: Props) {
  const { data, isLoading, error } = useActiveGroups();

  if (isLoading) return <p>Cargando…</p>;
  if (error) return <p style={{ color: 'tomato' }}>Error: {error.message}</p>;
  const groups = data ?? [];
  if (groups.length === 0) return <p>El plazo de las clasificaciones ya cerró.</p>;

  return (
    <div>
      {groups.map((g) => (
        <GroupForm key={g.id} group={g} pickemId={pickemId} />
      ))}
    </div>
  );
}

function GroupForm({ group, pickemId }: { group: ActiveGroup; pickemId: string }) {
  const [positions, setPositions] = useState<Positions>(['', '', '', '']);
  const mutation = useMutation({ mutationFn: submitStandings });

  const setPos = (idx: 0 | 1 | 2 | 3, value: string) => {
    setPositions((prev) => {
      const next: Positions = [...prev];
      next[idx] = value;
      return next;
    });
  };

  const allPicked = positions.every((p) => p !== '');
  const allDistinct = new Set(positions).size === positions.length;
  const canSubmit = allPicked && allDistinct && !mutation.isPending;

  const onSubmit = () => {
    if (!canSubmit) return;
    mutation.mutate({
      pickem_group_id: pickemId,
      tournament_group_id: group.id,
      pos1_team_id: positions[0],
      pos2_team_id: positions[1],
      pos3_team_id: positions[2],
      pos4_team_id: positions[3],
    });
  };

  return (
    <Section title={group.name}>
      {[0, 1, 2, 3].map((i) => (
        <PositionRow
          key={i}
          position={i + 1}
          teams={group.teams}
          value={positions[i]}
          onChange={(v) => setPos(i as 0 | 1 | 2 | 3, v)}
        />
      ))}
      {!allDistinct && allPicked && (
        <p style={{ color: 'tomato', fontSize: 13 }}>
          Cada equipo solo puede ocupar una posición.
        </p>
      )}
      <button onClick={onSubmit} disabled={!canSubmit}>
        {mutation.isPending ? 'Guardando…' : 'Guardar'}
      </button>
      <MutationStatus isSuccess={mutation.isSuccess} error={mutation.error} />
    </Section>
  );
}

function PositionRow({
  position,
  teams,
  value,
  onChange,
}: {
  position: number;
  teams: Team[];
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '4px 0' }}>
      <span style={{ width: 32 }}>{position}º</span>
      <select value={value} onChange={(e) => onChange(e.target.value)} style={{ flex: 1 }}>
        <option value="">— Selecciona —</option>
        {teams.map((t) => (
          <option key={t.id} value={t.id}>
            {t.flag_emoji} {t.name}
          </option>
        ))}
      </select>
    </div>
  );
}
