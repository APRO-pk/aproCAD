// ============================================================
// APRO CAD — 2D Sketch tool (workplane drawing overlay)
//
// Self-contained module: createSketcher(ctx) returns the sketch API that
// main.js exposes as `window.__sketcher` and ux.js drives from the ribbon.
//
// Design notes (why, not what):
//  - Everything is authored in plane-local (u, v). The plane frame below is a
//    byte-for-byte mirror of the Rust `SketchPlane::basis()` in
//    crates/document/src/vehicle.rs so a drawn point means the same thing to
//    the geometry compiler and to the pixel under the cursor.
//  - The document is edited by string surgery, not by parsing RON. The user's
//    formatting and comments in every other component survive untouched; that
//    is the entire point of not round-tripping through a parser.
//  - No constraint solver. Snapping is a priority-ordered candidate pass that
//    is recomputed on every move, so behaviour stays predictable.
//  - The overlay lives in its own THREE.Group, added on begin() and fully torn
//    down (geometries + materials disposed) on finish()/cancel()/dispose().
// ============================================================

'use strict';

import { getEditorRon, setEditorRon } from './editor-syntax.js';

// ---------- plane math ----------

/**
 * The plane frame: `world = origin(offset) + u*uAxis + v*vAxis`.
 * Mirrors SketchPlane::basis() in the Rust document crate exactly.
 */
const PLANES = {
  XY: { u: [1, 0, 0], v: [0, 1, 0], normal: [0, 0, 1] },
  XZ: { u: [1, 0, 0], v: [0, 0, 1], normal: [0, -1, 0] },
  YZ: { u: [0, 1, 0], v: [0, 0, 1], normal: [1, 0, 0] },
};

const PLANE_NAMES = ['XY', 'XZ', 'YZ'];

/** Coerce anything into a known plane name (defaults to XY). */
function normPlane(plane) {
  const p = String(plane || 'XY').toUpperCase();
  return PLANES[p] ? p : 'XY';
}

/**
 * Place a plane-local 2D point into world space.
 *
 * Pure and dependency-free on purpose: this is the one piece of the coordinate
 * contract that must be verifiable in isolation (see the module self-check).
 *
 * @param {'XY'|'XZ'|'YZ'} plane
 * @param {[number, number]} uv plane-local (u, v)
 * @param {number} [offset=0] offset along the plane normal
 * @returns {[number, number, number]} world (x, y, z)
 */
export function planeToWorld(plane, uv, offset = 0) {
  const p = PLANES[normPlane(plane)];
  const o = offset || 0;
  return [
    p.normal[0] * o + p.u[0] * uv[0] + p.v[0] * uv[1],
    p.normal[1] * o + p.u[1] * uv[0] + p.v[1] * uv[1],
    p.normal[2] * o + p.u[2] * uv[0] + p.v[2] * uv[1],
  ];
}

/** Inverse of {@link planeToWorld}: world point -> plane-local (u, v). */
export function worldToPlane(plane, xyz, offset = 0) {
  const p = PLANES[normPlane(plane)];
  const o = offset || 0;
  const d = [xyz[0] - p.normal[0] * o, xyz[1] - p.normal[1] * o, xyz[2] - p.normal[2] * o];
  return [
    d[0] * p.u[0] + d[1] * p.u[1] + d[2] * p.u[2],
    d[0] * p.v[0] + d[1] * p.v[1] + d[2] * p.v[2],
  ];
}

// ---------- number / RON formatting ----------

/** RON wants a decimal point on every float: 60 -> "60.0". */
function nf(n) {
  const x = Number(n) || 0; // also folds -0 into 0
  if (!Number.isFinite(x)) return '0.0';
  let s = x.toFixed(4).replace(/0+$/, '');
  if (s.endsWith('.')) s += '0';
  return s;
}

const uv = (p) => `(${nf(p[0])}, ${nf(p[1])})`;

/** Serialize one placed entity as a single RON line. */
function entityRon(e) {
  switch (e.type) {
    case 'line': return `Line(start: ${uv(e.start)}, end: ${uv(e.end)})`;
    case 'rect': return `Rectangle(corner1: ${uv(e.c1)}, corner2: ${uv(e.c2)})`;
    case 'circle': return `Circle(center: ${uv(e.center)}, radius: ${nf(e.radius)})`;
    case 'arc': return `Arc(center: ${uv(e.center)}, radius: ${nf(e.radius)}, `
      + `start_angle: ${nf(e.startAngle)}, end_angle: ${nf(e.endAngle)})`;
    case 'spline': return `Spline(points: [${e.points.map(uv).join(', ')}], closed: ${e.closed ? 'true' : 'false'})`;
    default: return null;
  }
}

/** The whole `Component(...)` block for a sketch, at the given base indent. */
function sketchComponentRon(name, plane, offset, entities, indent = 0) {
  const pad = (n) => ' '.repeat(indent + n);
  const lines = entities.map(entityRon).filter(Boolean);
  const body = lines.length
    ? lines.map((l) => `${pad(12)}${l},`).join('\n')
    : `${pad(12)}// (no entities)`;
  return [
    `${pad(0)}Component(`,
    `${pad(4)}name: ${JSON.stringify(String(name))},`,
    `${pad(4)}material: "",`,
    `${pad(4)}kind: Sketch(SketchParams(`,
    `${pad(8)}plane: ${plane},`,
    `${pad(8)}offset: ${nf(offset)},`,
    `${pad(8)}entities: [`,
    body,
    `${pad(8)}],`,
    `${pad(4)})),`,
    `${pad(0)})`,
  ].join('\n');
}

// ---------- document string surgery ----------
//
// Every scan below treats string literals and comments as opaque so that a
// `"("` inside a name or a `//` note never throws off the paren balance.

/** If `text[i]` starts a string literal, return the index just past it. */
function skipString(text, i) {
  const q = text[i];
  i++;
  while (i < text.length) {
    if (text[i] === '\\') { i += 2; continue; }
    if (text[i] === q) return i + 1;
    i++;
  }
  return i;
}

/** If `text[i]` starts a comment, return the index just past it. */
function skipComment(text, i) {
  if (text[i] === '/' && text[i + 1] === '/') {
    const nl = text.indexOf('\n', i);
    return nl === -1 ? text.length : nl;
  }
  if (text[i] === '/' && text[i + 1] === '*') {
    const end = text.indexOf('*/', i + 2);
    return end === -1 ? text.length : end + 2;
  }
  return i;
}

/**
 * Index of the bracket matching the opener at `open`, or -1 when unbalanced.
 * Handles nesting plus strings/comments.
 */
function matchBracket(text, open, openCh, closeCh) {
  if (text[open] !== openCh) return -1;
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    const ch = text[i];
    if (ch === '"') { i = skipString(text, i) - 1; continue; }
    if (ch === '/' && (text[i + 1] === '/' || text[i + 1] === '*')) { i = skipComment(text, i) - 1; continue; }
    if (ch === openCh) depth++;
    else if (ch === closeCh) { depth--; if (depth === 0) return i; }
  }
  return -1;
}

