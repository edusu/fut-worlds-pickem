/**
 * Top-level "Predicciones" page. Hosts three tabs:
 *
 *  - **Partidos** — group-stage and knockout regulation predictions.
 *  - **Posiciones** — final ordering of each World Cup group (1st..4th).
 *  - **Mejores terceros** — set of 8 teams predicted to advance as the
 *    best third-placed teams.
 *
 * The pickem id comes from `Telegram.WebApp.initDataUnsafe.start_param`,
 * which the bot's inline button sets to the pickem's UUID. If the Mini
 * App is opened without it (e.g. via the bot's global menu button) we
 * render a hint instead of the predictions UI — submitting per-pickem
 * predictions is impossible without the binding.
 */

import { useEffect, useState } from 'react';

import { getPickemId } from '../lib/pickem';
import { notifyReady } from '../lib/telegram';

import { BestThirdsTab } from './predictions/BestThirdsTab';
import { MatchesTab } from './predictions/MatchesTab';
import { StandingsTab } from './predictions/StandingsTab';

type TabKey = 'matches' | 'standings' | 'best-thirds';

const TABS: { key: TabKey; label: string }[] = [
  { key: 'matches', label: 'Partidos' },
  { key: 'standings', label: 'Posiciones' },
  { key: 'best-thirds', label: 'Mejores terceros' },
];

export function PredictionsPage() {
  useEffect(() => {
    notifyReady();
  }, []);

  const [active, setActive] = useState<TabKey>('matches');
  const pickemId = getPickemId();

  if (!pickemId) {
    return (
      <main style={{ padding: 16 }}>
        <h1>Predicciones</h1>
        <p>
          Abre el Mini App desde el botón del bot en tu grupo para hacer predicciones de
          esa pickem. Si lo has abierto desde el menú del bot, vuelve al chat del grupo y
          usa el botón <em>Predecir</em> que el bot ancló al crear la pickem.
        </p>
      </main>
    );
  }

  return (
    <main style={{ padding: 16 }}>
      <h1>Predicciones</h1>
      <nav style={{ display: 'flex', gap: 8, marginBottom: 16, borderBottom: '1px solid #333' }}>
        {TABS.map((t) => (
          <button
            key={t.key}
            onClick={() => setActive(t.key)}
            style={{
              padding: '8px 12px',
              background: active === t.key ? '#234' : 'transparent',
              border: 'none',
              borderBottom: active === t.key ? '2px solid #6cf' : '2px solid transparent',
              color: 'inherit',
              cursor: 'pointer',
            }}
          >
            {t.label}
          </button>
        ))}
      </nav>
      {active === 'matches' && <MatchesTab pickemId={pickemId} />}
      {active === 'standings' && <StandingsTab pickemId={pickemId} />}
      {active === 'best-thirds' && <BestThirdsTab pickemId={pickemId} />}
    </main>
  );
}
