/**
 * "Best thirds" tab — pick exactly 8 of the 48 tournament teams. Reuses
 * `useTeamsById` so the global team set is materialized once across all
 * tabs (the active-groups response carries the same teams).
 *
 * v1 limitation: existing best-thirds predictions are not preloaded — the
 * GET endpoint isn't part of #4. Re-submitting replaces atomically via
 * `POST /api/predictions/best-thirds`.
 */

import { useMutation } from '@tanstack/react-query';
import { useMemo, useState } from 'react';

import { submitBestThirds } from '../../lib/api-client';

import { useActiveGroups, useTeamsById } from './shared';
import { MutationStatus } from './ui';

const REQUIRED = 8;

interface Props {
  pickemId: string;
}

export function BestThirdsTab({ pickemId }: Props) {
  const { isLoading, error } = useActiveGroups();
  const teamsById = useTeamsById();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const mutation = useMutation({ mutationFn: submitBestThirds });

  const teams = useMemo(
    () => [...teamsById.values()].sort((a, b) => a.name.localeCompare(b.name)),
    [teamsById],
  );

  if (isLoading) return <p>Cargando…</p>;
  if (error) return <p style={{ color: 'tomato' }}>Error: {error.message}</p>;
  if (teams.length === 0) return <p>El plazo de los mejores terceros ya cerró.</p>;

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else if (next.size < REQUIRED) {
        next.add(id);
      }
      return next;
    });
  };

  const canSubmit = selected.size === REQUIRED && !mutation.isPending;

  const onSubmit = () => {
    if (!canSubmit) return;
    mutation.mutate({ pickem_group_id: pickemId, team_ids: [...selected] });
  };

  return (
    <section>
      <p>
        Selecciona los <strong>{REQUIRED}</strong> equipos que crees que pasarán como
        mejores terceros — {selected.size} / {REQUIRED} elegidos.
      </p>
      <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
        {teams.map((t) => {
          const isSelected = selected.has(t.id);
          const disabledByCap = !isSelected && selected.size >= REQUIRED;
          return (
            <li key={t.id} style={{ padding: '4px 0' }}>
              <label style={{ opacity: disabledByCap ? 0.5 : 1 }}>
                <input
                  type="checkbox"
                  checked={isSelected}
                  disabled={disabledByCap}
                  onChange={() => toggle(t.id)}
                />{' '}
                {t.flag_emoji} {t.name}
              </label>
            </li>
          );
        })}
      </ul>
      <button onClick={onSubmit} disabled={!canSubmit} style={{ marginTop: 16 }}>
        {mutation.isPending ? 'Guardando…' : 'Guardar'}
      </button>
      <MutationStatus isSuccess={mutation.isSuccess} error={mutation.error} />
    </section>
  );
}
