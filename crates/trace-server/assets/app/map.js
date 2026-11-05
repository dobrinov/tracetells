"use strict";

// The product map: areas as tiles sized by their sessions and coloured by how
// often something goes wrong, laid out as a squarified treemap. Full size on
// Product map, in miniature on Pulse.
// The dashboard is plain scripts loaded in order (see index.html); they share one scope.

// Status → tile colour class. Each tile also says its status in words.
const AREA_TONE = { painful: "hurts", rare_painful: "hurts", rare: "warn", healthy: "ok", too_little: "none" };
// Areas with less than this share of the sessions join one "smaller areas" tile.
const SMALL_AREA = 0.03;

// Squarified treemap (Bruls, Huizing, van Wijk): rectangles for `values`
// (largest first) filling a w×h box, each as close to square as rows allow.
function squarify(values, w, h) {
  const total = values.reduce((s, v) => s + v, 0) || 1;
  const areas = values.map((v) => (v / total) * w * h);
  const out = [];
  let r = { x: 0, y: 0, w, h };
  let row = [];
  const worst = (cells, side) => {
    const s = cells.reduce((a, b) => a + b, 0);
    return Math.max((side * side * Math.max(...cells)) / (s * s), (s * s) / (side * side * Math.min(...cells)));
  };
  const lay = (cells) => {
    const s = cells.reduce((a, b) => a + b, 0);
    if (r.w >= r.h) {
      const cw = s / r.h;
      let y = r.y;
      for (const a of cells) {
        out.push({ x: r.x, y, w: cw, h: a / cw });
        y += a / cw;
      }
      r = { x: r.x + cw, y: r.y, w: r.w - cw, h: r.h };
    } else {
      const ch = s / r.w;
      let x = r.x;
      for (const a of cells) {
        out.push({ x, y: r.y, w: a / ch, h: ch });
        x += a / ch;
      }
      r = { x: r.x, y: r.y + ch, w: r.w, h: r.h - ch };
    }
  };
  let i = 0;
  while (i < areas.length) {
    const side = Math.min(r.w, r.h);
    if (!row.length || worst([...row, areas[i]], side) <= worst(row, side)) row.push(areas[i++]);
    else {
      lay(row);
      row = [];
    }
  }
  if (row.length) lay(row);
  return out;
}

// The map for an areas report. `mini` draws Pulse's smaller version.
function productMap(report, { mini = false } = {}) {
  const tops = report.nodes.filter((n) => !n.area);
  const used = tops.filter((n) => n.sessions > 0).sort((a, b) => b.sessions - a.sessions);
  const unused = tops.filter((n) => !n.sessions);
  if (!used.length) return html`<p class="hint">No area had sessions in this window.</p>`;
  const total = used.reduce((s, n) => s + n.sessions, 0);
  const big = used.filter((n) => n.sessions / total >= SMALL_AREA);
  const small = used.filter((n) => n.sessions / total < SMALL_AREA);
  const tiles = [...big.map((n) => ({ node: n, sessions: n.sessions }))];
  if (small.length) tiles.push({ group: small, sessions: small.reduce((s, n) => s + n.sessions, 0) });
  // A box twice as wide as tall (wider for the miniature), in percent.
  const W = 100;
  const H = mini ? 22 : 45;
  const rects = squarify(tiles.map((t) => t.sessions), W, H);
  const unlabelled = [];
  const cells = tiles.map((t, i) => {
    const r = rects[i];
    const style = `left:${r.x.toFixed(2)}%;top:${((r.y / H) * 100).toFixed(2)}%;width:${r.w.toFixed(2)}%;height:${((r.h / H) * 100).toFixed(2)}%`;
    // Too small to read: the tile keeps a title, and is listed under the map.
    const tight = r.w < (mini ? 11 : 9) || r.h / H < (mini ? 0.3 : 0.18);
    if (t.group) {
      return html`<div class="map-tile none" style="${style}" title="Smaller areas: ${t.group.map((n) => n.name).join(", ")}">
        ${tight ? "" : html`<strong>Smaller areas</strong><span>${num(t.sessions)} sessions</span><span class="map-status">${t.group.length} areas, listed below</span>`}
      </div>`;
    }
    const n = t.node;
    const status = (AREA_STATUS[n.status] || [n.status])[0];
    if (tight) unlabelled.push(n);
    return html`<a class="map-tile ${AREA_TONE[n.status] || "none"}" style="${style}" href="${pageUrl("features")}/${encodeURIComponent(n.key)}"
      aria-label="${n.name}: ${num(n.sessions)} sessions, ${status}" title="${n.name}: ${num(n.sessions)} sessions · ${status}">
      ${tight ? "" : html`<strong>${n.name}</strong><span>${num(n.sessions)} sessions${n.problem_rate != null && !mini ? ` · ${pct(n.problem_rate)} hit a problem` : ""}</span><span class="map-status">${status}</span>`}
    </a>`;
  });
  const listed = [...unlabelled, ...small];
  return html`
    <div class="map-box${mini ? " mini" : ""}" style="aspect-ratio:${W} / ${H}">${cells}</div>
    <div class="map-legend hint">
      <span><i class="hurts"></i>Hurts</span><span><i class="warn"></i>Little used</span><span><i class="ok"></i>Healthy</span><span><i class="none"></i>Too little data</span>
    </div>
    ${listed.length || unused.length
      ? html`<p class="map-more hint">${listed.length ? html`Also: ${listed.map((n, i) => html`${i ? ", " : ""}<a href="${pageUrl("features")}/${encodeURIComponent(n.key)}">${n.name}</a> (${num(n.sessions)})`)}. ` : ""}${
          unused.length ? `No sessions in this window: ${unused.map((n) => n.name).join(", ")}.` : ""
        }</p>`
      : ""}
    ${report.source === "suggested" ? html`<p class="hint">These areas are suggested from your URLs. Name your own in <code>trace.json</code>.</p>` : ""}`;
}
