// The lists' search boxes (the Player's and «Ver archivos»): no case, no
// accents and no spaces around, so "cancion" finds "Canción del Mar".

/** Lower case, accents taken off, trimmed. */
export function normalizeForSearch(text: string | null | undefined): string {
  return (text ?? '').normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().trim();
}

/** The items whose title holds `query`; an empty query keeps them all. */
export function filterByTitle<T extends { title: string }>(items: readonly T[], query: string | null | undefined): T[] {
  const needle = normalizeForSearch(query);
  if (!needle) return [...items];
  return items.filter(item => normalizeForSearch(item.title).includes(needle));
}
