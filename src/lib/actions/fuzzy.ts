/**
 * Score how well `query` matches `text`, or null for no match. Each
 * whitespace-separated term must appear in order as a subsequence of `text`;
 * terms may match in any order. Higher scores rank first: matches at the very
 * start, at word starts and in consecutive runs score most.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const haystack = text.toLowerCase();
  let total = 0;
  for (const term of terms) {
    const score = termScore(term, haystack);
    if (score === null) return null;
    total += score;
  }
  return total;
}

function termScore(term: string, text: string): number | null {
  let score = 0;
  let from = 0;
  let previous = -2;
  for (const char of term) {
    const at = text.indexOf(char, from);
    if (at === -1) return null;
    score += 1;
    if (at === 0) score += 8;
    else if (/[\s\-_./]/.test(text[at - 1])) score += 5;
    if (at === previous + 1) score += 4;
    else score -= Math.min(at - from, 5) * 0.1;
    previous = at;
    from = at + 1;
  }
  return score;
}
