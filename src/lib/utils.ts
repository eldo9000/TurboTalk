/**
 * Simple segmented-button helper: returns the CSS class for the i-th segment
 * in a row of `count` segments, given that `active` is true.
 */
export function seg(active: boolean, i: number, count: number): string {
  const base  = 'tt-seg-btn';
  const first = i === 0         ? ' tt-seg-first' : '';
  const last  = i === count - 1 ? ' tt-seg-last'  : '';
  const on    = active          ? ' tt-seg-on'    : '';
  return base + first + last + on;
}
