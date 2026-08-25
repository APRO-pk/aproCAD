# APRO CAD — Deep UI Redesign Plan

## Vision

Transform APRO CAD from a utilitarian dev-tool into a premium, SolidWorks-class CAD environment: glassmorphic dark UI, a command ribbon, a feature-manager tree, a live view cube, rich animations, and an extensible tool surface where future features (sketching, mates, simulation, CAM) plug in.

Design language: **"Obsidian Forge"** — deep graphite panels, neon teal + violet accents, glass blur, soft glows, micro-animated interactions, cinematic panel transitions.

---

## Layout

```
┌──────────────────────────────────────────────────────────────┐
│ TOP BAR — logo · file menu · presets · undo/redo · settings  │
├──────────────────────────────────────────────────────────────┤
│ RIBBON — tabs: Home | Sketch | Features | Surfaces |         │
│          Evaluate | AI | View — tool buttons w/ animations   │
├───────────────┬──────────────────────────────────────────────┤
│ LEFT DOCK     │  VIEWPORT                                    │
│ ┌───────────┐ │  ┌────────────────────────────────────────┐  │
│ │Feature    │ │  │ ViewCube (top-right, 3D, interactive)  │  │
│ │Manager    │ │  │ Mini toolbars (left/top floating)      │  │
│ │(tree)     │ │  │ Selection chip (bottom-center)         │  │
│ ├───────────┤ │  │ Measure overlay / section plane        │  │
│ │Tabs:      │ │  │ Analysis HUD (mass, CoM, volume)       │  │
│ │Editor     │ │  └────────────────────────────────────────┘  │
│ │Chat       │ │  ┌────────────────────────────────────────┐  │
│ │Inspector  │ │  │ Properties Drawer (collapsible)        │  │
│ └───────────┘ │  └────────────────────────────────────────┘  │
├───────────────┴──────────────────────────────────────────────┤
│ STATUS BAR — mode · units · selection · mass · polycount ·   │
│               cache · diagnostics · notifications            │
└──────────────────────────────────────────────────────────────┘
```

---

## Design Tokens

| Token | Value | Use |
|---|---|---|
| `--bg-void` | `#05050a` | deepest backdrop / viewport |
| `--bg-0` | `#0a0a14` | app background |
| `--bg-1` | `#10101e` | panels |
| `--bg-2` | `#161628` | cards, buttons |
| `--bg-3` | `#1d1d33` | hover states |
| `--edge` | `rgba(255,255,255,0.06)` | hairline borders |
| `--teal` | `#00e5b0` | primary accent |
| `--violet` | `#8b7bff` | secondary accent |
| `--amber` | `#ffb454` | warning |
| `--rose` | `#ff5d73` | danger |
| `--ink` | `#e8e8f2` | primary text |
| `--ink-dim` | `#8a8a9e` | secondary text |
| `--glass` | `rgba(16,16,30,0.72)` | floating panels + blur |

Typography: UI = system stack; Mono = JetBrains Mono for code/data.

---

## Animation Language

- **Panel transitions**: 240ms cubic-bezier(0.22,1,0.36,1) — slide + fade + scale(0.98→1)
- **Hover micro-motion**: 120ms ease — lift 1px, glow border
- **Active press**: scale(0.96)
- **Status pulses**: spinner rings, success flash ring on evaluate
- **ViewCube**: faces flip 300ms with 3D rotateY/rotateX
- **Ribbon tabs**: underline sweep animation
- **Tree nodes**: indent slide-in, chevron rotate
- **Toast**: slide-up + glow ring
- **Background ambience**: subtle radial glow breathing in viewport corners (CSS only)

---

## Phases

### Phase 1 — Foundation & Skeleton
- Rewrite `index.html` layout: topbar, ribbon, left dock (3 tabs), viewport, props drawer, status bar
- Rewrite CSS design system: tokens, glass, shadows, keyframe library
- Preserve EVERY existing element ID used by `main.js` (critical contract)
- Add placeholder tool buttons wired with no-op `data-tool` attributes

### Phase 2 — Top Bar + Ribbon
- Animated logo mark, file menu dropdown (New/Open/Save/Export), presets
- Ribbon with 7 tabs × tool groups; active-tab underline sweep
- Keyboard-hint tooltips (`Kbd` spans)

### Phase 3 — Left Dock
- Feature Manager: live component tree rendered from the document, expand/collapse, selection highlight, icons per component kind
- Editor & Chat tabs restyled with glass input, send button glow, agent-log timeline
- Inspector tab: document metadata (units, name, mass props summary)

### Phase 4 — Viewport Overlays
- **ViewCube** (top-right): 6 faces + corners, click to snap camera, follows orbit rotation
- Floating mini-toolbar (left edge, vertical): measure, section, explode, annotate placeholders
- Selection chip (bottom-center) showing hovered/selected item
- Analysis HUD: mass / CoM / volume readouts top-left
- Grid + axes restyle with glow

### Phase 5 — Property Drawer + Status Bar
- Drawer: tabbed (Properties | Materials | Mass Props), animated expand
- Status bar: mode indicator, units selector, live stats with animated number ticks, notification toast stack

### Phase 6 — Settings Modal + Polish
- Full settings modal (AI settings + app prefs + shortcuts list)
- Keyboard shortcut overlay
- Polish pass: consistent spacing, focus rings, empty states, reduced-motion respect

---

## Compatibility Contract

`main.js` binds these IDs — MUST remain present and functional:
`editor, eval-btn, eval-btn-small, status-text, poly-count, mass-info, cache-stats, issues, kind-badge, vp-info, undo-btn, redo-btn, format-btn, props-panel, props-toggle-btn, props-header, props-wrapper, viewer, viewport-canvas-wrap, vp-rotate, vp-pan, vp-grid-toggle, vp-axes-toggle, vp-wireframe-toggle, vp-zoom-extents, vp-reset-cam, panel-resize, editor-panel, stl-btn, step-btn, preset-select, ai-settings-btn, ai-settings-modal, ai-settings-close, ai-settings-save, ai-api-key, ai-model-select, ai-endpoint, ai-mode-btn, ai-mode-label, ai-prompt, ai-send-btn, chat-messages, ai-constraint-badge`

New IDs added (namespace `ux-`): `ux-viewcube, ux-tree, ux-ribbon, ux-ribbon-tabs, ux-dock-tabs, ux-inspector, ux-sel-chip, ux-hud, ux-vp-sidebar, ux-status-mode, ux-units, ux-shortcuts-modal, ux-settings-panel...`

New JS added in `ux.js` (loaded before main.js, non-invasive, guards on element existence).