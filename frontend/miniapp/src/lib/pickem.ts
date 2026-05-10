/**
 * Helpers around the active pickem id. The bot opens the Mini App with
 * `start_parameter` set to the pickem's UUID (sub-PR #7); the FE reads it
 * here and forwards it to the API on every request body / query param.
 */

import { uuidSchema } from './api-types';
import { getStartParam } from './telegram';

/**
 * Resolve the pickem the user is acting on. `null` when the Mini App was
 * opened without a `start_param` (e.g. via the bot's global menu button)
 * — callers should render a "open from your group" hint instead of the
 * predictions UI.
 */
export function getPickemId(): string | null {
  const raw = getStartParam();
  if (!raw) return null;
  const parsed = uuidSchema.safeParse(raw);
  return parsed.success ? parsed.data : null;
}
