import { useEffect } from 'react';
import { notifyReady } from '../lib/telegram';

/**
 * Lists the active round's matches and lets the user submit predictions.
 * Stub: shows a placeholder until the API endpoints are wired.
 */
export function PredictionsPage() {
  useEffect(() => {
    notifyReady();
  }, []);

  return (
    <main style={{ padding: 16 }}>
      <h1>Predicciones</h1>
      <p>TODO: load active round and render the prediction form.</p>
    </main>
  );
}
