/**
 * Helpers around the active pickem id. The bot encodes the pickem id
 * as a `?pickem=<uuid>` query parameter on the Mini App launch URL
 * (inline `web_app` buttons do not populate `initDataUnsafe.start_param`
 * — Telegram only sets that field for Direct Link Mini Apps,
 * `t.me/<bot>/<app>?startapp=X`). We read the query parameter first
 * and fall back to `start_param` so future Direct Link wiring keeps
 * working without changes here.
 *
 * The id is captured once at module load — before React Router's
 * `<Navigate>` strips the query string from the URL.
 */

import { uuidSchema } from './api-types';
import { getStartParam } from './telegram';

function resolvePickemId(): string | null {
  const raw = new URLSearchParams(window.location.search).get('pickem') ?? getStartParam();
  if (!raw) return null;
  const parsed = uuidSchema.safeParse(raw);
  return parsed.success ? parsed.data : null;
}

const cachedPickemId = resolvePickemId();

/**
 * Resolve the pickem the user is acting on. `null` when the Mini App
 * was opened without a pickem binding (e.g. via the bot's global menu
 * button, or `/play` in a chat with no pickem yet) — callers should
 * render a "open from your group" hint instead of the predictions UI.
 */
export function getPickemId(): string | null {
  return cachedPickemId;
}
