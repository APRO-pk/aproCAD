// ============================================================
// APRO CAD — persistent sketch display
//
// A `Sketch` component is construction geometry: the backend deliberately
// produces no mesh for it, so without this module a finished sketch would be
// invisible in the 3D viewport. This draws every sketch in the document as an
// overlay so it stays visible after the drawing tool closes, the way a CAD
// sketch is expected to behave.
//
// What it draws is the COMPILED profile (stitched + closed), not the raw
// entity soup, so the picture matches what an Extrude referencing the sketch
// will actually produce. Open chains are drawn in a warning colour instead,
// which makes "why won't this extrude?" visible at a glance.
//
// The document text is the source of truth; parsing lives in sketcher.js.
// ============================================================

'use strict';

import { parseSketches, compileSketchEntities, planeToWorld } from './sketcher.js';

const ACCENT = 0x00e5b0;   // matches --teal / the sketcher overlay
const OPEN_COLOR = 0xffb454; // --amber: not closed, cannot be extruded
const SELECTED = 0xffffff;

/**
 * Build the sketch renderer.
 *
 * @param {object} THREE the three module namespace
 * @param {object} scene THREE.Scene to host the overlay group
 * @returns {{sync: Function, setSelected: Function, clear: Function, dispose: Function, group: object}}
 */
export function createSketchDisplay(THREE, scene) {
  const group = new THREE.Group();
  group.name = '__sketchDisplay';
  group.renderOrder = 900;
  scene.add(group);

  const matLoop = new THREE.LineBasicMaterial({
    color: ACCENT, transparent: true, opacity: 0.95, depthTest: false,
  });
  const matOpen = new THREE.LineBasicMaterial({
    color: OPEN_COLOR, transparent: true, opacity: 0.9, depthTest: false,
  });
  const matSel = new THREE.LineBasicMaterial({
    color: SELECTED, transparent: true, opacity: 1, depthTest: false,
  });

  let selectedName = null;

  /** Drop every line currently in the group. */
  function clear() {
    while (group.children.length) {
      const c = group.children.pop();
      if (c.geometry) c.geometry.dispose();
    }
  }

  /**
   * Build one polyline child.
   * @param {number[]} flat vertex data, already in world space
   * @param {boolean} isOpen open chain (unclosed → cannot be extruded)
   * @param {boolean} isSelected draw in the selection colour
   * @param {string} owner the sketch component name this line belongs to
   */
  function addLine(flat, isOpen, isSelected, owner) {
    if (flat.length < 6) return;
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(flat, 3));
    const line = new THREE.Line(g, isSelected ? matSel : (isOpen ? matOpen : matLoop));
    line.renderOrder = isSelected ? 950 : 900;
    // Not pickable: sketches must never steal a click from a solid part.
    line.userData.isSketch = true;
    line.userData.isOpen = isOpen;
    line.userData.sketchName = owner;
    group.add(line);
  }

  /** World-space flat vertex array for a plane-local point list. */
  function flatten(plane, offset, pts, closeIt) {
    const flat = [];
    for (const uv of pts) {
      const w = planeToWorld(plane, uv, offset);
      flat.push(w[0], w[1], w[2]);
    }
    if (closeIt && pts.length) {
      const w0 = planeToWorld(plane, pts[0], offset);
      flat.push(w0[0], w0[1], w0[2]);
    }
    return flat;
  }

  /**
   * Rebuild the display from a document.
   *
   * @param {string} ron the current RON document text
   * @returns {number} how many sketches were drawn
   */
  function sync(ron) {
    clear();
    let sketches;
    try {
      sketches = parseSketches(ron);
    } catch (e) {
      // A half-typed document must never break the viewport.
      return 0;
    }
    let drawn = 0;
    for (const s of sketches) {
      if (s.visible === false) continue;
      const compiled = compileSketchEntities(s.entities);
      const isSelected = !!s.name && s.name === selectedName;

      // Closed loops first (the real profile), then any open leftovers in amber.
      for (const loop of compiled.loops) {
        addLine(flatten(s.plane, s.offset, loop, true), false, isSelected, s.name);
      }
      for (const chain of compiled.open) {
        addLine(flatten(s.plane, s.offset, chain, false), true, isSelected, s.name);
      }
      drawn++;
    }
    return drawn;
  }

  /**
   * Highlight one sketch by component name (null clears the highlight).
   * Deliberately unconditional: a rebuild replaces every line object, so an
   * unchanged name still has to be reapplied or the highlight is lost.
   */
  function setSelected(name) {
    selectedName = name || null;
    for (const line of group.children) {
      const isSel = !!line.userData.sketchName && line.userData.sketchName === selectedName;
      line.material = isSel ? matSel : (line.userData.isOpen ? matOpen : matLoop);
      line.renderOrder = isSel ? 950 : 900;
    }
  }

  function dispose() {
    clear();
    matLoop.dispose();
    matOpen.dispose();
    matSel.dispose();
    if (group.parent) group.parent.remove(group);
  }

  return { sync, setSelected, clear, dispose, group };
}