/** All `name: "..."` values in the document (used for the next free number). */
function collectNames(text) {
  const out = [];
  const re = /Component\s*\(\s*name\s*:\s*"((?:[^"\\]|\\.)*)"/g;
  let m;
  while ((m = re.exec(text))) out.push(m[1].replace(/\\"/g, '"'));
  return out;
}

/** `Sketch`, `Sketch3`, ... -> next unused index. */
function nextSketchName(text) {
  const used = new Set();
  for (const n of collectNames(text)) {
    const m = /^Sketch(\d+)?$/.exec(n);
    if (m) used.add(m[1] ? Number(m[1]) : 1);
  }
  let i = 1;
  while (used.has(i)) i++;
  return 'Sketch' + i;
}

/** Span `[start, end)` of the top-level `[...]` following `key:`. */
function findArraySpan(text, key) {
  const re = new RegExp(`\\b${key}\\s*:`);
  const m = re.exec(text);
  if (!m) return null;
  let p = m.index + m[0].length;
  while (p < text.length && /\s/.test(text[p])) p++;
  while (text[p] === '/' && (text[p + 1] === '/' || text[p + 1] === '*')) {
    p = skipComment(text, p);
    while (p < text.length && /\s/.test(text[p])) p++;
  }
  if (text[p] !== '[') return null;
  const close = matchBracket(text, p, '[', ']');
  return close === -1 ? null : [p, close + 1];
}

/** True when the doc root is a bare `Component(...)` (app's own detection). */
function isBareComponent(text) {
  return text.replace(/^\s*\/\/.*$/gm, '').trimStart().startsWith('Component(');
}

/**
 * Top-level `Component(...)` spans inside a `components: [...]` body.
 * Scans by bracket balance instead of regex so comments and `)` inside strings
 * (a `Points([...])` profile, for instance) cannot desynchronise the spans.
 */
function topLevelComponentSpans(body) {
  const spans = [];
  let i = 0;
  const N = body.length;
  let guard = 0;
  while (i < N && guard++ < 100000) {
    const ch = body[i];
    if (ch === '"') { i = skipString(body, i); continue; }
    if (ch === '/' && (body[i + 1] === '/' || body[i + 1] === '*')) { i = skipComment(body, i); continue; }
    if (body.startsWith('Component', i)) {
      let p = i + 'Component'.length;
      while (p < N && /\s/.test(body[p])) p++;
      if (body[p] === '(') {
        const close = matchBracket(body, p, '(', ')');
        if (close !== -1) {
          spans.push([i, close + 1, body.slice(i, close + 1)]);
          i = close + 1;
          continue;
        }
      }
    }
    i++;
  }
  return spans;
}

/** Indent every line of `block` by `pad` (an all-whitespace string). */
function indentBlock(block, pad) {
  if (!pad) return block;
  return block.split('\n').map((l) => (l ? pad + l : l)).join('\n');
}

/** Leading whitespace of the first non-blank line in a component list body. */
function componentIndent(body) {
  for (const line of body.split('\n')) {
    if (!line.trim()) continue;
    return (line.match(/^[ \t]*/) || [''])[0] || '        ';
  }
  return '        ';
}

/**
 * Leading whitespace of the line a component entry starts on.
 * `start` is the entry's index inside the array body.
 */
function siblingIndent(body, start) {
  const nl = body.lastIndexOf('\n', Math.max(0, start - 1));
  return (body.slice(nl + 1, start).match(/^[ \t]*/) || [''])[0];
}

// ---------- shared geometry helpers (also used by the persistent renderer) ----

/**
 * Sample one entity into a 2D polyline in plane-local (u, v).
 * Mirrors the tessellation in the Rust compiler so what you see matches what
 * will be extruded.
 *
 * @returns {{points: Array<[number,number]>, closed: boolean}|null} null when
 *   the entity is degenerate.
 */
export function sampleSketchEntity(e) {
  if (!e) return null;
  if (e.type === 'line') {
    if (Math.hypot(e.end[0] - e.start[0], e.end[1] - e.start[1]) <= 1e-9) return null;
    return { points: [e.start, e.end], closed: false };
  }
  if (e.type === 'rect') {
    if (Math.abs(e.c2[0] - e.c1[0]) <= 1e-9 || Math.abs(e.c2[1] - e.c1[1]) <= 1e-9) return null;
    const [u0, v0, u1, v1] = [e.c1[0], e.c1[1], e.c2[0], e.c2[1]];
    return { points: [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], closed: true };
  }
  if (e.type === 'circle') {
    if (!(e.radius > 1e-9)) return null;
    const n = 64;
    const pts = [];
    for (let i = 0; i < n; i++) {
      const a = (Math.PI * 2 * i) / n;
      pts.push([e.center[0] + e.radius * Math.cos(a), e.center[1] + e.radius * Math.sin(a)]);
    }
    return { points: pts, closed: true };
  }
  if (e.type === 'arc') {
    if (!(e.radius > 1e-9)) return null;
    let sweep = e.endAngle - e.startAngle;
    while (sweep <= 0) sweep += Math.PI * 2;
    const n = Math.max(2, Math.ceil((sweep / (Math.PI * 2)) * 64));
    const pts = [];
    for (let i = 0; i <= n; i++) {
      const a = e.startAngle + (sweep * i) / n;
      pts.push([e.center[0] + e.radius * Math.cos(a), e.center[1] + e.radius * Math.sin(a)]);
    }
    return { points: pts, closed: false };
  }
  if (e.type === 'spline') {
    if (!e.points || e.points.length < 2) return null;
    if (e.points.length === 2) return { points: e.points.slice(), closed: false };
    const n = e.points.length;
    const at = (i) => (e.closed
      ? e.points[((i % n) + n) % n]
      : e.points[Math.max(0, Math.min(n - 1, i))]);
    const spans = e.closed ? n : n - 1;
    const per = Math.max(6, Math.floor(64 / spans));
    const pts = [];
    for (let s = 0; s < spans; s++) {
      const p0 = at(s - 1), p1 = at(s), p2 = at(s + 1), p3 = at(s + 2);
      for (let k = 0; k < per; k++) {
        const t = k / per, t2 = t * t, t3 = t2 * t;
        const cr = (i) => 0.5 * ((2 * p1[i]) + (-p0[i] + p2[i]) * t
          + (2 * p0[i] - 5 * p1[i] + 4 * p2[i] - p3[i]) * t2
          + (-p0[i] + 3 * p1[i] - 3 * p2[i] + p3[i]) * t3);
        pts.push([cr(0), cr(1)]);
      }
    }
    if (!e.closed) pts.push(e.points[n - 1]);
    return { points: pts, closed: !!e.closed };
  }
  return null;
}

/** Geometric tolerance for "these two points are the same" (sketch units). */
const WELD_TOL = 1e-6;

const samePt = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]) <= WELD_TOL;

/** Signed area of a closed loop; positive is counter-clockwise. */
export function signedArea(pts) {
  if (!pts || pts.length < 3) return 0;
  let a = 0;
  for (let i = 0; i < pts.length; i++) {
    const p = pts[i], q = pts[(i + 1) % pts.length];
    a += p[0] * q[1] - q[0] * p[1];
  }
  return a / 2;
}

/** Join two open chains when an endpoint of one meets an endpoint of the other. */
function joinChains(a, b) {
  const [aHead, aTail] = [a[0], a[a.length - 1]];
  const [bHead, bTail] = [b[0], b[b.length - 1]];
  if (samePt(aTail, bHead)) return a.concat(b.slice(1));
  if (samePt(aTail, bTail)) return a.concat(b.slice(0, -1).reverse());
  if (samePt(aHead, bTail)) return b.concat(a.slice(1));
  if (samePt(aHead, bHead)) return a.slice().reverse().concat(b.slice(1));
  return null;
}

/**
 * Compile entities into closed loops plus leftover open chains.
 *
 * This is the frontend twin of the Rust compiler: it stitches open chains
 * end-to-end (order-independent) and normalises winding to counter-clockwise,
 * so the persistent sketch display draws exactly the profile that will be
 * extruded rather than the raw entity soup.
 *
 * @returns {{loops: Array<Array<[number,number]>>, open: Array<Array<[number,number]>>, skipped: number}}
 */
export function compileSketchEntities(entities) {
  const loops = [];
  const open = [];
  let skipped = 0;
  for (const e of entities || []) {
    const s = sampleSketchEntity(e);
    if (!s) { skipped++; continue; }
    if (s.closed) {
      if (s.points.length >= 3 && Math.abs(signedArea(s.points)) > WELD_TOL) {
        loops.push(signedArea(s.points) < 0 ? s.points.slice().reverse() : s.points);
      } else skipped++;
    } else {
      open.push(s.points);
    }
  }

  let chains = open;
  for (;;) {
    let merged = false;
    outer: for (let i = 0; i < chains.length; i++) {
      for (let j = i + 1; j < chains.length; j++) {
        const joined = joinChains(chains[i], chains[j]);
        if (joined) {
          chains[i] = joined;
          chains.splice(j, 1);
          merged = true;
          break outer;
        }
      }
    }
    if (!merged) break;
  }

  const openChains = [];
  for (const chain of chains) {
    if (chain.length >= 4 && samePt(chain[0], chain[chain.length - 1])) {
      const pts = chain.slice(0, -1);
      if (Math.abs(signedArea(pts)) > WELD_TOL) {
        loops.push(signedArea(pts) < 0 ? pts.reverse() : pts);
        continue;
      }
    }
    openChains.push(chain);
  }
  return { loops, open: openChains, skipped };
}

/**
 * Parse every `Sketch` component out of a document.
 *
 * Text-based like the rest of this module: the document is the source of truth,
 * and a sketch is plain enough that a focused scanner beats round-tripping the
 * whole document through a parser.
 *
 * @param {string} doc RON document text (Vehicle or bare Component)
 * @returns {Array<{name: string, plane: string, offset: number, entities: Array<object>}>}
 */
