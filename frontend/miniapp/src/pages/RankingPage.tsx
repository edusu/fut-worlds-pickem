/**
 * Renders the leaderboard for the user's pickem group.
 *
 * Pickem id comes from `Telegram.WebApp.initDataUnsafe.start_param` — the
 * same source as the predictions page. Without it (e.g. opened from the
 * bot's global menu button) we render a hint instead of a query.
 */

import { useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';

import { fetchRanking } from '../lib/api-client';
import { getPickemId } from '../lib/pickem';
import { notifyReady } from '../lib/telegram';

export function RankingPage() {
  useEffect(() => {
    notifyReady();
  }, []);

  const pickemId = getPickemId();

  if (!pickemId) {
    return (
      <main style={{ padding: 16 }}>
        <h1>Ranking</h1>
        <p>
          Abre el Mini App desde el botón del bot en tu grupo para ver el ranking de esa
          pickem.
        </p>
      </main>
    );
  }

  return (
    <main style={{ padding: 16 }}>
      <h1>Ranking</h1>
      <RankingList pickemId={pickemId} />
    </main>
  );
}

function RankingList({ pickemId }: { pickemId: string }) {
  const query = useQuery({
    queryKey: ['ranking', pickemId] as const,
    queryFn: () => fetchRanking(pickemId),
    // The leaderboard only changes when the scorer publishes new points.
    // 30 s stale prevents react-query from refetching on every window focus.
    staleTime: 30_000,
  });

  if (query.isLoading) {
    return <p>Cargando…</p>;
  }
  if (query.error) {
    return <p>No se pudo cargar el ranking.</p>;
  }
  const rows = query.data ?? [];
  if (rows.length === 0) {
    return <p>Aún no hay puntos asignados.</p>;
  }

  return (
    <ol style={{ paddingLeft: 24 }}>
      {rows.map((row) => (
        <li
          key={row.user_id}
          style={{ display: 'flex', justifyContent: 'space-between', padding: '4px 0' }}
        >
          <span>{row.first_name}</span>
          <span style={{ fontVariantNumeric: 'tabular-nums' }}>{row.total_points} pts</span>
        </li>
      ))}
    </ol>
  );
}
