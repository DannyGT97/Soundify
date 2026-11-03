// "hoy", "hace 3 días", "hace 2 semanas"... how the Player's list says when
// each file was downloaded (the Java `RelativeTime`). Pure logic: it picks the
// i18n key and the number; the text is the caller's.

/** An i18n key under `player.added.*` and the number that goes in it. */
export interface Ago { key: string; amount: number }

/**
 * By calendar days (the back already counts them so): something from 23:59
 * yesterday is "ayer", not "hoy". A negative count (a clock set wrong) is today.
 */
export function ago(daysAgo: number): Ago {
  const days = Math.max(0, Math.floor(daysAgo));
  if (days === 0) return { key: 'player.added.today', amount: 0 };
  if (days === 1) return { key: 'player.added.yesterday', amount: 1 };
  if (days < 7) return { key: 'player.added.days', amount: days };
  if (days < 30) return plural('week', Math.floor(days / 7));
  // Capped at 11: days 360-364 would read "12 meses" right before "1 año".
  if (days < 365) return plural('month', Math.min(11, Math.max(1, Math.floor(days / 30))));
  return plural('year', Math.floor(days / 365));
}

function plural(unit: string, amount: number): Ago {
  return { key: `player.added.${unit}${amount === 1 ? '' : 's'}`, amount };
}