export function parseSketches(doc) {
  const text = String(doc || '');
  if (!text.trim()) return [];

  let spans;
  if (isBareComponent(text)) {
    spans = topLevelComponentSpans(text);
  } else {
    const arr = findArraySpan(text, 'components');
    if (!arr) return [];
    spans = topLevelComponentSpans(text.slice(arr[0] + 1, arr[1] - 1));
  }

  const out = [];
  for (const span of spans) {
    const block = span[2];
    const nameM = /name\s*:\s*"((?:[^"\\]|\\.)*)"/.exec(block);
    const kindM = /kind\s*:\s*Sketch\s*\(/.exec(block);
    if (!kindM) continue;
    const name = nameM ? nameM[1].replace(/\\"/g, '"') : '';
    const planeM = /plane\s*:\s*(XY|XZ|YZ)\b/.exec(block);
    const offM = /offset\s*:\s*(-?[0-9.]+(?:[eE][-+]?[0-9]+)?)/.exec(block);
    const entM = /entities\s*:\s*\[/.exec(block);
    let entities = [];
    if (entM) {
      const open = block.indexOf('[', entM.index);
      const close = open === -1 ? -1 : matchBracket(block, open, '[', ']');
      if (close !== -1) entities = parseEntityList(block.slice(open + 1, close));
    }
    const visM = /visible\s*:\s*(true|false)/.exec(block);
    out.push({
      name,
      plane: planeM ? planeM[1] : 'XY',
      offset: offM ? Number(offM[1]) : 0,
      entities,
      visible: visM ? visM[1] === 'true' : true,
    });
  }
  return out;
}

/** Parse a 2D tuple `(u, v)` (or `[u, v]`) into a number pair. */
function parseUV(s) {
  const m = /^[([\s]*(-?[0-9.]+(?:[eE][-+]?[0-9]+)?)\s*,\s*(-?[0-9.]+(?:[eE][-+]?[0-9]+)?)[)\]\s]*$/.exec(String(s).trim());
  return m ? [Number(m[1]), Number(m[2])] : null;
}

/** Split a comma-separated argument list at nesting depth 0. */
function splitTopLevel(s) {
  const parts = [];
  let depth = 0, cur = '';
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch === '"') { const e = skipString(s, i); cur += s.slice(i, e); i = e - 1; continue; }
    if (ch === '(' || ch === '[') depth++;
    else if (ch === ')' || ch === ']') depth--;
    if (ch === ',' && depth === 0) { parts.push(cur); cur = ''; continue; }
    cur += ch;
  }
  if (cur.trim()) parts.push(cur);
  return parts.map((p) => p.trim()).filter(Boolean);
}

/** Collect `key: <value>` pairs inside a variant's parentheses. */
function variantFields(inner) {
  const out = {};
  for (const part of splitTopLevel(inner)) {
    const c = part.indexOf(':');
    if (c === -1) continue;
    out[part.slice(0, c).trim()] = part.slice(c + 1).trim();
  }
  return out;
}

