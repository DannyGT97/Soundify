/** The wide S used for the Home navigation entry and page title. */
export function homeLogo(size: number): SVGSVGElement {
  const w = 0.5, half = 50 * w, l = 50 - half, r = 50 + half, top = 25, bottom = 75, mid = 50, k = half * 0.55;
  const d = `M${r - 2} ${top + 7} C${r - 8} ${top - 1}, ${50 + k} ${top}, 50 ${top} C${l + 2} ${top}, ${l} ${top + 8}, ${l} ${top + 12} C${l} ${mid - 3}, ${l + 10} ${mid - 1}, 50 ${mid} C${r - 10} ${mid + 1}, ${r} ${mid + 3}, ${r} ${bottom - 12} C${r} ${bottom - 8}, ${r - 2} ${bottom}, 50 ${bottom} C${50 - k} ${bottom}, ${l + 8} ${bottom + 1}, ${l + 2} ${bottom - 7}`;
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('class', 'home-logo'); svg.setAttribute('width', String(size)); svg.setAttribute('height', String(size));
  svg.setAttribute('viewBox', '0 0 100 100'); svg.setAttribute('aria-hidden', 'true');
  const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
  rect.setAttribute('width', '100'); rect.setAttribute('height', '100'); rect.setAttribute('rx', '24'); rect.setAttribute('fill', 'var(--sp-icon-btn-hover)');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  path.setAttribute('d', d); path.setAttribute('fill', 'none'); path.setAttribute('stroke', 'var(--sp-accent)'); path.setAttribute('stroke-width', '15'); path.setAttribute('stroke-linecap', 'round'); path.setAttribute('stroke-linejoin', 'round');
  svg.append(rect, path);
  return svg;
}
