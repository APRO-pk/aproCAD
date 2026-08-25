// ============================================================
// APRO CAD — UX layer (non-invasive, guards on element existence)
// Loaded before main.js. All behaviors are additive.
// ============================================================

(function () {
  'use strict';

  // ---------- utils ----------
  const $ = (id) => document.getElementById(id);
  const on = (el, ev, fn) => { if (el) el.addEventListener(ev, fn); };
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  function notify(msg, kind = 'info', ms = 3500) {
    const stack = $('ux-notify-stack');
    if (!stack) return;
    const el = document.createElement('div');
    el.className = 'ux-notify' + (kind === 'warn' ? ' warn' : kind === 'err' ? ' err' : '');
    el.innerHTML = `<span class="n-dot"></span><span>${msg}</span>`;
    stack.appendChild(el);
    setTimeout(() => {
      el.classList.add('leaving');
      setTimeout(() => el.remove(), 220);
    }, ms);
  }

  // ---------- Ribbon ----------
  const TOOL_ACTIONS = {
    'undo': () => $('undo-btn')?.click(),
    'redo': () => $('redo-btn')?.click(),
    'format': () => $('format-btn')?.click(),
    'export-stl': () => $('stl-btn')?.click(),
    'export-step': () => $('step-btn')?.click(),
    'new-doc': () => { window.__apro?.newDocument?.(); },
    'open-doc': () => { window.__apro?.openDocument?.(); },
    'save-doc': () => { window.__apro?.saveDocument?.(); },
    'export-image': () => { window.__apro?.screenshotViewport?.(); },
    'ai-chat': () => {
      const chatTab = document.querySelector('#editor-tabs .tab-btn[data-tab=chat]');
      chatTab?.click();
    },
    'library': () => { window.__apro?.openLibrary?.(); },
    'tool-cursor': () => { window.__apro?.setGizmoMode?.('cursor'); },
    'tool-move': () => { window.__apro?.setGizmoMode?.('translate'); },
    'tool-scale': () => { window.__apro?.setGizmoMode?.('scale'); },
    'tool-rotate': () => { window.__apro?.setGizmoMode?.('rotate'); },
    'view-front': () => snapCamDir('front'),
    'view-back': () => snapCamDir('back'),
    'view-top': () => snapCamDir('top'),
    'view-bottom': () => snapCamDir('bottom'),
    'view-left': () => snapCamDir('left'),
    'view-right': () => snapCamDir('right'),
    'view-iso': () => {
      // Cycles: perspective ⇄ true isometric (orthographic) with a smooth
      // projection morph. Falls back to an angled snap outside Tauri.
      const r = window.__apro?.toggleIsoView?.();
      if (!r || typeof r.then !== 'function') { snapCamDir('iso'); return; }
      r.then(on => {
        syncToggleButton('view-iso', on);
        notify(on ? 'Isometric view (orthographic)' : 'Perspective view', 'info', 1200);
      }).catch(() => {});
    },
    'view-fit': () => {
      // Real zoom-extents against the current model (was a hardcoded camera).
      window.__apro?.fitView?.();
      notify('Fit to model', 'info', 1200);
    },
    'view-wireframe': () => { window.__apro?.setDisplayMode?.('wireframe'); syncViewModeButtons(); },
    'view-shaded': () => { window.__apro?.setDisplayMode?.('shaded'); syncViewModeButtons(); },
    'view-xray': () => { window.__apro?.setDisplayMode?.('xray'); syncViewModeButtons(); },
    'view-grid': () => {
      const v = window.__apro?.toggleGrid?.();
      if (v != null) syncToggleButton('view-grid', v);
    },
    'view-axes': () => {
      const v = window.__apro?.toggleAxes?.();
      if (v != null) syncToggleButton('view-axes', v);
    },
  };

  function snapCamDir(dir) {
    const apro = window.__apro;
    if (!apro || !apro.camera || !apro.controls) return;
    if (dir === 'iso') {
      const t = apro.controls.target;
      const dist = apro.camera.position.distanceTo(t);
      apro.camera.position.set(t.x + dist * 0.6, t.y + dist * 0.5, t.z + dist * 0.8);
      apro.controls.update();
      return;
    }
    snapCamera(dir);
  }

  // Ribbon active-state for display modes + toggles
  function setBtnActive(tool, on) {
    const btn = document.querySelector(`.ribbon-btn[data-tool="${tool}"]`);
    if (btn) btn.classList.toggle('active', !!on);
  }
  function syncViewModeButtons() {
    const mode = window.__apro?.getDisplayMode?.() || 'shaded';
    ['shaded', 'wireframe', 'xray'].forEach(m => setBtnActive('view-' + m, m === mode));
  }
  function syncToggleButton(tool, on) {
    setBtnActive(tool, on);
  }

  function initRibbon() {
    const tabs = document.querySelectorAll('.ribbon-tab');
    if (!tabs.length) return;
    tabs.forEach((tab) => {
      on(tab, 'click', () => {
        tabs.forEach((t) => t.classList.remove('active'));
        tab.classList.add('active');
        document.querySelectorAll('.ribbon-panel').forEach((p) => {
          p.classList.toggle('active', p.dataset.ribbonPanel === tab.dataset.ribbon);
        });
      });
    });

    // Ribbon tool buttons → real actions where available, else toast placeholder
    document.querySelectorAll('.ribbon-btn[data-tool]').forEach((btn) => {
      on(btn, 'click', () => {
        const tool = btn.dataset.tool;
        btn.classList.add('primary-glow');
        setTimeout(() => btn.classList.remove('primary-glow'), 900);
        const action = TOOL_ACTIONS[tool];
        if (action) {
          action();
        } else {
          notify(`Tool <b>${tool}</b> — coming soon`, 'warn', 1800);
        }
      });

      // Kbd tooltips: custom styled tooltip with title + shortcut hint
      if (btn.title) {
        btn.dataset.hint = btn.title;
        on(btn, 'pointerenter', () => {
          const tip = document.createElement('div');
          tip.className = 'ux-tip';
          const kbd = btn.dataset.kbd ? `<span class="kbd">${btn.dataset.kbd}</span>` : '';
          tip.innerHTML = `<span>${btn.title}</span>${kbd}`;
          document.body.appendChild(tip);
          const r = btn.getBoundingClientRect();
          tip.style.left = Math.min(r.left, window.innerWidth - tip.offsetWidth - 8) + 'px';
          tip.style.top = (r.bottom + 6) + 'px';
          btn.__tip = tip;
        });
        on(btn, 'pointerleave', () => { btn.__tip?.remove(); btn.__tip = null; });
      }
    });
  }

  // ---------- Left dock tabs ----------
  function initDockTabs() {
    const tabs = document.querySelectorAll('#editor-tabs .tab-btn');
    if (!tabs.length) return;
    tabs.forEach((tab) => {
      on(tab, 'click', () => {
        tabs.forEach((t) => t.classList.remove('active'));
        tab.classList.add('active');
        document.querySelectorAll('.tab-content').forEach((p) => {
          p.classList.toggle('active', p.id === tab.dataset.tab + '-tab');
        });
      });
    });
  }

  // ---------- ViewCube ----------
  function snapCamera(dir) {
    const apro = window.__apro;
    if (!apro || !apro.camera || !apro.controls) return;
    const dist = apro.camera.position.distanceTo(apro.controls.target);
    const t = apro.controls.target;
    const pos = {
      front: [0, 0, 1], back: [0, 0, -1], right: [1, 0, 0],
      left: [-1, 0, 0], top: [0, 1, 0], bottom: [0, -1, 0],
    }[dir];
    if (!pos) return;
    apro.camera.position.set(t.x + pos[0] * dist, t.y + pos[1] * dist, t.z + pos[2] * dist);
    apro.controls.update();
  }

  function initViewCube() {
    const cube = $('ux-viewcube');
    const caption = $('ux-viewcube-caption');
    if (!cube) return;

    // Faces snap the camera (delegates to main.js via custom event, if present)
    cube.querySelectorAll('.vc-face').forEach((face) => {
      on(face, 'click', () => {
        const cls = Array.from(face.classList).find((c) => /^vc-(front|back|left|right|top|bottom)$/.test(c));
        if (!cls) return;
        const dir = cls.slice(3);
        const angle = {
          front: { x: 0, y: 0 }, back: { x: 0, y: 180 },
          right: { x: 0, y: 90 }, left: { x: 0, y: -90 },
          top: { x: 90, y: 0 }, bottom: { x: -90, y: 0 },
        }[dir];
        if (angle) {
          cube.style.transform = `rotateX(${angle.x}deg) rotateY(${angle.y}deg)`;
          snapCamera(dir);
          window.dispatchEvent(new CustomEvent('ux:view-face', { detail: dir }));
        }
      });
    });

    // Drag to orbit the cube (visual only)
    let dragging = false, sx = 0, sy = 0, rx = -24, ry = 32;
    on(cube, 'pointerdown', (e) => {
      dragging = true; sx = e.clientX; sy = e.clientY;
      cube.setPointerCapture(e.pointerId);
    });
    on(cube, 'pointermove', (e) => {
      if (!dragging) return;
      ry += (e.clientX - sx) * 0.6;
      rx += (e.clientY - sy) * 0.6;
      rx = Math.max(-90, Math.min(90, rx));
      sx = e.clientX; sy = e.clientY;
      cube.style.transform = `rotateX(${rx}deg) rotateY(${ry}deg)`;
      if (caption) caption.textContent = `${Math.round(rx)}° / ${Math.round(ry)}°`;
    });
    on(cube, 'pointerup', () => { dragging = false; });
  }

  // ---------- Feature tree ----------
  // Currently selected tree entry (component name or '__parameters__');
  // re-applied after every rebuild so selection survives editor polls.
  let currentSelection = null;

  function countComps(ron) {
    const re = /Component\(\s*name:\s*"/g;
    let n = 0;
    while (re.exec(ron)) n++;
    return n;
  }

  function clearTreeSelection() {
    currentSelection = null;
    document.querySelectorAll('#ux-tree-body .tree-node.selected').forEach((n) => n.classList.remove('selected'));
    const chip = $('ux-sel-chip');
    if (chip) chip.style.display = 'none';
  }

  function buildTree(ron) {
    const body = $('ux-tree-body');
    if (!body) return;
    body.innerHTML = '';

    // Quick heuristic parse: component names/kinds + parameter names
    const comps = [];
    const re = /Component\(\s*name:\s*"([^"]+)"[^]*?kind:\s*(\w+)/g;
    let m;
    while ((m = re.exec(ron))) comps.push({ name: m[1], kind: m[2] });
    const paramNames = [];
    const pre = /Parameter\(\s*name:\s*"([^"]+)"/g;
    while ((m = pre.exec(ron))) paramNames.push(m[1]);

    if (!comps.length && !paramNames.length) {
      const empty = document.createElement('div');
      empty.className = 'tree-empty';
      empty.innerHTML = 'No components yet.<br><span>Edit the document or use AI to create parts.</span>';
      body.appendChild(empty);
      return;
    }

    const root = document.createElement('div');
    root.className = 'tree-node open';
    const rootMeta = `${comps.length} part${comps.length === 1 ? '' : 's'}${paramNames.length ? ` · ${paramNames.length} param${paramNames.length === 1 ? '' : 's'}` : ''}`;
    root.innerHTML = `<span class="chev">▶</span><span class="node-icon">🗀</span><span>Document</span><span class="node-kind">${rootMeta}</span>`;
    body.appendChild(root);

    const children = document.createElement('div');
    children.className = 'tree-children open';

    const wireNode = (node, name, index, inspectorName) => {
      node.dataset.treeName = name;
      if (currentSelection === name) node.classList.add('selected');
      node.addEventListener('click', () => {
        const wasSelected = node.classList.contains('selected');
        body.querySelectorAll('.tree-node').forEach((n) => n.classList.remove('selected'));
        if (wasSelected) {
          clearTreeSelection();
          window.dispatchEvent(new CustomEvent('ux:select-part', { detail: { name, index, deselected: true } }));
          return;
        }
        node.classList.add('selected');
        currentSelection = name;
        updateInspector({ name: inspectorName, units: 'SI (g · mm)', components: null });
        const chip = $('ux-sel-chip');
        if (chip && name !== '__parameters__') {
          const label = $('ux-sel-label');
          if (label) label.textContent = `${inspectorName}`;
          chip.style.display = 'flex';
        }
        window.dispatchEvent(new CustomEvent('ux:select-part', { detail: { name, index } }));
      });
    };

    // Parameters pseudo-node (vehicle-level equations)
    if (paramNames.length) {
      const pnode = document.createElement('div');
      pnode.className = 'tree-node';
      pnode.style.animationDelay = '0ms';
      pnode.innerHTML = `<span class="chev">▶</span><span class="node-icon">ƒ</span><span>Parameters</span><span class="node-kind">${paramNames.length}</span>`;
      wireNode(pnode, '__parameters__', -1, 'Parameters');
      children.appendChild(pnode);
    }

    comps.forEach((c, i) => {
      const icons = {
        NoseCone: '△', BodyTube: '▯', Transition: '◮', Tank: '●',
        Nozzle: '▽', FinSet: '▲', Solid: '▧',
      };
      const node = document.createElement('div');
      node.className = 'tree-node';
      node.style.animationDelay = `${(i + 1) * 40}ms`;
      node.innerHTML = `<span class="chev">▶</span><span class="node-icon">${icons[c.kind] || '▫'}</span><span>${c.name}</span><span class="node-kind">${c.kind}</span>`;
      wireNode(node, c.name, i, c.name);
      children.appendChild(node);
    });
    body.appendChild(children);

    // Wire refresh/collapse
    const refresh = $('ux-tree-refresh');
    if (refresh) on(refresh, 'click', () => buildTree(ron));
    const collapse = $('ux-tree-collapse');
    if (collapse) on(collapse, 'click', () => {
      children.classList.toggle('open');
      root.classList.toggle('open');
    });
  }

  // ---------- Inspector ----------
  function updateInspector(info) {
    const set = (id, v) => { const el = $(id); if (el && v != null) el.textContent = v; };
    if (!info) return;
    if (info.name) set('ux-insp-name', info.name);
    if (info.units) set('ux-insp-units', info.units);
    if (info.components != null) set('ux-insp-comps', String(info.components));
    if (info.mass != null) set('ux-insp-mass', typeof info.mass === 'number' ? info.mass.toFixed(2) + ' g' : info.mass);
    if (info.volume != null) set('ux-insp-volume', typeof info.volume === 'number' ? info.volume.toFixed(2) + ' cm³' : info.volume);
    if (info.com) set('ux-insp-com', Array.isArray(info.com) ? info.com.map((v) => v.toFixed(0)).join(', ') : info.com);
    if (info.tris != null) set('ux-insp-tris', String(info.tris));
    if (info.verts != null) set('ux-insp-verts', String(info.verts));
    if (info.tris != null) {
      const bar = $('ux-insp-meshbar');
      if (bar) bar.style.width = Math.min(100, (info.tris / 50000) * 100) + '%';
    }
  }

  // ---------- HUD ----------
  function updateHud(info) {
    const set = (id, v) => { const el = $(id); if (!el || v == null) return; el.textContent = v; };
    if (info.mass != null) set('ux-hud-mass', typeof info.mass === 'number' ? info.mass.toFixed(1) + ' g' : info.mass);
    if (info.volume != null) set('ux-hud-vol', typeof info.volume === 'number' ? info.volume.toFixed(1) : info.volume);
    if (info.com) set('ux-hud-com', Array.isArray(info.com) ? info.com.map((v) => v.toFixed(0)).join(', ') : info.com);
  }

  // ---------- File menu ----------
  function initFileMenu() {
    const trigger = $('ux-file-btn');
    const pop = $('ux-file-pop');
    if (!trigger || !pop) return;
    const toggle = (open) => pop.classList.toggle('open', open);
    on(trigger, 'click', (e) => {
      e.stopPropagation();
      toggle(!pop.classList.contains('open'));
    });
    on(document, 'click', () => toggle(false));
    on(document, 'keydown', (e) => { if (e.key === 'Escape') toggle(false); });

    pop.querySelectorAll('.menu-item').forEach((item) => {
      on(item, 'click', () => {
        const a = item.dataset.action;
        const editor = $('editor');
        const apro = window.__apro;
        if (a === 'export-stl') { $('stl-btn')?.click(); }
        else if (a === 'export-step') { $('step-btn')?.click(); }
        else if (a === 'reset-view' && apro && apro.controls) {
          apro.controls.target.set(0, 0, 150);
          apro.camera.position.set(400, 300, 500);
          apro.controls.update();
          notify('View reset', 'info', 1500);
        } else if (a === 'fullscreen') {
          const wrap = $('viewport-canvas-wrap');
          if (wrap && !document.fullscreenElement) wrap.requestFullscreen?.();
          else document.exitFullscreen?.();
        } else if (a === 'new') {
          window.__apro?.newDocument?.();
        } else {
          notify(`<b>${a}</b> — coming soon`, 'warn', 1800);
        }
        toggle(false);
      });
    });
  }

  // ---------- Shortcuts modal ----------
  function initShortcuts() {
    const openBtn = $('ux-shortcuts-btn');
    const modal = $('ux-shortcuts-modal');
    const close = $('ux-shortcuts-close');
    const backdrop = $('ux-shortcuts-backdrop');
    if (!modal) return;
    const show = () => { modal.style.display = 'flex'; };
    const hide = () => { modal.style.display = 'none'; };
    if (openBtn) on(openBtn, 'click', show);
    if (close) on(close, 'click', hide);
    if (backdrop) on(backdrop, 'click', hide);
    on(document, 'keydown', (e) => {
      if (e.key === 'Escape' && modal.style.display !== 'none') hide();
    });
  }

  // ---------- Status mode flash ----------
  function flashMode(text) {
    const el = $('ux-status-mode');
    if (!el) return;
    el.textContent = text;
    el.classList.add('sketch');
    setTimeout(() => { el.classList.remove('sketch'); el.textContent = 'Modeling'; }, 1600);
  }

  // ---------- Selection chip ----------
  function initSelChip() {
    const chip = $('ux-sel-chip');
    const label = $('ux-sel-label');
    const close = $('ux-sel-close');
    if (!chip) return;
    window.addEventListener('ux:select-part', (e) => {
      if (e.detail.deselected) { chip.style.display = 'none'; return; }
      if (label) label.textContent = e.detail.name === '__parameters__'
        ? 'Parameters'
        : `${e.detail.name} (part ${e.detail.index + 1})`;
      chip.style.display = 'flex';
    });
    on(close, 'click', () => {
      chip.style.display = 'none';
      window.dispatchEvent(new CustomEvent('ux:deselect-part'));
      notify('Selection cleared', 'info', 1200);
    });
    // main.js deselects (Esc / popup close / component removed): sync the tree
    window.addEventListener('ux:deselect-part', clearTreeSelection);
    // main.js selected a part directly (3D click): highlight its tree node
    window.addEventListener('ux:sync-selection', (e) => {
      const name = e.detail && e.detail.name;
      if (!name) return;
      currentSelection = name;
      document.querySelectorAll('#ux-tree-body .tree-node').forEach((n) => {
        n.classList.toggle('selected', n.dataset.treeName === name);
      });
      const chip = $('ux-sel-chip');
      if (chip) {
        const label = $('ux-sel-label');
        if (label) label.textContent = name === '__parameters__' ? 'Parameters' : name;
        chip.style.display = 'flex';
      }
    });
  }

  // ---------- Sidebar tools ----------
  function initSidebar() {
    document.querySelectorAll('#ux-vp-sidebar .vp-side-btn').forEach((btn) => {
      on(btn, 'click', () => {
        if (btn.dataset.tool === 'fullscreen') {
          const wrap = $('viewport-canvas-wrap');
          if (wrap && !document.fullscreenElement) wrap.requestFullscreen?.();
          else document.exitFullscreen?.();
          return;
        }
        // Known tools route through the shared action table (gizmos etc.);
        // unknown ones keep the placeholder toast.
        const action = TOOL_ACTIONS[btn.dataset.tool];
        if (action) {
          btn.classList.add('active');
          action();
          return;
        }
        notify(`Tool <b>${btn.dataset.tool}</b> — coming soon`, 'warn', 1800);
        flashMode(btn.dataset.tool);
      });
    });
  }

  // ---------- Live refresh hooks (called by main.js or polled) ----------
  window.__ux = {
    notify,
    buildTree,
    updateInspector,
    updateHud,
    flashMode,
  };

  // Poll the editor for tree updates (non-invasive; main.js owns the editor).
  const editor = $('editor');
  if (editor) {
    let lastRon = '';
    setInterval(() => {
      const ron = editor.value;
      if (ron !== lastRon) {
        lastRon = ron;
        buildTree(ron);
        updateInspector({ components: countComps(ron) });
      }
    }, 1200);
  }

  // Mirror main.js status readouts into the HUD / inspector (guarded).
  setInterval(() => {
    const massEl = $('mass-info');
    const polyEl = $('poly-count');
    const cacheEl = $('cache-stats');
    if (massEl) {
      updateHud({ mass: massEl.textContent });
      const name = $('ux-insp-name');
      if (name && name.textContent === '—') {
        const m = /([\d.]+)\s*kg/i.exec(massEl.textContent);
        if (m) updateInspector({ mass: parseFloat(m[1]) * 1000 });
      }
    }
    if (polyEl) updateHud({ tris: polyEl.textContent });
    if (cacheEl) {
      const c = $('ux-insp-cache');
      if (c) c.textContent = cacheEl.textContent;
    }
  }, 1200);

  // Render-stats: FPS + WebGL renderer/device (guarded, only when inspector tab exists)
  if ($('ux-insp-fps')) {
    let frames = 0, lastT = performance.now();
    const statLoop = () => {
      frames++;
      const now = performance.now();
      if (now - lastT >= 1000) {
        const fps = Math.round((frames * 1000) / (now - lastT));
        frames = 0; lastT = now;
        const f = $('ux-insp-fps');
        if (f) f.textContent = fps;
      }
      requestAnimationFrame(statLoop);
    };
    requestAnimationFrame(statLoop);

    const viewer = $('viewer');
    if (viewer) {
      try {
        const gl = viewer.getContext('webgl2') || viewer.getContext('webgl');
        if (gl) {
          const dbg = gl.getExtension('WEBGL_debug_renderer_info');
          const r = $('ux-insp-renderer');
          const d = $('ux-insp-device');
          if (r) r.textContent = gl.getParameter(gl.RENDERER) || '—';
          if (d && dbg) d.textContent = gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) || '—';
        }
      } catch (e) { /* webgl context already claimed by main.js */ }
    }
  }

  // ---------- Init ----------
  function init() {
    initRibbon();
    initDockTabs();
    initViewCube();
    initFileMenu();
    initShortcuts();
    initSelChip();
    initSidebar();
    syncViewModeButtons(); // shaded is the default mode
    setBtnActive('view-grid', true);
    setBtnActive('view-axes', true);
    if (editor) buildTree(editor.value);
    setTimeout(() => {
      notify('Welcome to APRO CAD <b>Obsidian Forge</b> UI', 'info', 2600);
    }, 800);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();

if (window.__boot) window.__boot.push('ux: loaded');