/** Parse the body of an `entities: [...]` list. */
function parseEntityList(body) {
  const out = [];
  const re = /\b(Line|Rectangle|Circle|Arc|Spline)\s*\(/g;
  let m;
  while ((m = re.exec(body))) {
    const open = body.indexOf('(', m.index);
    const close = matchBracket(body, open, '(', ')');
    if (close === -1) break;
    const f = variantFields(body.slice(open + 1, close));
    const num = (k) => (f[k] != null && Number.isFinite(Number(f[k]))) ? Number(f[k]) : 0;
    switch (m[1]) {
      case 'Line': {
        const a = parseUV(f.start), b = parseUV(f.end);
        if (a && b) out.push({ type: 'line', start: a, end: b });
        break;
      }
      case 'Rectangle': {
        const a = parseUV(f.corner1), b = parseUV(f.corner2);
        if (a && b) out.push({ type: 'rect', c1: a, c2: b });
        break;
      }
      case 'Circle': {
        const c = parseUV(f.center);
        if (c) out.push({ type: 'circle', center: c, radius: num('radius') });
        break;
      }
      case 'Arc': {
        const c = parseUV(f.center);
        if (c) out.push({
          type: 'arc', center: c, radius: num('radius'),
          startAngle: num('start_angle'), endAngle: num('end_angle'),
        });
        break;
      }
      case 'Spline': {
        const pts = [];
        const bm = /points\s*:\s*\[([\s\S]*)\]/.exec(body.slice(open + 1, close));
        if (bm) {
          for (const chunk of splitTopLevel(bm[1])) {
            const uv2 = parseUV(chunk);
            if (uv2) pts.push(uv2);
          }
        }
        if (pts.length >= 2) {
          out.push({ type: 'spline', points: pts, closed: /closed\s*:\s*true/.test(f.closed || '') });
        }
        break;
      }
      default: break;
    }
    re.lastIndex = close + 1;
  }
  return out;
}

/**
 * Insert or replace a sketch component, preserving every other byte of the
 * document. `data` is `{ name, plane, offset, entities }`; the sibling
 * indentation of an existing entry is reused so the result stays tidy.
 * @returns {{text: string, replaced: boolean}}
 */
function upsertSketchComponent(doc, data) {
  const name = data.name;
  const block = (indent) => sketchComponentRon(name, data.plane, data.offset, data.entities, indent);

  if (isBareComponent(doc)) {
    // A bare Component has nowhere to put a sibling: promote the document to a
    // Vehicle (exactly what the app's own wrapRon does) and keep it intact.
    const lead = (doc.match(/^\s*(?:\/\/[^\n]*\n)*/) || [''])[0];
    const rest = doc.slice(lead.length).replace(/\s+$/, '');
    const wrapped = `${lead}Vehicle(\n`
      + `    name: "Vehicle",\n`
      + `    units: Millimeters,\n`
      + `    components: [\n`
      + `${indentBlock(rest, '        ')},\n`
      + `${block(8)},\n`
      + `    ],\n`
      + `)`;
    return { text: wrapped, replaced: false };
  }

  const arr = findArraySpan(doc, 'components');
  if (!arr) return { text: doc, replaced: false }; // not a Vehicle we understand

  const body = doc.slice(arr[0] + 1, arr[1] - 1);
  const target = topLevelComponentSpans(body).find((sp) => sp[2].includes(`name: ${JSON.stringify(name)}`));
  if (target) {
    // Replace in place: the entry keeps its slot, its indentation and its
    // trailing comma; its neighbours keep every byte they had.
    const lineStart = body.lastIndexOf('\n', Math.max(0, target[0] - 1)) + 1;
    const indent = siblingIndent(body, target[0]).length;
    const suffix = body.slice(target[1]).match(/^[ \t]*,/);
    return {
      text: doc.slice(0, arr[0] + 1) + body.slice(0, lineStart) + block(indent)
        + (suffix ? suffix[0] : '')
        + body.slice(target[1] + (suffix ? suffix[0].length : 0))
        + doc.slice(arr[1] - 1),
      replaced: true,
    };
  }

  // Append as the last entry, using the sibling indent verbatim.
  const siblings = topLevelComponentSpans(body);
  const base = siblings.length ? siblingIndent(body, siblings[0][0]) : componentIndent(body);
  const trimmed = body.replace(/[\s,]+$/, '');
  const newBody = trimmed
    ? `${trimmed},\n${block(base.length)}`
    : `\n${block(base.length)}`;
  return { text: doc.slice(0, arr[0] + 1) + newBody + `\n${base}` + doc.slice(arr[1] - 1), replaced: false };
}

// ---------- small DOM helpers ----------

function esc(s) {
  return String(s == null ? '' : s)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function el(tag, cls, html) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (html != null) e.innerHTML = html;
  return e;
}

// ============================================================
// createSketcher
// ============================================================

/**
 * Build the sketch tool.
 *
 * @param {object} ctx
 * @param {object} ctx.THREE           the three module namespace
 * @param {HTMLCanvasElement} ctx.canvas renderer.domElement
 * @param {() => object} ctx.getCamera live camera getter (the app swaps instances)
 * @param {() => object|null} ctx.getControls live OrbitControls getter
 * @param {object} ctx.scene           THREE.Scene to host the overlay group
 * @param {() => Promise<void>} ctx.evaluate re-evaluation after a commit
 * @param {(msg: string, kind?: string, ms?: number) => void} [ctx.notify]
 * @returns {object} sketcher API
 */
export function createSketcher(ctx) {
  const THREE = ctx.THREE;
  const canvas = ctx.canvas;
  const scene = ctx.scene;
  const getCamera = ctx.getCamera || (() => null);
  const getControls = ctx.getControls || (() => null);
  const notify = typeof ctx.notify === 'function' ? ctx.notify : () => {};

  if (!THREE || !canvas || !scene || typeof getCamera !== 'function') {
    // Never throw during boot: a half-wired host should fail soft.
    const noop = () => {};
    return {
      isActive: () => false, activeTool: () => null, begin: noop, setPlane: noop,
      getPlane: () => 'XY', cancel: noop, finish: noop, undoLast: noop,
      getEntityCount: () => 0, tick: noop, setGridStep: noop, dispose: noop,
    };
  }

  // ---------- state ----------
  let active = false;
  let tool = null;
  let plane = 'XY';
  let offset = 0;
  let gridStep = 1.0;
  let entities = [];
  let pending = null;      // in-progress entity
  let chainFrom = null;    // polyline carry-over start point (line tool)
  let preview = null;      // live snapped point under the cursor
  let snapHit = null;      // last snap candidate { point, label, kind }
  let shiftDown = false;
  let saving = false;

  // controls the app owns: remember their enabled state so we restore it and
  // never fight the iso-morph, which toggles `enabled` on its own.
  const controlsState = new Map();

  const raycaster = new THREE.Raycaster();
  const ndc = new THREE.Vector2();
  const planeObj = new THREE.Plane();
  const hitVec = new THREE.Vector3();
  const rayDir = new THREE.Vector3();
  const vWorld = new THREE.Vector3();

  // ---------- overlay group ----------
  const group = new THREE.Group();
  group.name = '__sketchOverlay';
  group.visible = false;
  // Drawing overlay, not a shaded object: always in front of the model.
  group.renderOrder = 999;

  const placedSurface = new THREE.Group();
  const previewSurface = new THREE.Group();
  group.add(placedSurface, previewSurface);

  /** Materials/geometries we own, disposed together on teardown. */
  const owned = [];
  const track = (o) => { owned.push(o); return o; };

  const matGridFine = track(new THREE.LineBasicMaterial({ color: 0x00e5b0, transparent: true, opacity: 0.10, depthTest: false }));
  const matGridCoarse = track(new THREE.LineBasicMaterial({ color: 0x00e5b0, transparent: true, opacity: 0.22, depthTest: false }));
  const matAxis = track(new THREE.LineBasicMaterial({ color: 0x8b7bff, transparent: true, opacity: 0.55, depthTest: false }));
  const matPlaced = track(new THREE.LineBasicMaterial({ color: 0x00e5b0, depthTest: false }));
  const matPreview = track(new THREE.LineDashedMaterial({
    color: 0xe8e8f2, dashSize: 4, gapSize: 3, transparent: true, opacity: 0.85, depthTest: false,
  }));
  const matSnap = track(new THREE.LineBasicMaterial({ color: 0x00e5b0, depthTest: false }));

  // Snap marker: a diamond plus a centre dot, kept at a constant screen size.
  const snapMarker = new THREE.LineSegments(track(new THREE.BufferGeometry()), matSnap);
  snapMarker.renderOrder = 1001;
  snapMarker.visible = false;
  previewSurface.add(snapMarker);
  const snapDot = new THREE.Points(track(new THREE.BufferGeometry()), track(new THREE.PointsMaterial({
    color: 0x00e5b0, size: 5, sizeAttenuation: false, depthTest: false,
  })));
  snapDot.renderOrder = 1002;
  snapDot.visible = false;
  previewSurface.add(snapDot);

  const gridObj = new THREE.LineSegments(track(new THREE.BufferGeometry()), matGridFine);
  gridObj.renderOrder = 1000;
  const gridCoarseObj = new THREE.LineSegments(track(new THREE.BufferGeometry()), matGridCoarse);
  gridCoarseObj.renderOrder = 1000;
  const axesObj = new THREE.LineSegments(track(new THREE.BufferGeometry()), matAxis);
  axesObj.renderOrder = 1000;
  group.add(gridObj, gridCoarseObj, axesObj);

  // ---------- HTML overlay (banner, dimension readout, rotate hint) ----------
  // Styling lives in ui.css (.sk-*), except `display`, which JS owns so the
  // elements can be shown/hidden without fighting the stylesheet.
  const wrap = document.getElementById('viewport-canvas-wrap') || document.body;
  const banner = el('div', 'sk-banner', '');
  const dimLabel = el('div', 'sk-dim', '');
  const snapLabel = el('div', 'sk-snap-label', '');
  const planePanel = buildPlanePanel();
  banner.style.display = 'none';
  dimLabel.style.display = 'none';
  snapLabel.style.display = 'none';
  planePanel.root.style.display = 'none';
  wrap.appendChild(banner);
  wrap.appendChild(dimLabel);
  wrap.appendChild(snapLabel);
  wrap.appendChild(planePanel.root);

  // ---------- geometry builders ----------

  const tmpV = new THREE.Vector3();

  function setGridGeometry() {
    // Scale the lattice so it stays useful at any zoom, but keep the line count
    // bounded — this is a hint, not a wireframe of the universe.
    const cam = getCamera();
    let reach = gridStep * 20;
    if (cam) {
      const p = cam.getWorldPosition ? cam.getWorldPosition(tmpV) : null;
      const o = planeOriginVec();
      const d = p ? p.distanceTo(hitVec.clone().set(o[0], o[1], o[2])) : 0;
      if (d > 0) reach = Math.max(reach, d * 0.9);
    }
    // Coarse lines every 5 fine lines; bounded near ~200 segments per lattice.
    const divisions = Math.max(4, Math.min(60, Math.round(reach / gridStep)));
    const half = Math.round(divisions / 2);
    const extent = half * gridStep;
    const fine = [];
    const coarse = [];
    for (let i = -half; i <= half; i++) {
      if (i === 0) continue;                       // the axes own the centre lines
      const t = i * gridStep;
      const arr = (i % 5 === 0) ? coarse : fine;
      arr.push(-extent, t, 0, extent, t, 0, t, -extent, 0, t, extent, 0);
    }
    gridObj.geometry.dispose();
    gridObj.geometry = new THREE.BufferGeometry();
    gridObj.geometry.setAttribute('position', new THREE.Float32BufferAttribute(fine, 3));
    gridCoarseObj.geometry.dispose();
    gridCoarseObj.geometry = new THREE.BufferGeometry();
    gridCoarseObj.geometry.setAttribute('position', new THREE.Float32BufferAttribute(coarse, 3));
    const a = extent;
    axesObj.geometry.dispose();
    axesObj.geometry = new THREE.BufferGeometry();
    axesObj.geometry.setAttribute('position',
      new THREE.Float32BufferAttribute([-a, 0, 0, a, 0, 0, 0, -a, 0, 0, a, 0], 3));
  }

  /**
   * Fresh polyline for one entity. Returns null for degenerate input.
   *
   * Points are authored in PLANE-LOCAL (u, v, 0) on purpose: the overlay group
   * owns a matrix that maps the plane frame into world space (see
   * `applyPlaneTransform`), so converting to world here as well would apply the
   * frame twice and collapse the geometry toward the origin on XZ/YZ.
   *
   * Sampling comes from the shared `sampleSketchEntity` so the live overlay and
   * the persistent sketch renderer can never diverge; only the ribbon tessellation
   * detail differs (smoother here, since this is the thing being drawn).
   */
  function entityGeometry(e) {
    const s = sampleSketchEntity(e);
    if (!s) return null;
    const pts = [];
    for (const p of s.points) pts.push(p[0], p[1], 0);
    // A closed loop must return to its first point to draw as an outline.
    if (s.closed && s.points.length) {
      pts.push(s.points[0][0], s.points[0][1], 0);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
    return g;
  }

  /** Rebuild the placed-entity lines (cheap: entity counts are small). */
  function rebuildPlaced() {
    while (placedSurface.children.length) {
      const c = placedSurface.children.pop();
      if (c.geometry) c.geometry.dispose();
    }
    for (const e of entities) {
      const g = entityGeometry(e);
      if (!g) continue;
      const line = new THREE.Line(g, matPlaced);
      line.renderOrder = 1000;
      placedSurface.add(line);
    }
  }

  /** Rebuild the in-progress ghost. */
  function rebuildPreview() {
    while (previewSurface.children.length) {
      const c = previewSurface.children.pop();
      if (c.geometry && c !== snapMarker.geometry && c !== snapDot.geometry) c.geometry.dispose();
    }
    if (!preview) {
      snapMarker.visible = false;
      snapDot.visible = false;
      return;
    }
    // snapMarker/snapDot are re-added after the ghost so they always draw on top.
    previewSurface.add(snapMarker, snapDot);

    const cur = liveSpec();
    if (cur) {
      const g = entityGeometry(cur);
      if (g) {
        const line = new THREE.Line(g, matPreview);
        line.computeLineDistances();
        line.renderOrder = 1001;
        previewSurface.add(line);
      }
    }
    // Rubber-band handle from the anchor to the cursor for multi-click tools.
    // Plane-local like every other overlay point (the group matrix places it).
    const anchor = anchorPoint();
    if (anchor && preview && tool !== 'circle' && tool !== 'arc') {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(
        [anchor[0], anchor[1], 0, preview[0], preview[1], 0], 3));
      const l = new THREE.Line(g, matPreview);
      l.computeLineDistances();
      l.renderOrder = 1001;
      previewSurface.add(l);
    }
    sizeMarker();
  }

  /** The point the current tool measures from. */
  function anchorPoint() {
    if (!pending) return chainFrom;
    switch (tool) {
      case 'line': return pending.start;
      case 'rect': return pending.c1;
      case 'circle': return pending.center;
      case 'arc':
        if (pending.stage === 1) return pending.center;
        if (pending.stage === 2) return pending.start;
        return pending.center;
      default: return null;
    }
  }

  /** The entity the current tool would commit if the pointer clicked now. */
  function liveSpec() {
    if (!preview) return null;
    if (tool === 'line') {
      const s = chainFrom || (pending && pending.start);
      return s ? { type: 'line', start: s, end: preview } : null;
    }
    if (tool === 'rect' && pending && pending.stage === 1) {
      return { type: 'rect', c1: pending.c1, c2: preview };
    }
    if (tool === 'circle' && pending && pending.stage === 1) {
      const r = Math.hypot(preview[0] - pending.center[0], preview[1] - pending.center[1]);
      return { type: 'circle', center: pending.center, radius: r };
    }
    if (tool === 'arc' && pending && pending.stage === 2) {
      const r = Math.hypot(pending.start[0] - pending.center[0], pending.start[1] - pending.center[1]);
      return {
        type: 'arc', center: pending.center, radius: r,
        startAngle: Math.atan2(pending.start[1] - pending.center[1], pending.start[0] - pending.center[0]),
        endAngle: Math.atan2(preview[1] - pending.center[1], preview[0] - pending.center[0]),
      };
    }
    if (tool === 'spline') {
      const pts = (pending && pending.points ? pending.points.slice() : []);
      if (chainFrom) pts.unshift(chainFrom);
      if (!pts.length) return null;
      const near = Math.hypot(preview[0] - pts[0][0], preview[1] - pts[0][1]);
      if (pts.length >= 2 && near <= snapTolerance()) return { type: 'spline', points: pts, closed: true };
      return { type: 'spline', points: [...pts, preview], closed: false };
    }
    return null;
  }

  // ---------- snapping ----------

  /** World-space snap tolerance converted to plane units at the cursor. */
  function snapTolerance() {
    const cam = getCamera();
    if (!cam) return gridStep;
    const w = planeToWorld(plane, preview || [0, 0], offset);
    const p = cam.getWorldPosition ? cam.getWorldPosition(tmpV) : null;
    const d = p ? p.distanceTo(hitVec.clone().set(w[0], w[1], w[2])) : 50;
    const h = canvas.clientHeight || 600;
    const fov = (cam.isPerspectiveCamera ? cam.fov : 45) * Math.PI / 180;
    // ~10 CSS pixels' worth of plane units, so the tolerance feels the same at
    // any zoom level (and is a constant 10 px in the geometric snap pass).
    return Math.max(0.01, (2 * Math.max(1, d) * Math.tan(fov / 2) * 10) / h);
  }

  /** Project world -> canvas CSS pixels, or null when behind the camera. */
  function worldToScreen(p) {
    const cam = getCamera();
    if (!cam) return null;
    vWorld.set(p[0], p[1], p[2]);
    vWorld.project(cam);
    if (vWorld.z > 1) return null;
    return [((vWorld.x + 1) / 2) * canvas.clientWidth, ((1 - vWorld.y) / 2) * canvas.clientHeight];
  }

  /** Every candidate point on an already-placed entity, priority-tagged. */
  function candidatesFor(e) {
    const out = [];
    const add = (priority, kind, label, p) => out.push({ priority, kind, label, point: p });
    if (e.type === 'line') {
      add(0, 'endpoint', 'endpoint', e.start);
      add(0, 'endpoint', 'endpoint', e.end);
      add(1, 'midpoint', 'midpoint', [(e.start[0] + e.end[0]) / 2, (e.start[1] + e.end[1]) / 2]);
    } else if (e.type === 'rect') {
      const [u0, v0, u1, v1] = [e.c1[0], e.c1[1], e.c2[0], e.c2[1]];
      add(0, 'endpoint', 'endpoint', [u0, v0]);
      add(0, 'endpoint', 'endpoint', [u1, v0]);
      add(0, 'endpoint', 'endpoint', [u1, v1]);
      add(0, 'endpoint', 'endpoint', [u0, v1]);
      add(1, 'midpoint', 'midpoint', [(u0 + u1) / 2, v0]);
      add(1, 'midpoint', 'midpoint', [u1, (v0 + v1) / 2]);
      add(1, 'midpoint', 'midpoint', [(u0 + u1) / 2, v1]);
      add(1, 'midpoint', 'midpoint', [u0, (v0 + v1) / 2]);
      add(2, 'center', 'center', [(u0 + u1) / 2, (v0 + v1) / 2]);
    } else if (e.type === 'circle' || e.type === 'arc') {
      const c = e.center, r = e.radius;
      add(2, 'center', 'center', c);
      for (const q of [[1, 0], [0, 1], [-1, 0], [0, -1]]) {
        add(3, 'quadrant', 'quadrant', [c[0] + r * q[0], c[1] + r * q[1]]);
      }
      if (e.type === 'arc') {
        let sweep = e.endAngle - e.startAngle;
        while (sweep <= 0) sweep += Math.PI * 2;
        for (const a of [e.startAngle, e.endAngle, e.startAngle + sweep / 2]) {
          add(0, 'endpoint', 'endpoint', [c[0] + r * Math.cos(a), c[1] + r * Math.sin(a)]);
        }
      }
    } else if (e.type === 'spline') {
      e.points.forEach((p) => add(0, 'endpoint', 'endpoint', p));
      for (let i = 0; i + 1 < e.points.length; i++) {
        const a = e.points[i], b = e.points[i + 1];
        add(1, 'midpoint', 'midpoint', [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]);
      }
    }
    return out;
  }

  /**
   * Priority-ordered snap pass. Endpoint > midpoint > center > quadrant >
   * horizontal/vertical inference > grid. Ortho (Shift) and the axis lock
   * applied by the previous click outrank everything geometric.
   */
  function snapAt(raw) {
    const from = anchorPoint();
    const lock = (pending && pending.lock) || null;
    const axisLock = (point, axis, label) => {
      const base = from || [0, 0];
      const p = axis === 'h' ? [point[0], base[1]] : [base[0], point[1]];
      return { point: p, kind: axis === 'h' ? 'horizontal' : 'vertical', label, axisLocked: true };
    };

    if (lock === 'h' || lock === 'v') return axisLock(raw, lock, lock === 'h' ? 'horizontal' : 'vertical');

    // Geometric candidates, in screen space (so tolerance is zoom-independent).
    const cur = preview || raw;
    const curScreen = worldToScreen(planeToWorld(plane, cur, offset));
    const rawScreen = worldToScreen(planeToWorld(plane, raw, offset));
    let best = null;
    if (curScreen) {
      let bestPx = Infinity;
      for (const e of entities) {
        for (const c of candidatesFor(e)) {
          const s = worldToScreen(planeToWorld(plane, c.point, offset));
          if (!s) continue;
          const d = Math.hypot(s[0] - curScreen[0], s[1] - curScreen[1]);
          if (d > 10) continue;
          if (!best || c.priority < best.priority || (c.priority === best.priority && d < bestPx - 0.5)) {
            best = c; bestPx = d;
          }
        }
      }
    }
    if (best) return { point: best.point, kind: best.kind, label: best.label };

    // Horizontal / vertical inference against the anchor, at ~5 degrees.
    if (from && rawScreen) {
      const anchorS = worldToScreen(planeToWorld(plane, from, offset));
      if (anchorS) {
        const dx = rawScreen[0] - anchorS[0], dy = rawScreen[1] - anchorS[1];
        const deg = (a, b) => Math.abs(Math.atan2(a, b)) * 180 / Math.PI;
        if (shiftDown) {
          return Math.abs(dx) >= Math.abs(dy)
            ? axisLock(raw, 'h', 'horizontal')
            : axisLock(raw, 'v', 'vertical');
        }
        if (deg(dy, dx) <= 5) return axisLock(raw, 'h', 'horizontal');
        if (deg(dx, dy) <= 5) return axisLock(raw, 'v', 'vertical');
      }
    }

    // Last resort (and the only snap available before anything is drawn).
    if (gridStep > 0) {
      const g = [Math.round(raw[0] / gridStep) * gridStep, Math.round(raw[1] / gridStep) * gridStep];
      return { point: g, kind: 'grid', label: 'grid' };
    }
    return { point: raw, kind: null, label: '' };
  }

  // ---------- picking ----------

  function planeOriginVec() {
    return planeToWorld(plane, [0, 0], offset);
  }

  function updatePlaneObject() {
    const o = planeOriginVec();
    planeObj.setFromNormalAndCoplanarPoint(
      new THREE.Vector3(...PLANES[plane].normal), new THREE.Vector3(o[0], o[1], o[2]));
  }

  /**
   * Raycast the pointer onto the workplane.
   * @returns {[number, number]|null} plane-local (u, v), or null if the ray
   *   cannot reach the plane (near-parallel view, or outside the camera).
   */
  function pick(clientX, clientY) {
    const cam = getCamera();
    if (!cam) return null;
    const rect = canvas.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return null;
    ndc.set(
      ((clientX - rect.left) / rect.width) * 2 - 1,
      -((clientY - rect.top) / rect.height) * 2 + 1
    );
    raycaster.setFromCamera(ndc, cam);
    updatePlaneObject();
    rayDir.copy(raycaster.ray.direction).normalize();
    if (Math.abs(rayDir.dot(planeObj.normal)) < 1e-4) return null; // edge-on
    const hit = raycaster.ray.intersectPlane(planeObj, hitVec);
    if (!hit) return null;
    return worldToPlane(plane, [hit.x, hit.y, hit.z], offset);
  }

  // ---------- commit / undo ----------

  function pushEntity(e) {
    if (!e) return false;
    entities.push(e);
    rebuildPlaced();
    updateBanner();
    return true;
  }

  /** Drop any incomplete state and restart the current tool cleanly. */
  function resetTool() {
    pending = null;
    chainFrom = null;
    preview = null;
    snapHit = null;
    rebuildPreview();
    hide(dimLabel);
    hide(snapLabel);
    updateBanner();
  }

  function cancelInProgress() {
    if (!pending && !chainFrom) return false;
    resetTool();
    notify('Sketch element discarded', 'info', 1200);
    return true;
  }

  function finishSpline() {
    const pts = (pending && pending.points ? pending.points.slice() : []);
    if (chainFrom) pts.unshift(chainFrom);
    if (pts.length < 2) {
      resetTool();
      notify('A spline needs at least 2 points', 'warn', 1500);
      return;
    }
    const tol = snapTolerance();
    const closed = pts.length >= 3
      && Math.hypot(pts[pts.length - 1][0] - pts[0][0], pts[pts.length - 1][1] - pts[0][1]) <= tol;
    const finalPts = closed ? pts.slice(0, -1) : pts;
    resetTool();
    pushEntity({ type: 'spline', points: finalPts, closed });
  }

  // ---------- pointer handlers ----------

  function onPointerDown(ev) {
    if (!active) return;
    if (ev.button === 1) return; // middle: ignore (no orbit while sketching)
    if (ev.button === 2) {
      // Right-click ends the current entity/chain, like every CAD app.
      ev.preventDefault();
      if (pending || chainFrom) cancelInProgress();
      else endChain();
      return;
    }
    if (ev.button !== 0) return;

    const now = performance.now();
    const isDouble = dblTime && now - dblTime < 350
      && Math.hypot(ev.clientX - dblX, ev.clientY - dblY) < 6;
    dblTime = now; dblX = ev.clientX; dblY = ev.clientY;

    const uvHit = pick(ev.clientX, ev.clientY);
    if (!uvHit) {
      notify('View is edge-on to the ' + plane + ' plane — orbit the view to draw', 'warn', 2000);
      return;
    }
    const snapped = snapAt(uvHit);
    const p = snapped.point;

    if (tool === 'line') {
      if (!chainFrom) {
        chainFrom = p;
        pending = { start: p, lock: null };
      } else {
        const s = chainFrom;
        if (Math.hypot(p[0] - s[0], p[1] - s[1]) > 1e-9) pushEntity({ type: 'line', start: s, end: p });
        else notify('Zero-length line discarded', 'warn', 1200);
        chainFrom = p;           // polyline-style chaining
        pending = { start: p, lock: snapped.axisLocked ? snapped.kind : null };
      }
    } else if (tool === 'rect') {
      if (!pending) {
        pending = { stage: 1, c1: p, lock: snapped.axisLocked ? snapped.kind : null };
      } else {
        const c1 = pending.c1;
        if (Math.abs(p[0] - c1[0]) > 1e-9 && Math.abs(p[1] - c1[1]) > 1e-9) {
          pushEntity({ type: 'rect', c1, c2: p });
        } else {
          notify('Zero-size rectangle discarded', 'warn', 1200);
        }
        pending = null;
      }
    } else if (tool === 'circle') {
      if (!pending) {
        pending = { stage: 1, center: p, lock: snapped.axisLocked ? snapped.kind : null };
      } else {
        const r = Math.hypot(p[0] - pending.center[0], p[1] - pending.center[1]);
        if (r > 1e-9) pushEntity({ type: 'circle', center: pending.center, radius: r });
        else notify('Zero-radius circle discarded', 'warn', 1200);
        pending = null;
      }
    } else if (tool === 'arc') {
      if (!pending) {
        pending = { stage: 1, center: p, lock: snapped.axisLocked ? snapped.kind : null };
      } else if (pending.stage === 1) {
        if (Math.hypot(p[0] - pending.center[0], p[1] - pending.center[1]) > 1e-9) {
          pending.stage = 2; pending.start = p;
          pending.lock = snapped.axisLocked ? snapped.kind : null;
        } else {
          notify('Arc needs a radius: click further from the centre', 'warn', 1500);
        }
      } else {
        const c = pending.center;
        const r = Math.hypot(pending.start[0] - c[0], pending.start[1] - c[1]);
        let s = Math.atan2(pending.start[1] - c[1], pending.start[0] - c[0]);
        let e = Math.atan2(p[1] - c[1], p[0] - c[0]);
        if (e <= s) e += Math.PI * 2;    // always the CCW sweep, like the compiler
        pushEntity({ type: 'arc', center: c, radius: r, startAngle: s, endAngle: e });
        pending = null;
      }
    } else if (tool === 'spline') {
      if (isDouble) {
        finishSpline();
        return;
      }
      if (!pending) pending = { points: [] };
      const pts = pending.points;
      const last = pts.length ? pts[pts.length - 1] : chainFrom;
      if (last && Math.hypot(p[0] - last[0], p[1] - last[1]) <= 1e-9) return; // ignore repeats
      pts.push(p);
    }

    preview = p;
    snapHit = snapped;
    rebuildPreview();
    updateBanner();
  }

  function onPointerMove(ev) {
    if (!active) return;
    lastMouse.x = ev.clientX;
    lastMouse.y = ev.clientY;
    const uvHit = pick(ev.clientX, ev.clientY);
    if (!uvHit) {
      preview = null;
      rebuildPreview();
      hide(snapLabel);
      showRotateHint();
      return;
    }
    hideRotateHint();
    const snapped = snapAt(uvHit);
    preview = snapped.point;
    snapHit = snapped;
    rebuildPreview();
    updateDimensionLabel(ev);
    updateSnapLabel(ev);
  }

  function onPointerLeave() {
    if (!active) return;
    preview = null;
    rebuildPreview();
    hide(dimLabel);
    hide(snapLabel);
  }

  function onContextMenu(ev) {
    if (active) ev.preventDefault();
  }

  function onWheel(ev) {
    if (active) ev.preventDefault(); // no zoom while drawing: the plane must stay put
  }

  // ---------- keyboard ----------

  function editableTarget(t) {
    const tag = (t && t.tagName) || '';
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || (t && t.isContentEditable);
  }

  function onKeyDown(ev) {
    if (ev.key === 'Shift') shiftDown = true;
    if (!active) return;
    if (editableTarget(ev.target)) return;

    if (ev.key === 'Escape') {
      ev.preventDefault();
      ev.stopPropagation();
      if (cancelInProgress()) return;
      cancel();
      notify('Sketch mode ended', 'info', 1500);
      return;
    }
    if (ev.key === 'Enter') {
      ev.preventDefault();
      ev.stopPropagation();
      if (tool === 'spline' && pending) finishSpline();
      else if (pending || chainFrom) endChain();
      return;
    }
    if (ev.key === 'Backspace' || ev.key === 'Delete') {
      ev.preventDefault();
      if (pending || chainFrom) cancelInProgress();
      else undoLast();
    }
  }

  function onKeyUp(ev) {
    if (ev.key === 'Shift') {
      shiftDown = false;
      if (active && preview) {
        const uvHit = pick(lastMouse.x, lastMouse.y);
        if (uvHit) { snapHit = snapAt(uvHit); preview = snapHit.point; rebuildPreview(); }
      }
    }
  }

  /** Commit the pending chain and return to a clean state for the same tool. */
  function endChain() {
    if (tool === 'spline' && pending) { finishSpline(); return; }
    chainFrom = null;
    pending = null;
    rebuildPreview();
    hide(dimLabel);
    updateBanner();
  }

  // ---------- labels ----------

  const lastMouse = { x: 0, y: 0 };

  function positionAtLabel(node, clientX, clientY) {
    const wrapR = wrap.getBoundingClientRect();
    node.style.left = (clientX - wrapR.left) + 'px';
    node.style.top = (clientY - wrapR.top) + 'px';
  }

  function show(node) { node.style.display = 'block'; }
  function hide(node) { node.style.display = 'none'; }

  function updateSnapLabel(ev) {
    if (!snapHit || !snapHit.kind) { hide(snapLabel); return; }
    snapLabel.textContent = snapHit.label;
    positionAtLabel(snapLabel, ev.clientX, ev.clientY);
    show(snapLabel);
  }

  function updateDimensionLabel(ev) {
    const spec = liveSpec();
    const dims = spec ? dimensionText(spec) : '';
    if (!dims) { hide(dimLabel); return; }
    dimLabel.textContent = `${dims}  ·  u ${nf(preview[0])}  v ${nf(preview[1])}`;
    positionAtLabel(dimLabel, ev.clientX, ev.clientY);
    show(dimLabel);
  }

  /** Live dimensions for the in-progress entity. */
  function dimensionText(e) {
    if (e.type === 'line') {
      const dx = e.end[0] - e.start[0], dy = e.end[1] - e.start[1];
      return `L ${nf(Math.hypot(dx, dy))}  @ ${nf(Math.atan2(dy, dx) * 180 / Math.PI)}°`;
    }
    if (e.type === 'rect') return `W ${nf(Math.abs(e.c2[0] - e.c1[0]))}  ×  H ${nf(Math.abs(e.c2[1] - e.c1[1]))}`;
    if (e.type === 'circle') return `R ${nf(e.radius)}`;
    if (e.type === 'arc') {
      let sweep = e.endAngle - e.startAngle;
      while (sweep <= 0) sweep += Math.PI * 2;
      return `R ${nf(e.radius)}  ∠ ${nf(sweep * 180 / Math.PI)}°`;
    }
    if (e.type === 'spline') {
      const n = e.points.length;
      return `${n} pts${e.closed ? '  closed' : ''}`;
    }
    return '';
  }

  function showRotateHint() {
    dimLabel.textContent = 'Rotate the view to face the ' + plane + ' plane';
    dimLabel.style.display = 'block';
    snapLabel.style.display = 'none';
  }
  function hideRotateHint() {
    // The hint shares the dimension label; clear it so a stale hint never sits
    // on screen once the ray reaches the plane again.
    if (dimLabel.textContent.startsWith('Rotate the view')) dimLabel.style.display = 'none';
  }

  // ---------- banner + plane panel ----------

  function updateBanner() {
    if (!active) { banner.style.display = 'none'; return; }
    banner.style.display = 'block';
    banner.innerHTML = `<span style="color:var(--teal);font-weight:700">Sketch</span>`
      + ` <span style="color:var(--ink-dim)">${esc(plane)}</span>`
      + ` <span style="color:var(--violet)">${esc((tool || '').toUpperCase())}</span>`
      + ` <span style="color:var(--ink-faint)">· ${entities.length} entit${entities.length === 1 ? 'y' : 'ies'}`
      + `${pending ? ' · in progress' : (chainFrom ? ' · chaining' : '')}</span>`
      + `<div class="sk-hints">Enter finish · Esc cancel · Shift ortho · RMB end chain · grid ${esc(nf(gridStep))}</div>`;
    const ps = planePanel.root.querySelectorAll('.sk-plane-btn');
    ps.forEach((b) => b.classList.toggle('active', b.dataset.plane === plane));
  }

  /** Floating XY/XZ/YZ + Finish/Cancel control, bottom-centre of the viewport. */
  function buildPlanePanel() {
    const root = el('div', 'sk-panel', '');
    const lbl = el('span', 'sk-panel-label', 'Plane');
    root.appendChild(lbl);
    for (const p of PLANE_NAMES) {
      const b = el('button', 'vp-btn sk-plane-btn', p);
      b.dataset.plane = p;
      b.title = `Draw on the ${p} plane`;
      b.addEventListener('click', () => setPlane(p));
      root.appendChild(b);
    }
    root.appendChild(el('div', 'vp-divider', ''));
    const ok = el('button', 'vp-btn sk-finish', '✓ Finish');
    ok.title = 'Compile the sketch into the document (Enter)';
    ok.addEventListener('click', () => { finish(); });
    const no = el('button', 'vp-btn sk-cancel', '✕ Cancel');
    no.title = 'Discard this sketch (Esc)';
    no.addEventListener('click', () => { cancel(); });
    root.appendChild(ok);
    root.appendChild(no);
    return { root };
  }

  // ---------- overlay transform + per-frame tick ----------

  function applyPlaneTransform() {
    const frame = PLANES[plane];
    const o = planeOriginVec();
    // Columns are (uAxis, vAxis, normal, origin): plane-local -> world. The
    // grid geometry and every entity are authored in plane-local (u, v, 0), so
    // this single matrix carries them all into place.
    group.matrix.set(
      frame.u[0], frame.v[0], frame.normal[0], o[0],
      frame.u[1], frame.v[1], frame.normal[1], o[1],
      frame.u[2], frame.v[2], frame.normal[2], o[2],
      0, 0, 0, 1
    );
    group.matrixAutoUpdate = false;
    group.matrixWorldNeedsUpdate = true;
  }

  /** Keep the snap marker a constant number of pixels across. */
  function sizeMarker() {
    const g = snapMarker.geometry;
    if (!g.getAttribute('position')) {
      // A diamond (two orthogonal segments) plus a point for the exact vertex.
      g.setAttribute('position', new THREE.Float32BufferAttribute(
        [-1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, -1, 0, 0, -1, 0, -1, 0, 0], 3));
      snapDot.geometry.setAttribute('position', new THREE.Float32BufferAttribute([0, 0, 0], 3));
    }
    const p = preview;
    if (!p) return;
    const cam = getCamera();
    let s = Math.max(0.05, gridStep * 0.15);
    if (cam) {
      const eye = cam.getWorldPosition ? cam.getWorldPosition(tmpV) : null;
      const o = planeOriginVec();
      const d = eye ? eye.distanceTo(hitVec.clone().set(o[0], o[1], o[2])) : 0;
      if (d > 0) s = Math.max(0.02, d * 0.012);
    }
    // Plane-local placement: the overlay group matrix carries it to world.
    snapMarker.position.set(p[0], p[1], 0);
    snapDot.position.set(p[0], p[1], 0);
    snapMarker.scale.set(s, s, 1);
    snapMarker.visible = true;
    snapDot.visible = true;
  }

  /** Per-frame housekeeping, driven from the app's animation loop. */
  function tick() {
    if (!active) return;
    const c = getControls();
    if (c && c.enabled) c.enabled = false;   // the app may swap controls mid-sketch
    sizeMarker();
  }

  // ---------- lifecycle ----------

  function attach() {
    canvas.addEventListener('pointerdown', onPointerDown, true);
    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerleave', onPointerLeave);
    canvas.addEventListener('contextmenu', onContextMenu);
    canvas.addEventListener('wheel', onWheel, { passive: false });
    document.addEventListener('keydown', onKeyDown, true);
    document.addEventListener('keyup', onKeyUp, true);
  }

  function detach() {
    canvas.removeEventListener('pointerdown', onPointerDown, true);
    canvas.removeEventListener('pointermove', onPointerMove);
    canvas.removeEventListener('pointerleave', onPointerLeave);
    canvas.removeEventListener('contextmenu', onContextMenu);
    canvas.removeEventListener('wheel', onWheel);
    document.removeEventListener('keydown', onKeyDown, true);
    document.removeEventListener('keyup', onKeyUp, true);
  }

  // Double-click detection is manual: a single click must never wait for a
  // possible second one, and contextmenu/pointerup ordering is unreliable.
  let dblTime = 0, dblX = 0, dblY = 0;

  function attachControls() {
    const c = getControls();
    if (c) {
      controlsState.set(c, c.enabled);
      c.enabled = false;
    }
  }

  function restoreControls() {
    for (const [c, enabled] of controlsState) {
      try { c.enabled = enabled; } catch (e) { /* disposed controls */ }
    }
    controlsState.clear();
  }

  /**
   * Enter sketch mode with a tool.
   * @param {'line'|'rect'|'circle'|'arc'|'spline'} t
   * @param {'XY'|'XZ'|'YZ'} [p]
   * @returns {boolean} true when sketch mode is now active
   */
  function begin(t, p) {
    if (!['line', 'rect', 'circle', 'arc', 'spline'].includes(t)) {
      notify(`Unknown sketch tool "${t}"`, 'warn', 1800);
      return false;
    }
    if (active) {
      // Switching tools mid-sketch keeps the plane and the drawn entities.
      if (pending || chainFrom) resetTool();
      if (p && normPlane(p) !== plane) setPlane(p);
      tool = t;
      updateBanner();
      window.dispatchEvent(new CustomEvent('ux:sketch-tool', { detail: { tool, active: true } }));
      return true;
    }

    plane = normPlane(p || plane);
    tool = t;
    active = true;
    entities = [];
    pending = null;
    chainFrom = null;
    preview = null;
    snapHit = null;
    offset = 0;

    updatePlaneObject();
    setGridGeometry();
    applyPlaneTransform();
    rebuildPlaced();
    rebuildPreview();
    group.visible = true;
    scene.add(group);

    attach();
    attachControls();
    planePanel.root.style.display = 'flex';
    updateBanner();
    window.__sketchActive = true;
    window.dispatchEvent(new CustomEvent('ux:sketch-tool', { detail: { tool, active: true } }));
    notify(`Sketch on ${plane} — ${t}. Enter finish · Esc cancel`, 'info', 2600);
    return true;
  }

  /** Switch the workplane while sketching (entities keep their u/v values). */
  function setPlane(p) {
    const np = normPlane(p);
    if (np === plane) return plane;
    plane = np;
    if (active) {
      updatePlaneObject();
      setGridGeometry();
      applyPlaneTransform();
      rebuildPlaced();
      rebuildPreview();
      updateBanner();
    }
    window.dispatchEvent(new CustomEvent('ux:sketch-plane', { detail: { plane } }));
    return plane;
  }

  function teardown() {
    detach();
    restoreControls();
    active = false;
    pending = null;
    chainFrom = null;
    preview = null;
    snapHit = null;
    tool = null;
    group.visible = false;
    if (group.parent) group.parent.remove(group);
    while (placedSurface.children.length) {
      const c = placedSurface.children.pop();
      if (c.geometry) c.geometry.dispose();
    }
    while (previewSurface.children.length) {
      const c = previewSurface.children.pop();
      if (c.geometry && c !== snapMarker.geometry && c !== snapDot.geometry) c.geometry.dispose();
    }
    banner.style.display = 'none';
    planePanel.root.style.display = 'none';
    dimLabel.style.display = 'none';
    snapLabel.style.display = 'none';
    window.__sketchActive = false;
    window.dispatchEvent(new CustomEvent('ux:sketch-tool', { detail: { tool: null, active: false } }));
    updateBanner();
  }

  /** Abort: discard everything drawn and leave sketch mode. */
  function cancel() {
    if (!active) return;
    teardown();
    entities = [];
    notify('Sketch cancelled', 'info', 1500);
  }

  /**
   * Compile the drawn entities into a `Sketch` component, write it into the
   * document (preserving every other byte), and re-evaluate.
   * @returns {Promise<boolean>} true when the document was written
   */
  async function finish() {
    if (!active || saving) return false;
    const list = entities.slice();
    const ronLines = list.map(entityRon).filter(Boolean);
    if (!ronLines.length) {
      notify('Nothing to sketch — draw at least one entity first', 'warn', 2000);
      return false;
    }

    saving = true;
    try {
      const doc = getEditorRon() || '';
      if (!String(doc).trim()) {
        notify('Cannot place a sketch: the document is empty', 'warn', 2200);
        return false;
      }
      const name = nextSketchName(doc);
      const res = upsertSketchComponent(doc, { name, plane, offset, entities: list });
      if (!res.text || res.text === doc) {
        notify('Could not write the sketch into this document', 'error', 2600);
        return false;
      }
      setEditorRon(res.text);
      teardown();
      entities = [];
      if (typeof ctx.evaluate === 'function') {
        try { await ctx.evaluate(); } catch (e) { /* issues surface in the app UI */ }
      }
      notify(`sketch "${name}" created — reference it from an Extrude`, 'info', 3200);
      return true;
    } catch (e) {
      notify('Sketch failed: ' + (e && e.message ? e.message : e), 'error', 3000);
      return false;
    } finally {
      saving = false;
    }
  }

  /** Remove the most recently placed entity. */
  function undoLast() {
    if (!entities.length) return false;
    entities.pop();
    rebuildPlaced();
    updateBanner();
    return true;
  }

  /** Dispose every Three.js resource this module owns. */
  function dispose() {
    if (active) teardown();
    for (const m of owned) {
      try { if (m && m.dispose) m.dispose(); } catch (e) { /* already gone */ }
    }
    owned.length = 0;
    if (gridObj.geometry) gridObj.geometry.dispose();
    if (gridCoarseObj.geometry) gridCoarseObj.geometry.dispose();
    if (axesObj.geometry) axesObj.geometry.dispose();
    if (snapMarker.geometry) snapMarker.geometry.dispose();
    if (snapDot.geometry) snapDot.geometry.dispose();
    for (const node of [banner, dimLabel, snapLabel, planePanel.root]) {
      if (node && node.parentNode) node.parentNode.removeChild(node);
    }
  }

  return {
    isActive: () => active,
    activeTool: () => tool,
    begin,
    setPlane,
    getPlane: () => plane,
    cancel,
    finish,
    undoLast,
    getEntityCount: () => entities.length,
    setGridStep: (s) => { const v = Number(s); if (Number.isFinite(v) && v > 0) { gridStep = v; if (active) { setGridGeometry(); updateBanner(); } } },
    getGridStep: () => gridStep,
    getOffset: () => offset,
    setOffset: (o) => { const v = Number(o); if (Number.isFinite(v)) { offset = v; if (active) { updatePlaneObject(); applyPlaneTransform(); rebuildPlaced(); rebuildPreview(); } } },
    /** Called from the app's animation loop. */
    tick,
    dispose,
    // Exposed for white-box debugging from the console.
    __debug: {
      entities: () => entities.slice(),
      setEntities: (list) => { entities = (list || []).slice(); rebuildPlaced(); updateBanner(); },
      ron: () => sketchComponentRon(nextSketchName(getEditorRon() || ''), plane, offset, entities, 0),
      block: (name) => sketchComponentRon(name, plane, offset, entities, 0),
      data: (name) => ({ name, plane, offset, entities: entities.slice() }),
      upsert: (doc, data) => upsertSketchComponent(doc, data),
      nextName: (doc) => nextSketchName(doc),
      planeToWorld,
      worldToPlane,
    },
  };
}
