import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { TransformControls } from 'three/addons/controls/TransformControls.js';
import { initSyntaxEditor, getEditorRon, setEditorRon, selectSyntaxTarget, createSyntaxEditor } from './editor-syntax.js';
window.__boot && window.__boot.push('main: imports ok');

// ===== DOM refs =====
// The RON editor: syntax highlighting + folding live in editor-syntax.js.
// `editor` is a proxy — `.value` reads return the FULL source even while
// sections are folded, and writes rebuild the display model.
const editorRaw = document.getElementById('editor');
initSyntaxEditor(editorRaw);
const editor = {
  get value() { return getEditorRon(); },
  set value(v) { setEditorRon(v); },
  addEventListener(...a) { return editorRaw.addEventListener(...a); },
  dispatchEvent(...a) { return editorRaw.dispatchEvent(...a); },
};
const evalBtn = document.getElementById('eval-btn');
const evalBtnSmall = document.getElementById('eval-btn-small');
const statusText = document.getElementById('status-text');
const polyCount = document.getElementById('poly-count');
const massInfo = document.getElementById('mass-info');
const cacheStats = document.getElementById('cache-stats');
const issuesDiv = document.getElementById('issues');
const kindBadge = document.getElementById('kind-badge');
const vpInfo = document.getElementById('vp-info');
const undoBtn = document.getElementById('undo-btn');
const redoBtn = document.getElementById('redo-btn');
const formatBtn = document.getElementById('format-btn');
// Component properties popup (opens from the Feature Manager tree selection)
const vpPropsPopup = document.getElementById('vp-props-popup');
const vpPropsBody = document.getElementById('vp-props-body');
const vpPropsTitle = document.getElementById('vp-props-title');
const vpPropsKind = document.getElementById('vp-props-kind');
const vpPropsIcon = document.getElementById('vp-props-icon');
const vpPropsClose = document.getElementById('vp-props-close');
const vpPropsAntsRect = document.querySelector('#vp-props-ants rect');
const leaderSvg = document.getElementById('vp-leader-line');
const leaderSeg = document.getElementById('leader-seg');
const leaderDot = document.getElementById('leader-dot');

// ===== Three.js scene =====
const scene = new THREE.Scene();
scene.background = new THREE.Color(0x08080f);

let camera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
const perspCamera = camera; // stable reference to the perspective camera
let orthoCamera = null; // lazily created for true isometric mode
let isoActive = false;
camera.position.set(400, 300, 500);

const renderer = new THREE.WebGLRenderer({
  canvas: document.getElementById('viewer'),
  antialias: true,
});
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.2;

const ambient = new THREE.AmbientLight(0x404070, 0.5);
scene.add(ambient);

const dirLight = new THREE.DirectionalLight(0xffeedd, 1.5);
dirLight.position.set(300, 400, 500);
scene.add(dirLight);

const fillLight = new THREE.DirectionalLight(0x8888ff, 0.5);
fillLight.position.set(-200, -100, -300);
scene.add(fillLight);

const rimLight = new THREE.DirectionalLight(0x4488ff, 0.3);
rimLight.position.set(0, -300, 200);
scene.add(rimLight);

const gridHelper = new THREE.GridHelper(600, 20, 0x00d4aa, 0x1a3355);
gridHelper.position.y = -0.1;
scene.add(gridHelper);

const axesHelper = new THREE.AxesHelper(200);
scene.add(axesHelper);

let meshGroup = new THREE.Group();
scene.add(meshGroup);

let showWireframe = false;
const wireframeGroup = new THREE.Group();
scene.add(wireframeGroup);

// ===== Camera defaults =====
const DEFAULT_CAM_POS = new THREE.Vector3(400, 300, 500);
const DEFAULT_TARGET = new THREE.Vector3(0, 0, 150);

// ===== Sizing =====
function updateSize() {
  const container = document.getElementById('viewport-canvas-wrap');
  const w = container.clientWidth;
  const h = container.clientHeight;
  if (w < 1 || h < 1) return;
  renderer.setSize(w, h);
  if (camera === orthoCamera) {
    setOrthoFrustum(orthoCamera.userData.viewHeight || 400);
  } else {
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  }
}
window.addEventListener('resize', updateSize);

// ===== Controls =====
function makeControls(cam) {
  const c = new OrbitControls(cam, renderer.domElement);
  c.target.copy(DEFAULT_TARGET);
  c.enableDamping = true;
  c.dampingFactor = 0.12;
  c.minDistance = 10;
  c.maxDistance = 5000;
  c.minZoom = 0.02; // ortho zoom bounds
  c.maxZoom = 200;
  c.rotateSpeed = 0.8;
  c.mouseButtons = {
    LEFT: THREE.MOUSE.ROTATE,
    MIDDLE: THREE.MOUSE.DOLLY,
    RIGHT: THREE.MOUSE.PAN,
  };
  c.touches = {
    ONE: THREE.TOUCH.ROTATE_PAN,
    TWO: THREE.TOUCH.DOLLY_PAN,
  };
  return c;
}
let controls = makeControls(camera);
controls.update();

// ===== Isometric ⇄ Perspective cycle (smooth projection morph) =====
// True isometric: orthographic camera. The transition animates the perspective
// FOV toward telephoto while walking the camera back so the framed view-height
// stays CONSTANT — visually converging to orthographic — then swaps cameras
// seamlessly. Reversing does the mirror image.
const ISO_END_FOV = 10; // degrees; at this FOV perspective ≈ orthographic
const PROJ_ANIM_MS = 340;
let projBusy = false;

function easeInOutCubic(t) {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

function currentViewHeight() {
  if (camera === orthoCamera) {
    return (orthoCamera.userData.viewHeight || 400) / (camera.zoom || 1);
  }
  const dist = Math.max(1, camera.position.distanceTo(controls.target));
  return 2 * dist * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2));
}

function animatePerspFov(targetFov, durationMs) {
  return new Promise(resolve => {
    const cam = perspCamera;
    const startFov = cam.fov;
    const H = currentViewHeight(); // constant framing invariant
    controls.enabled = false; // no input fights mid-morph
    const t0 = performance.now();
    const step = now => {
      const k = Math.min(1, (now - t0) / durationMs);
      const e = easeInOutCubic(k);
      const fov = startFov + (targetFov - startFov) * e;
      const dist = H / (2 * Math.tan(THREE.MathUtils.degToRad(fov / 2)));
      cam.fov = fov;
      cam.updateProjectionMatrix();
      const dir = new THREE.Vector3().subVectors(cam.position, controls.target).normalize();
      cam.position.copy(controls.target).addScaledVector(dir, dist);
      controls.update();
      if (k < 1) requestAnimationFrame(step);
      else { controls.enabled = true; resolve(); }
    };
    requestAnimationFrame(step);
  });
}

function setOrthoFrustum(viewHeight) {
  if (!orthoCamera) return;
  const wrap = document.getElementById('viewport-canvas-wrap');
  const aspect = Math.max(0.01, wrap.clientWidth / Math.max(1, wrap.clientHeight));
  orthoCamera.userData.viewHeight = viewHeight;
  orthoCamera.left = -viewHeight * aspect / 2;
  orthoCamera.right = viewHeight * aspect / 2;
  orthoCamera.top = viewHeight / 2;
  orthoCamera.bottom = -viewHeight / 2;
  orthoCamera.updateProjectionMatrix();
}

function switchActiveCamera(newCam) {
  const target = controls.target.clone();
  controls.dispose();
  camera = newCam;
  controls = makeControls(camera);
  controls.target.copy(target);
  // Keep the gizmo aligned with whichever camera is rendering.
  if (transformGizmo) transformGizmo.camera = newCam;
  controls.update();
}

async function enterIso() {
  if (!orthoCamera) {
    orthoCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.01, 50000);
  }
  await animatePerspFov(ISO_END_FOV, PROJ_ANIM_MS);
  // Seamless swap: the ortho frustum equals the final animated view height,
  // so the last animated frame and the first ortho frame are identical.
  setOrthoFrustum(currentViewHeight());
  orthoCamera.position.copy(perspCamera.position); // same eye point & direction
  orthoCamera.zoom = 1;
  orthoCamera.updateProjectionMatrix();
  switchActiveCamera(orthoCamera);
  isoActive = true;
}

async function exitIso() {
  // Start the perspective camera in its near-orthographic twin state, then
  // relax the FOV back to normal — again preserving view height throughout.
  const vh = currentViewHeight(); // zoom-aware ortho extent
  const eyeDist = vh / (2 * Math.tan(THREE.MathUtils.degToRad(ISO_END_FOV / 2)));
  let dir = new THREE.Vector3().subVectors(camera.position, controls.target).normalize();
  if (!isFinite(dir.x) || dir.lengthSq() < 0.5) dir.set(0.6, 0.5, 0.8).normalize();
  perspCamera.fov = ISO_END_FOV;
  perspCamera.updateProjectionMatrix();
  perspCamera.position.copy(controls.target).addScaledVector(dir, eyeDist);
  switchActiveCamera(perspCamera);
  isoActive = false;
  await animatePerspFov(45, PROJ_ANIM_MS);
}

async function toggleIsoView() {
  if (projBusy) return isoActive; // ignore clicks while morphing
  projBusy = true;
  try {
    if (isoActive) await exitIso();
    else await enterIso();
  } finally {
    projBusy = false;
  }
  return isoActive;
}

// ===== Display modes: shaded | wireframe | xray =====
let displayMode = 'shaded';
let gridVisible = true;
let axesVisible = true;

function applyDisplayMode() {
  const isWire = displayMode === 'wireframe';
  const isXray = displayMode === 'xray';
  const edgesOn = isWire || isXray || showWireframe;
  meshGroup.visible = !isWire; // pure wireframe hides the solid bodies
  // The group alone isn't enough: every edge LineSegments child also carries
  // its own .visible flag (from creation/toolbar), so sync those explicitly.
  wireframeGroup.visible = true;
  wireframeGroup.children.forEach(c => { c.visible = edgesOn; });
  componentMeshes.forEach(list => list.forEach(m => {
    const mat = m.material;
    if (!mat) return;
    // The selected part keeps full presence in every mode.
    if (isXray && m.userData.componentName !== selectedComponentName) {
      mat.transparent = true;
      mat.opacity = 0.25;
      mat.depthWrite = false;
    } else if (!dimmedMaterials.some(d => d.m === mat)) {
      // don't stomp selection dimming
      mat.transparent = false;
      mat.opacity = 1;
      mat.depthWrite = true;
    }
  }));
}

function setDisplayMode(mode) {
  if (!['shaded', 'wireframe', 'xray'].includes(mode)) return;
  displayMode = mode;
  applyDisplayMode();
}

function getDisplayMode() { return displayMode; }

function toggleGrid() {
  gridVisible = !gridVisible;
  gridHelper.visible = gridVisible;
  return gridVisible;
}

function toggleAxes() {
  axesVisible = !axesVisible;
  axesHelper.visible = axesVisible;
  return axesVisible;
}

// ===== UX hook: expose scene handles to ux.js (non-invasive) =====
// camera/controls are LIVE getters: the isometric cycle swaps both instances,
// and every external consumer must follow the active ones.
window.__apro = {
  get camera() { return camera; },
  get controls() { return controls; },
  scene,
  meshGroup,
  wireframeGroup,
  get gridVisible() { return gridVisible; },
  get axesVisible() { return axesVisible; },
  getDisplayMode,
  setDisplayMode,
  toggleGrid,
  toggleAxes,
  fitView: () => frameToFit(),
  toggleIsoView,
  get isoActive() { return isoActive; },
  showGrid: (v) => { gridVisible = !!v; gridHelper.visible = gridVisible; },
  showAxes: (v) => { axesVisible = !!v; axesHelper.visible = axesVisible; },
  setWireframe: (v) => {
    showWireframe = !!v;
    applyDisplayMode();
  },
};

// ===== Status helpers =====
function setStatus(text, isBusy) {
  statusText.dataset.busy = isBusy ? '1' : '';
  statusText.innerHTML = isBusy ? '<span class="spinner"></span> ' + text : text;
}

function showIssues(issues) {
  issuesDiv.innerHTML = '';
  if (!issues || issues.length === 0) return;
  issues.forEach(issue => {
    const el = document.createElement('div');
    el.className = 'issue issue-' + issue.severity.toLowerCase();
    el.textContent = issue.message;
    issuesDiv.appendChild(el);
  });
}

// ===== Mesh helpers =====
function clearMeshes() {
  while (meshGroup.children.length > 0) {
    const child = meshGroup.children[0];
    meshGroup.remove(child);
    if (child.geometry) child.geometry.dispose();
    if (child.material) child.material.dispose();
  }
  while (wireframeGroup.children.length > 0) {
    const child = wireframeGroup.children[0];
    wireframeGroup.remove(child);
    if (child.geometry) child.geometry.dispose();
    if (child.material) child.material.dispose();
  }
}

// ===== Component colors =====
// Palette used when a component has no explicit `color`: keyed by a hash of
// the material name so the same material always renders the same color.
const COMPONENT_PALETTE = [0x00d4aa, 0x4f8bff, 0xff8f4f, 0xb44fff, 0xff4f87, 0x4fd0ff, 0xa8e05f, 0xf5d76e];
const CSS_COLOR_NAMES = {
  red: 0xe74c3c, green: 0x2ecc71, blue: 0x3498db, yellow: 0xf1c40f,
  orange: 0xe67e22, purple: 0x9b59b6, violet: 0x9b59b6, pink: 0xfd79a8,
  white: 0xecf0f1, black: 0x2d3436, gray: 0x95a5a6, grey: 0x95a5a6,
  silver: 0xbdc3c7, gold: 0xd4af37, cyan: 0x00cec9, magenta: 0xe84393,
  teal: 0x00b894, brown: 0x8d6e63, lime: 0xa3e635, navy: 0x1e3a8a,
};
function hashString(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); }
  return h >>> 0;
}
function parseColorString(s) {
  if (!s) return null;
  const t = String(s).trim().toLowerCase();
  let m = t.match(/^#([0-9a-f]{6})$/);
  if (m) return parseInt(m[1], 16);
  m = t.match(/^#([0-9a-f]{3})$/);
  if (m) return parseInt(m[1].split('').map(c => c + c).join(''), 16);
  if (CSS_COLOR_NAMES[t] != null) return CSS_COLOR_NAMES[t];
  return null;
}
function componentColor(comp) {
  const explicit = parseColorString(comp.color);
  if (explicit != null) return explicit;
  const key = (comp.material || comp.name || 'part').trim().toLowerCase();
  return COMPONENT_PALETTE[hashString(key) % COMPONENT_PALETTE.length];
}

// ===== Selection (Feature Manager -> 3D highlight + properties popup) =====
const componentMeshes = new Map(); // component name -> [THREE.Mesh]
const componentEdges = new Map();  // component name -> [wireframe LineSegments]
let selectedComponentName = null;
let selectionColorHex = 0x00d4aa;
let selectionOutlines = []; // backside shells added as children of selected meshes
let dimmedMaterials = [];   // materials dimmed while a component is focused

function randomSelectionColor() {
  const c = new THREE.Color().setHSL(Math.random(), 0.85, 0.6);
  return c.getHex();
}

function clearSelectionOutline() {
  selectionOutlines.forEach(shell => {
    if (shell.parent) shell.parent.remove(shell);
    if (shell.material) shell.material.dispose();
  });
  selectionOutlines = [];
  restoreFocusEffects();
}

// Focus effects: every NON-selected component is darkened, made translucent
// and visually recedes; the selected part keeps full presence.
function applyFocusEffects() {
  if (!selectedComponentName || selectedComponentName === '__parameters__') return;
  componentMeshes.forEach((meshes, name) => {
    if (name === selectedComponentName) return;
    meshes.forEach(mesh => {
      const m = mesh.material;
      if (!m) return;
      dimmedMaterials.push({
        m,
        color: m.color.getHex(),
        opacity: m.opacity,
        transparent: m.transparent,
        depthWrite: m.depthWrite,
      });
      m.transparent = true;
      m.opacity = 0.12;
      m.depthWrite = false;
      m.color.setHex(0x11141d); // darken toward the background
    });
    const edges = componentEdges.get(name) || [];
    edges.forEach(ln => {
      const m = ln.material;
      if (!m) return;
      dimmedMaterials.push({ m, color: m.color.getHex(), opacity: m.opacity, transparent: m.transparent, depthWrite: true });
      m.transparent = true;
      m.opacity = 0.05;
    });
  });
}

function restoreFocusEffects() {
  dimmedMaterials.forEach(d => {
    d.m.color.setHex(d.color);
    d.m.opacity = d.opacity;
    d.m.transparent = d.transparent;
    d.m.depthWrite = d.depthWrite;
  });
  dimmedMaterials = [];
}

// Silhouette outline: a slightly enlarged backface shell around each mesh of
// the selected component. The shell is recentered so it hugs parts that sit
// away from the world origin.
function applySelectionOutline() {
  clearSelectionOutline();
  const meshes = componentMeshes.get(selectedComponentName) || [];
  meshes.forEach(mesh => {
    const shell = new THREE.Mesh(
      mesh.geometry,
      new THREE.MeshBasicMaterial({ color: selectionColorHex, side: THREE.BackSide })
    );
    const attr = mesh.geometry.attributes.position;
    const box = new THREE.Box3().setFromBufferAttribute(attr);
    const center = box.getCenter(new THREE.Vector3());
    const s = 1.04;
    shell.scale.setScalar(s);
    shell.position.copy(center).multiplyScalar(1 - s);
    shell.raycast = () => {}; // never block orbit clicks
    mesh.add(shell);
    selectionOutlines.push(shell);
  });
  applyFocusEffects();
}

function deselectComponent() {
  selectedComponentName = null;
  clearSelectionOutline();
  stopLeaderLoop();
  selectSyntaxTarget(null);
  if (transformGizmo) transformGizmo.detach();
  vpPropsPopup.style.display = 'none';
  window.dispatchEvent(new CustomEvent('ux:deselect-part'));
}

async function selectComponent(name) {
  selectedComponentName = name;
  selectionColorHex = randomSelectionColor();
  applySelectionOutline();
  showLeaderLoop();
  selectSyntaxTarget(name);
  syncGizmoAttach(); // gizmo follows the newly selected part (when a tool is active)
  await showPropsPopup();
  // Sync the Feature Manager tree + selection chip with this selection.
  window.dispatchEvent(new CustomEvent('ux:sync-selection', { detail: { name } }));
}

// Re-apply the current selection's outline after a rebuild (evaluate()); the
// color stays stable while the selection persists.
function refreshSelectionAfterRebuild() {
  clearSelectionOutline();
  if (!selectedComponentName || selectedComponentName === '__parameters__') return;
  applySelectionOutline();
  syncGizmoAttach(); // re-attach to the freshly rebuilt mesh
}

// ===== Leader line: popup edge -> selected component =====
let leaderRaf = null;

function hideLeaderLine() {
  if (leaderSvg) leaderSvg.style.display = 'none';
}

function updateLeaderLine() {
  if (!leaderSvg || !selectedComponentName || selectedComponentName === '__parameters__' ||
      vpPropsPopup.style.display === 'none') {
    hideLeaderLine();
    return;
  }
  const meshes = componentMeshes.get(selectedComponentName);
  if (!meshes || meshes.length === 0) { hideLeaderLine(); return; }

  // Anchor: world-space center of the selected component, projected to screen.
  const box = new THREE.Box3();
  meshes.forEach(m => box.expandByObject(m));
  if (box.isEmpty()) { hideLeaderLine(); return; }
  const center = box.getCenter(new THREE.Vector3()).project(camera);

  const wrap = document.getElementById('viewport-canvas-wrap');
  const wr = wrap.getBoundingClientRect();
  if (center.z > 1) { hideLeaderLine(); return; } // behind the camera
  const mx = (center.x * 0.5 + 0.5) * wr.width;
  const my = (-center.y * 0.5 + 0.5) * wr.height;

  // Start: vertical-center point of the popup edge nearest to the component.
  const pr = vpPropsPopup.getBoundingClientRect();
  const px = pr.left - wr.left;
  const py = pr.top - wr.top;
  const edgeX = (px + pr.width / 2) < mx ? px + pr.width : px;
  const edgeY = py + pr.height / 2;

  const colCss = hexColorCss(selectionColorHex);
  leaderSeg.setAttribute('x1', edgeX); leaderSeg.setAttribute('y1', edgeY);
  leaderSeg.setAttribute('x2', mx); leaderSeg.setAttribute('y2', my);
  leaderSeg.setAttribute('stroke', colCss);
  leaderDot.setAttribute('cx', mx); leaderDot.setAttribute('cy', my);
  leaderDot.setAttribute('fill', colCss);
  leaderSvg.style.display = 'block';
}

function showLeaderLoop() {
  stopLeaderLoop();
  const loop = () => { updateLeaderLine(); leaderRaf = requestAnimationFrame(loop); };
  leaderRaf = requestAnimationFrame(loop);
}

function stopLeaderLoop() {
  if (leaderRaf) cancelAnimationFrame(leaderRaf);
  leaderRaf = null;
  hideLeaderLine();
}

// ===== Transform gizmos (Move / Scale / Rotate tools) =====
let gizmoMode = 'cursor'; // 'cursor' | 'translate' | 'scale' | 'rotate'
let transformGizmo = null;
let gizmoBase = null; // document-space pose of the attached component
let gizmoAttachCenter = new THREE.Vector3(); // part center at attach time
let suppressClickSelect = false;

function ensureTransformGizmo() {
  if (transformGizmo) return transformGizmo;
  const tc = new TransformControls(camera, renderer.domElement);
  tc.setSize(0.9);
  tc.setSpace('world');
  scene.add(tc);
  tc.addEventListener('dragging-changed', e => { controls.enabled = !e.value; });
  tc.addEventListener('mouseUp', () => commitGizmoTransform());
  // Suppress click-select when a drag starts on a gizmo handle.
  renderer.domElement.addEventListener('pointerdown', () => {
    if (gizmoMode !== 'cursor' && transformGizmo && transformGizmo.axis) {
      suppressClickSelect = true;
    }
  }, true);
  renderer.domElement.addEventListener('pointerup', () => {
    setTimeout(() => { suppressClickSelect = false; }, 0);
  }, true);
  transformGizmo = tc;
  return tc;
}

async function loadGizmoBase(name) {
  try {
    const invoke = tauriInvoke();
    if (!invoke) return;
    const tables = await invoke('describe_vehicle', { vehicleRon: getEditorRon() });
    if (selectedComponentName !== name) return;
    const t = (tables || []).find(x => x.name === name);
    if (!t) return;
    const get = k => { const r = t.rows.find(x => x.key === k); return r ? parseFloat(r.value) : 0; };
    gizmoBase = {
      pos: { x: get('pos_x'), y: get('pos_y'), z: get('pos_z') },
      rot: { x: get('rot_x_deg'), y: get('rot_y_deg'), z: get('rot_z_deg') },
      scale: { x: get('scale_x') || 1, y: get('scale_y') || 1, z: get('scale_z') || 1 },
    };
  } catch {}
}

function syncGizmoAttach() {
  if (!transformGizmo) return;
  if (gizmoMode === 'cursor' || !selectedComponentName || selectedComponentName === '__parameters__') {
    transformGizmo.detach();
    gizmoBase = null;
    return;
  }
  const list = componentMeshes.get(selectedComponentName);
  const mesh = list && list[0];
  if (!mesh) { transformGizmo.detach(); gizmoBase = null; return; }
  mesh.rotation.order = 'XYZ'; // matches the kernel's Rx·Ry·Rz convention
  transformGizmo.attach(mesh);
  gizmoAttachCenter.copy(mesh.position); // the part's own center
  loadGizmoBase(selectedComponentName);
}

function setGizmoMode(mode) {
  if (!['cursor', 'translate', 'scale', 'rotate'].includes(mode)) return;
  gizmoMode = mode;
  ensureTransformGizmo().setMode(mode === 'cursor' ? 'translate' : mode);
  syncGizmoAttach();
  updateToolButtons();
}

function updateToolButtons() {
  const map = { cursor: 'tool-cursor', translate: 'tool-move', scale: 'tool-scale', rotate: 'tool-rotate' };
  document.querySelectorAll('#ux-vp-sidebar [data-tool^="tool-"]').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.tool === map[gizmoMode]);
  });
}

function commitGizmoTransform() {
  if (!selectedComponentName || !gizmoBase) return;
  const obj = transformGizmo && transformGizmo.object;
  if (!obj) return;
  const name = selectedComponentName;
  const f = v => String(Math.round(v * 1000) / 1000);
  let entries = [];

  if (gizmoMode === 'translate') {
    // Delta relative to where the part's center sat at attach time.
    entries = [
      ['pos_x', gizmoBase.pos.x + (obj.position.x - gizmoAttachCenter.x)],
      ['pos_y', gizmoBase.pos.y + (obj.position.y - gizmoAttachCenter.y)],
      ['pos_z', gizmoBase.pos.z + (obj.position.z - gizmoAttachCenter.z)],
    ];
  } else if (gizmoMode === 'rotate' || gizmoMode === 'scale') {
    // Compose base pose with the gizmo delta, then solve for the translation
    // that keeps the part's center exactly where it was.
    const qBase = new THREE.Quaternion().setFromEuler(new THREE.Euler(
      THREE.MathUtils.degToRad(gizmoBase.rot.x),
      THREE.MathUtils.degToRad(gizmoBase.rot.y),
      THREE.MathUtils.degToRad(gizmoBase.rot.z), 'XYZ'));
    const sBase = new THREE.Vector3(gizmoBase.scale.x, gizmoBase.scale.y, gizmoBase.scale.z);
    const mOld = new THREE.Matrix4().compose(
      new THREE.Vector3(gizmoBase.pos.x, gizmoBase.pos.y, gizmoBase.pos.z), qBase, sBase);

    const qDelta = obj.quaternion.clone(); // started from identity
    const sMul = obj.scale.clone();        // started from (1,1,1)

    let qNew, sNew;
    if (gizmoMode === 'rotate') {
      qNew = qDelta.clone().multiply(qBase).normalize();
      sNew = sBase.clone();
    } else {
      qNew = qBase.clone();
      sNew = sBase.clone().multiply(sMul);
    }

    // Local centroid: where the part's center lives in untransformed space.
    const lLocal = gizmoAttachCenter.clone()
      .applyMatrix4(mOld.clone().invert());

    // Predicted center with identity translation, then solve for T_new so the
    // center stays put under the new rotation/scale.
    const mNew0 = new THREE.Matrix4().compose(new THREE.Vector3(), qNew, sNew);
    const cPred = lLocal.clone().applyMatrix4(mNew0);
    const tNew = gizmoAttachCenter.clone().sub(cPred);

    const eu = new THREE.Euler().setFromQuaternion(qNew, 'XYZ');
    entries = [
      ['pos_x', tNew.x], ['pos_y', tNew.y], ['pos_z', tNew.z],
      ['rot_x_deg', THREE.MathUtils.radToDeg(eu.x)],
      ['rot_y_deg', THREE.MathUtils.radToDeg(eu.y)],
      ['rot_z_deg', THREE.MathUtils.radToDeg(eu.z)],
      ['scale_x', sNew.x], ['scale_y', sNew.y], ['scale_z', sNew.z],
    ].filter(([k]) =>
      gizmoMode === 'rotate' ? !k.startsWith('scale_') : !k.startsWith('rot_'));
  }

  if (entries.length === 0) return;
  const patchRon = 'PatchList(patches:[' +
    entries.map(([k, v]) => `SetProperty(component_name: "${name}", key: "${k}", value: "${f(v)}")`).join(',') +
    '])';
  // Reset the visual proxy; evaluate() rebuilds from the updated document.
  obj.position.set(0, 0, 0);
  obj.quaternion.identity();
  obj.scale.set(1, 1, 1);
  applyPatchListToDoc(patchRon);
}

async function applyPatchListToDoc(patchRon) {
  try {
    const invoke = tauriInvoke();
    if (!invoke) return;
    const wasComp = isComponentRon(getEditorRon());
    const res = await invoke('apply_patch_ron', { vehicleRon: wrapRon(getEditorRon()), patchRon });
    if (res.success && res.vehicle_ron) {
      editor.value = unwrapRon(res.vehicle_ron, wasComp);
      evaluate();
    } else {
      statusText.textContent = 'Transform rejected: ' + ((res.issues || []).map(i => i.message).join('; ') || '?');
      refreshSelectedPopup();
    }
  } catch (e) {
    statusText.textContent = 'Transform error: ' + e;
  }
}

function buildMeshesFromResult(result) {
  if (!result.mesh) return [];
  const comps = Array.isArray(result.components) ? result.components : [];

  function makeMesh(posArr, normArr, idxArr, color, compName) {
    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.BufferAttribute(new Float32Array(posArr), 3));
    geom.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(normArr), 3));
    geom.setIndex(new THREE.BufferAttribute(new Uint32Array(idxArr), 1));

    // Recenter: origin moves to the part's own center so gizmos (and
    // rotate/scale pivots) sit on the component, not the world origin.
    // Guard against NaN geometry — one bad vertex would poison the whole
    // bounding box and blank the part.
    geom.computeBoundingBox();
    const bb = geom.boundingBox;
    const nanVerts = (() => {
      const p = geom.attributes.position.array;
      let n = 0;
      for (let i = 0; i < p.length; i++) if (!Number.isFinite(p[i])) n++;
      return n;
    })();
    if (nanVerts > 0 && SCENE_DEBUG) {
      console.warn('[scene] component', compName || '(merged)', 'has', nanVerts, 'non-finite vertices');
    }
    const center = new THREE.Vector3(
      Number.isFinite(bb.min.x) && Number.isFinite(bb.max.x) ? (bb.min.x + bb.max.x) / 2 : 0,
      Number.isFinite(bb.min.y) && Number.isFinite(bb.max.y) ? (bb.min.y + bb.max.y) / 2 : 0,
      Number.isFinite(bb.min.z) && Number.isFinite(bb.max.z) ? (bb.min.z + bb.max.z) / 2 : 0
    );
    geom.translate(-center.x, -center.y, -center.z);

    const mat = new THREE.MeshPhysicalMaterial({
      color,
      metalness: 0.1,
      roughness: 0.3,
      clearcoat: 0.15,
      clearcoatRoughness: 0.4,
      side: THREE.DoubleSide,
      envMapIntensity: 1.0,
    });
    const mesh = new THREE.Mesh(geom, mat);
    mesh.position.copy(center); // carry the center in the transform
    mesh.castShadow = true;
    mesh.receiveShadow = true;

    // Edges AFTER recentering so they line up with the shifted geometry.
    const edgeGeom = new THREE.EdgesGeometry(geom);
    const edgeMat = new THREE.LineBasicMaterial({
      color,
      transparent: true,
      opacity: 0.18,
    });
    const wireframe = new THREE.LineSegments(edgeGeom, edgeMat);
    wireframe.visible = showWireframe;
    wireframe.userData.componentName = compName;
    wireframeGroup.add(wireframe);
    return mesh;
  }

  // Per-component rendering: each part in its own color (explicit `color`
  // field, else derived from the material name). Invisible parts are skipped.
  if (comps.length > 0) {
    const meshes = comps
      .filter(comp => comp.visible !== false && comp.positions && comp.positions.length > 0)
      .map(comp => {
        const mesh = makeMesh(comp.positions, comp.normals, comp.indices, componentColor(comp), comp.name);
        mesh.userData.componentName = comp.name;
        return mesh;
      });
    if (meshes.length > 0) return meshes;
    // All components empty/invisible: fall through to the merged mesh so the
    // viewport still reflects whatever the backend produced.
  }

  const pos = new Float32Array(result.mesh.positions);
  const norms = new Float32Array(result.mesh.normals);
  const idx = new Uint32Array(result.mesh.indices);

  const geom = new THREE.BufferGeometry();
  geom.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  geom.setAttribute('normal', new THREE.BufferAttribute(norms, 3));
  geom.setIndex(new THREE.BufferAttribute(idx, 1));

  const mat = new THREE.MeshPhysicalMaterial({
    color: 0x00d4aa,
    metalness: 0.1,
    roughness: 0.3,
    clearcoat: 0.15,
    clearcoatRoughness: 0.4,
    side: THREE.DoubleSide,
    envMapIntensity: 1.0,
  });

  const mesh = new THREE.Mesh(geom, mat);
  mesh.castShadow = true;
  mesh.receiveShadow = true;

  const edgeGeom = new THREE.EdgesGeometry(geom);
  const edgeMat = new THREE.LineBasicMaterial({
    color: 0x88ffee,
    transparent: true,
    opacity: 0.15,
  });
  const wireframe = new THREE.LineSegments(edgeGeom, edgeMat);
  wireframe.visible = showWireframe;
  wireframeGroup.add(wireframe);

  return [mesh];
}

function updateStats(result) {
  if (result.mesh) {
    const vertexCount = result.mesh.positions.length / 3;
    const faceCount = result.mesh.indices.length / 3;
    const compInfo = result.component_count > 1 ? ` (${result.component_count} cmp)` : '';
    polyCount.innerHTML = `<span class="stat-value">${vertexCount}</span> verts · <span class="stat-value">${faceCount}</span> faces${compInfo}`;

    if (result.cache_misses > 0 || result.cache_hits > 0) {
      const total = result.cache_hits + result.cache_misses;
      const pct = total > 0 ? Math.round(result.cache_hits / total * 100) : 0;
      cacheStats.innerHTML = `cache: <span class="stat-value">${pct}%</span> (${result.cache_hits}H / ${result.cache_misses}M)`;
    } else {
      cacheStats.textContent = '';
    }

    if (result.mass_props) {
      const mp = result.mass_props;
      massInfo.innerHTML = `<span class="stat-value">${mp.mass.toFixed(3)}</span> kg  CM: (${mp.center_of_mass.map(v => v.toFixed(0)).join(', ')})`;
    } else {
      massInfo.textContent = '';
    }
  } else {
    polyCount.textContent = '';
    massInfo.textContent = '';
    cacheStats.textContent = '';
  }
}

// ===== Evaluate =====
// Temporary scene diagnostics (flip to false once rendering issues are resolved)
const SCENE_DEBUG = true;
function debugSceneState(tag) {
  if (!SCENE_DEBUG) return;
  const cam = camera;
  const first = meshGroup.children[0];
  const info = {
    tag,
    mode: displayMode,
    meshGroupVisible: meshGroup.visible,
    meshes: meshGroup.children.length,
    edges: wireframeGroup.children.length,
    edgesVisible: wireframeGroup.children.filter(c => c.visible).length,
    firstMat: first && first.material
      ? { opacity: first.material.opacity, transparent: first.material.transparent, depthWrite: first.material.depthWrite }
      : null,
    camType: cam === orthoCamera ? 'ortho' : 'persp',
    fov: cam.fov,
    camPos: cam.position.toArray().map(v => Math.round(v)),
    target: controls.target.toArray().map(v => Math.round(v)),
    zoom: Math.round((cam.zoom || 1) * 100) / 100,
    viewH: cam === orthoCamera ? Math.round(orthoCamera.userData.viewHeight || 0) : undefined,
  };
  console.debug('[scene]', JSON.stringify(info));
  // Mirror into the status bar so diagnosis doesn't require devtools.
  try {
    const s = document.getElementById('status-text');
    if (s && !s.dataset.busy) {
      s.innerHTML += ` <span style="opacity:.65;font-size:9px">[${info.tag}: ${info.meshes} mesh · ${info.camType}${info.viewH ? ' · vh' + info.viewH : ''}]</span>`;
    }
  } catch {}
}

// ===== Evaluation progress bar =====
let evalProgressBound = false;

function bindEvalProgressEvents() {
  if (evalProgressBound) return;
  const ev = window.__TAURI__?.event;
  if (!ev || typeof ev.listen !== 'function') {
    console.warn('[progress] Tauri event API unavailable — indeterminate fallback');
    return;
  }
  evalProgressBound = true;
  ev.listen('eval-progress', e => {
    const p = e.payload || {};
    setEvalProgress(p.phase, p.done || 0, p.total || 0, p.current);
  }).catch(err => console.warn('[progress] listen failed:', err));
}

let evalShownAt = 0;

function showEvalProgress(total) {
  const wrap = document.getElementById('eval-progress');
  const fill = document.getElementById('ep-fill');
  const label = document.getElementById('ep-label');
  if (!wrap) return;
  evalShownAt = performance.now();
  wrap.classList.remove('indeterminate');
  wrap.style.display = 'block';
  // Two-phase stream: total steps = 2 × components (tessellate + assemble).
  fill.style.width = '2%';
  label.textContent = 'Evaluating…';
}

function setEvalProgress(phase, done, total, current) {
  const wrap = document.getElementById('eval-progress');
  const fill = document.getElementById('ep-fill');
  const label = document.getElementById('ep-label');
  if (!wrap || !fill) return;
  wrap.classList.remove('indeterminate');
  wrap.style.display = 'block';
  const pct = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 100;
  fill.style.width = pct + '%';
  const verb = phase === 'assemble' ? 'Assembling' : 'Tessellating';
  label.textContent = `${verb} ${current} (${done}/${total})`;
}

async function hideEvalProgress() {
  // Keep the bar visible long enough to be perceivable on fast evaluations.
  const elapsed = performance.now() - evalShownAt;
  const MIN_VISIBLE = 450;
  if (elapsed < MIN_VISIBLE) await new Promise(r => setTimeout(r, MIN_VISIBLE - elapsed));
  const wrap = document.getElementById('eval-progress');
  const fill = document.getElementById('ep-fill');
  if (wrap) {
    if (fill) fill.style.width = '100%'; // complete flash
    setTimeout(() => { if (wrap) wrap.style.display = 'none'; }, 140);
  }
}

async function evaluate() {
  setStatus('Evaluating...', true);
  issuesDiv.innerHTML = '';

  const ron = editor.value;

  // Empty (or comment-only) document: clear everything silently.
  if (!stripRonComments(ron).trim()) {
    clearMeshes();
    clearSelectionOutline();
    showIssues([]);
    polyCount.textContent = '';
    massInfo.textContent = '';
    cacheStats.textContent = '';
    updateViewportInfo(null);
    if (selectedComponentName) deselectComponent();
    setStatus('Empty document', false);
    return;
  }

  try {
    const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
    if (!invoke) throw new Error('Tauri IPC not available');
    const isVehicle = stripRonComments(ron).startsWith('Vehicle(');
    const cmd = isVehicle ? 'evaluate_vehicle' : 'evaluate';
    const argKey = isVehicle ? 'vehicleRon' : 'componentRon';
    bindEvalProgressEvents();
    showEvalProgress();
    let result = await invoke(cmd, { [argKey]: ron });
    await hideEvalProgress();

    // Defensive: if the text was mis-detected as a Component but it actually
    // carries `components:`, retry as a full Vehicle (RON anonymous structs
    // have no `Vehicle(` prefix to detect on).
    if (!isVehicle && (result.issues || []).some(i => /missing field `kind`|unexpected field/i.test(i.message))) {
      const asVehicle = stripRonComments(ron).includes('components:');
      if (asVehicle) {
        showEvalProgress();
        result = await invoke('evaluate_vehicle', { vehicleRon: ron });
        await hideEvalProgress();
      }
    }

    showIssues(result.issues);
    clearMeshes();
    clearSelectionOutline();

    if (result.mesh) {
      componentMeshes.clear();
      componentEdges.clear();
      const built = buildMeshesFromResult(result);
      if (SCENE_DEBUG) {
        console.debug('[scene] backend verts:', result.mesh.positions.length / 3,
          '| built meshes:', built.length,
          '| comps in payload:', (result.components || []).length);
        built.slice(0, 8).forEach(m => {
          m.geometry.computeBoundingBox();
          const b = m.geometry.boundingBox;
          console.debug('[scene] part', m.userData.componentName || '(merged)',
            'verts', m.geometry.attributes.position.count,
            'pos', m.position.toArray().map(v => Math.round(v)).join(','),
            'bbox', [b.min, b.max].flatMap(p => [p.x, p.y, p.z].map(v => Math.round(v))).join('/'));
        });
      }
      built.forEach(m => {
        meshGroup.add(m);
        const n = m.userData.componentName;
        if (n) {
          if (!componentMeshes.has(n)) componentMeshes.set(n, []);
          componentMeshes.get(n).push(m);
        }
      });
      wireframeGroup.children.forEach(w => {
        const n = w.userData && w.userData.componentName;
        if (!n) return;
        if (!componentEdges.has(n)) componentEdges.set(n, []);
        componentEdges.get(n).push(w);
      });
      refreshSelectionAfterRebuild();
      applyDisplayMode();
      debugSceneState('after-build+display');
      if (built.length === 0 && result.mesh.positions.length > 0) {
        setStatus('Warning: geometry produced but no preview meshes built', false);
      }
      updateStats(result);
      setStatus('OK', false);
      if (agentAutoFit) {
        agentAutoFit = false;
        frameToFit();
      } else {
        autoFrameCamera();
      }
      debugSceneState('after-frame');
      updateViewportInfo(result);

      const descRon = isVehicle ? ron : `Vehicle( name: "tmp", units: Millimeters, components: [${ron}] )`;
      invoke('describe_vehicle', { vehicleRon: descRon })
        .then(tables => renderPropertyTable(tables))
        .catch(() => {});
    } else {
      polyCount.textContent = '';
      setStatus('No mesh generated', false);
      updateViewportInfo(null);
    }
  } catch (err) {
    await hideEvalProgress();
    setStatus('Error: ' + err, false);
    showIssues([{ severity: 'Error', message: String(err) }]);
  }
}

function updateViewportInfo(result) {
  if (!result || !result.mesh) {
    vpInfo.innerHTML = '';
    return;
  }
  const v = result.mesh.positions.length / 3;
  const f = result.mesh.indices.length / 3;
  const camStr = `cam: ${camera.position.x.toFixed(0)}, ${camera.position.y.toFixed(0)}, ${camera.position.z.toFixed(0)}`;
  vpInfo.innerHTML = `<div>verts: <span class="key">${v}</span> &nbsp; tris: <span class="key">${f}</span></div><div>${camStr}</div>`;
}

// Frame the camera when the model sits far outside the default framing, so
// tiny (sub-mm) or huge designs are visible without manual zoom-extents.
// Normal rockets (~100–4000 mm) keep the user's current view.
function autoFrameCamera() {
  const box = new THREE.Box3().setFromObject(meshGroup);
  if (box.isEmpty()) return;
  const size = box.getSize(new THREE.Vector3()).length();
  if (size >= 100 && size <= 4000) return;
  const center = box.getCenter(new THREE.Vector3());
  const dist = Math.max(size * 1.4, 20);
  controls.target.copy(center);
  camera.position.set(center.x + dist * 0.6, center.y + dist * 0.5, center.z + dist * 0.8);
  controls.update();
}

// Zoom-extents: always reframe to fit the whole model (regardless of size).
function frameToFit() {
  const box = new THREE.Box3().setFromObject(meshGroup);
  if (box.isEmpty()) return;
  const size = box.getSize(new THREE.Vector3()).length();
  const center = box.getCenter(new THREE.Vector3());
  controls.target.copy(center);
  if (camera === orthoCamera) {
    // Ortho: fit by view height, keep the current direction.
    const dir = camera.position.clone().sub(center).normalize();
    const vh = Math.max(size * 1.15, 100);
    setOrthoFrustum(vh);
    camera.position.copy(center).addScaledVector(dir, size * 3 + 100);
  } else {
    const dist = Math.max(size * 1.4, 100);
    camera.position.set(center.x + dist * 0.6, center.y + dist * 0.5, center.z + dist * 0.8);
  }
  controls.update();
}

// When set, the next evaluate() call reframes the camera to fit the new
// geometry instead of preserving the user's current view.
let agentAutoFit = false;
async function agentEvaluate() {
  agentAutoFit = true;
  await evaluate();
}

// ===== Kind badge =====
function updateKindBadge() {
  const text = editor.value;
  if (text.includes('Vehicle(')) {
    kindBadge.textContent = 'Assembly';
    kindBadge.className = 'kind-badge vehicle';
  } else if (text.includes('Solid([') || text.includes('Extrude(') || text.includes('Revolve(') || text.includes('Loft(')) {
    kindBadge.textContent = 'Solid';
    kindBadge.className = 'kind-badge';
  } else {
    kindBadge.textContent = 'Shorthand';
    kindBadge.className = 'kind-badge shorthand';
  }
}

// ===== Presets =====
const PRESETS = {
  "full-rocket": `Vehicle(
    name: "APRO-1",
    units: Millimeters,
    components: [
        Component(
            name: "NoseCone",
            kind: NoseCone(NoseConeParams(
                profile: VonKarman,
                length: 300.0,
                base_radius: 54.0,
                wall: 2.0,
                material: "Al-6061-T6",
            )),
        ),
        Component(
            name: "BodyTube",
            transform: (position: (0.0, 0.0, 300.0), rotation: (0.0, 0.0, 0.0)),
            kind: BodyTube(BodyTubeParams(
                length: 600.0,
                radius: 54.0,
                wall: 2.0,
                material: "Al-6061-T6",
            )),
        ),
        Component(
            name: "Transition",
            transform: (position: (0.0, 0.0, 900.0), rotation: (0.0, 0.0, 0.0)),
            kind: Transition(TransitionParams(
                length: 150.0,
                start_radius: 54.0,
                end_radius: 70.0,
                wall: 2.0,
                material: "Al-6061-T6",
            )),
        ),
        Component(
            name: "Tank",
            transform: (position: (0.0, 0.0, 1050.0), rotation: (0.0, 0.0, 0.0)),
            kind: Tank(TankParams(
                radius: 70.0,
                cylindrical_length: 400.0,
                dome: Ellipsoidal(ratio: 2.0),
                wall: 3.0,
                material: "Al-6061-T6",
            )),
        ),
        Component(
            name: "Nozzle",
            transform: (position: (0.0, 0.0, 1520.0), rotation: (0.0, 0.0, 0.0)),
            kind: Nozzle(NozzleParams(
                kind: Bell,
                throat_radius: 20.0,
                expansion_ratio: 16.0,
                percent_bell: 85.0,
                chamber_radius: 54.0,
                wall: 3.0,
                material: "Inconel-718",
            )),
        ),
        Component(
            name: "Fins",
            transform: (position: (0.0, 0.0, 800.0), rotation: (0.0, 0.0, 0.0)),
            kind: FinSet(FinSetParams(
                count: 4,
                root_chord: 120.0,
                tip_chord: 60.0,
                span: 60.0,
                sweep: 15.0,
                airfoil: AirfoilParams(family: NACA(digits: "0012")),
                thickness: 3.0,
                material: "Al-6061-T6",
            )),
        ),
    ],
)`,
  bodytube: `Component(
    name: "BodyTube-1",
    kind: BodyTube(BodyTubeParams(
        length: 500.0,
        radius: 54.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  transition: `Component(
    name: "Transition-1",
    kind: Transition(TransitionParams(
        length: 200.0,
        start_radius: 54.0,
        end_radius: 70.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  "tank-hemi": `Component(
    name: "Tank-1",
    kind: Tank(TankParams(
        radius: 54.0,
        cylindrical_length: 600.0,
        dome: Hemispherical,
        wall: 3.0,
        material: "Al-6061-T6",
    )),
)`,
  "tank-ellip": `Component(
    name: "Tank-2",
    kind: Tank(TankParams(
        radius: 54.0,
        cylindrical_length: 600.0,
        dome: Ellipsoidal(ratio: 2.0),
        wall: 3.0,
        material: "Al-6061-T6",
    )),
)`,
  "fins-3": `Component(
    name: "FinSet-1",
    transform: (position: (0.0, 0.0, 400.0), rotation: (0.0, 0.0, 0.0)),
    kind: FinSet(FinSetParams(
        count: 3,
        root_chord: 150.0,
        tip_chord: 80.0,
        span: 70.0,
        sweep: 25.0,
        airfoil: AirfoilParams(family: NACA(digits: "0012")),
        thickness: 3.0,
        material: "Al-6061-T6",
    )),
)`,
  "fins-4": `Component(
    name: "FinSet-2",
    transform: (position: (0.0, 0.0, 400.0), rotation: (0.0, 0.0, 0.0)),
    kind: FinSet(FinSetParams(
        count: 4,
        root_chord: 120.0,
        tip_chord: 60.0,
        span: 60.0,
        sweep: 15.0,
        airfoil: AirfoilParams(family: NACA(digits: "0012")),
        thickness: 3.0,
        material: "Al-6061-T6",
    )),
)`,
  "nozzle-conical": `Component(
    name: "Nozzle-1",
    kind: Nozzle(NozzleParams(
        kind: Conical,
        throat_radius: 20.0,
        expansion_ratio: 9.0,
        percent_bell: 100.0,
        chamber_radius: 60.0,
        wall: 3.0,
        material: "Inconel-718",
    )),
)`,
  "nozzle-bell": `Component(
    name: "Nozzle-2",
    kind: Nozzle(NozzleParams(
        kind: Bell,
        throat_radius: 20.0,
        expansion_ratio: 16.0,
        percent_bell: 85.0,
        chamber_radius: 60.0,
        wall: 3.0,
        material: "Inconel-718",
    )),
)`,
  conical: `Component(
    name: "NoseCone-1",
    kind: NoseCone(NoseConeParams(
        profile: Conical,
        length: 300.0,
        base_radius: 54.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  ogive: `Component(
    name: "NoseCone-1",
    kind: NoseCone(NoseConeParams(
        profile: Ogive,
        length: 300.0,
        base_radius: 54.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  power: `Component(
    name: "NoseCone-1",
    kind: NoseCone(NoseConeParams(
        profile: Power(n: 0.75),
        length: 300.0,
        base_radius: 54.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  parabolic: `Component(
    name: "NoseCone-1",
    kind: NoseCone(NoseConeParams(
        profile: Parabolic(k: 0.5),
        length: 300.0,
        base_radius: 54.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  haack: `Component(
    name: "NoseCone-1",
    kind: NoseCone(NoseConeParams(
        profile: Haack(c: 0.333),
        length: 300.0,
        base_radius: 54.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  vonkarman: `Component(
    name: "NoseCone-1",
    kind: NoseCone(NoseConeParams(
        profile: VonKarman,
        length: 300.0,
        base_radius: 54.0,
        wall: 2.0,
        material: "Al-6061-T6",
    )),
)`,
  "lre-demo": `Vehicle(
    name: "APRO-LRE",
    units: Millimeters,
    components: [
        Component(
            name: "GimbalRing",
            kind: Solid([
                Revolve(profile: Points([(0.0,55.0),(0.0,85.0),(10.0,85.0),(10.0,55.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "GimbalLug_0",
            transform: (position: (0.0,0.0,0.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Extrude(profile: Points([(80.0,-8.0),(95.0,-8.0),(95.0,8.0),(80.0,8.0)]), height: 6.0, direction: None, taper: None),
            ]),
        ),
        Component(
            name: "GimbalLug_90",
            transform: (position: (0.0,0.0,0.0), rotation: (0.0,0.0,90.0)),
            kind: Solid([
                Extrude(profile: Points([(80.0,-8.0),(95.0,-8.0),(95.0,8.0),(80.0,8.0)]), height: 6.0, direction: None, taper: None),
            ]),
        ),
        Component(
            name: "GimbalLug_180",
            transform: (position: (0.0,0.0,0.0), rotation: (0.0,0.0,180.0)),
            kind: Solid([
                Extrude(profile: Points([(80.0,-8.0),(95.0,-8.0),(95.0,8.0),(80.0,8.0)]), height: 6.0, direction: None, taper: None),
            ]),
        ),
        Component(
            name: "GimbalLug_270",
            transform: (position: (0.0,0.0,0.0), rotation: (0.0,0.0,270.0)),
            kind: Solid([
                Extrude(profile: Points([(80.0,-8.0),(95.0,-8.0),(95.0,8.0),(80.0,8.0)]), height: 6.0, direction: None, taper: None),
            ]),
        ),
        Component(
            name: "FuelValve",
            transform: (position: (70.0,0.0,15.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Revolve(profile: Points([(0.0,0.0),(0.0,18.0),(25.0,18.0),(25.0,24.0),(30.0,24.0),(45.0,10.0),(60.0,10.0),(60.0,0.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "OxValve",
            transform: (position: (-70.0,0.0,15.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Revolve(profile: Points([(0.0,0.0),(0.0,18.0),(25.0,18.0),(25.0,24.0),(30.0,24.0),(45.0,10.0),(60.0,10.0),(60.0,0.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "FuelFeedLine",
            transform: (position: (70.0,0.0,75.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Sweep(profile: Points([(-4.0,-4.0),(4.0,-4.0),(4.0,4.0),(-4.0,4.0)]), path: Line(start: (0.0,0.0,0.0), end: (0.0,0.0,17.0)), twist: None),
            ]),
        ),
        Component(
            name: "OxFeedLine",
            transform: (position: (-70.0,0.0,75.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Sweep(profile: Points([(-4.0,-4.0),(4.0,-4.0),(4.0,4.0),(-4.0,4.0)]), path: Line(start: (0.0,0.0,0.0), end: (0.0,0.0,17.0)), twist: None),
            ]),
        ),
        Component(
            name: "FuelManifold",
            transform: (position: (0.0,0.0,80.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Revolve(profile: Points([(0.0,60.0),(0.0,72.0),(12.0,72.0),(12.0,60.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "OxManifold",
            transform: (position: (0.0,0.0,80.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Revolve(profile: Points([(0.0,78.0),(0.0,90.0),(12.0,90.0),(12.0,78.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "InjectorDome",
            transform: (position: (0.0,0.0,100.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Revolve(profile: Points([(0.0,0.0),(8.0,60.0),(20.0,95.0),(28.0,95.0),(28.0,0.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "CombustionChamber",
            transform: (position: (0.0,0.0,128.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Revolve(profile: Points([(0.0,90.0),(150.0,90.0),(150.0,95.0),(0.0,95.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "CoolingJacket",
            transform: (position: (0.0,0.0,128.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                RevolveChain(segments: [[(0.0,97.0),(150.0,97.0)],[(0.0,109.0),(150.0,109.0)]], angle: 360.0),
            ]),
        ),
        Component(
            name: "IgniterBoss",
            transform: (position: (95.0,0.0,160.0), rotation: (0.0,90.0,0.0)),
            kind: Solid([
                Extrude(profile: Points([(0.0,10.0),(7.0,7.0),(10.0,0.0),(7.0,-7.0),(0.0,-10.0),(-7.0,-7.0),(-10.0,0.0),(-7.0,7.0)]), height: 35.0, direction: None, taper: None),
                TransformOp(translate: Some((0.0,0.0,35.0)), rotate: None, scale: None),
                Extrude(profile: Points([(0.0,14.0),(10.0,10.0),(14.0,0.0),(10.0,-10.0),(0.0,-14.0),(-10.0,-10.0),(-14.0,0.0),(-10.0,10.0)]), height: 6.0, direction: None, taper: None),
            ]),
        ),
        Component(
            name: "ThrustRing",
            transform: (position: (0.0,0.0,278.0), rotation: (0.0,0.0,0.0)),
            kind: Solid([
                Revolve(profile: Points([(0.0,95.0),(0.0,120.0),(15.0,120.0),(15.0,95.0)]), angle: 360.0, axis: Some(Z)),
            ]),
        ),
        Component(
            name: "Nozzle",
            transform: (position: (0.0,0.0,278.0), rotation: (0.0,0.0,0.0)),
            kind: Nozzle(NozzleParams(
                kind: Bell,
                throat_radius: 40.0,
                expansion_ratio: 20.0,
                percent_bell: 80.0,
                chamber_radius: 90.0,
                wall: 4.0,
                material: "Inconel-718",
            )),
        ),
    ],
)`,
  "solid-bracket": `Component(
    name: "Bracket-1",
    kind: Solid([
        Extrude(
            profile: Points([(0.0, 0.0), (80.0, 0.0), (80.0, 60.0), (40.0, 60.0), (40.0, 20.0), (0.0, 20.0)]),
            height: 10.0,
            direction: None,
            taper: None,
        ),
        Extrude(
            profile: Points([(70.0, 10.0), (75.0, 10.0), (75.0, 15.0), (70.0, 15.0)]),
            height: 10.0,
            direction: None,
            taper: None,
        ),
    ]),
)`,
  "solid-extrude": `Component(
    name: "CustomSolid",
    transform: (position: (50.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0)),
    kind: Solid([
        Extrude(
            profile: Points([(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]),
            height: 200.0,
            direction: None,
            taper: None,
        ),
    ]),
)`,
  "solid-revolve": `Component(
    name: "CustomSolid",
    kind: Solid([
        Revolve(
            profile: Points([(0.0, 0.0), (100.0, 50.0), (300.0, 54.0)]),
            angle: 360.0,
            axis: Some(Z),
        ),
    ]),
)`,
  "solid-revolve-chain": `Component(
    name: "OgiveBody",
    kind: Solid([
        RevolveChain(
            segments: [
                [(0.0, 0.0), (100.0, 30.0), (200.0, 54.0), (300.0, 54.0)],
                [(0.0, 54.0), (400.0, 54.0)],
            ],
            angle: 360.0,
        ),
    ]),
)`,
  "solid-loft": `Component(
    name: "CustomSolid",
    kind: Solid([
        Loft(
            profiles: [
                Points([(0.0, 0.0), (50.0, 0.0), (25.0, 40.0)]),
                Points([(0.0, 0.0), (80.0, 0.0), (40.0, 60.0)]),
            ],
            guide_curves: None,
        ),
    ]),
)`,
};

// ===== Properties popup (Feature Manager selection -> 3D view) =====
const KIND_ICONS = { NoseCone: '?', BodyTube: '?', Transition: '?', Tank: '?', Nozzle: '?', FinSet: '?', Solid: '?' };

function hexColorCss(hex) {
  return '#' + hex.toString(16).padStart(6, '0');
}

function showPropsPopup() {
  vpPropsPopup.style.display = 'flex';
  // Inline style (not an attribute): the stylesheet's default stroke would
  // otherwise win and the border would never match the 3D highlight.
  if (vpPropsAntsRect) vpPropsAntsRect.style.stroke = hexColorCss(selectionColorHex);
  return refreshSelectedPopup();
}

// Values always come from the live document so the popup survives patches.
async function refreshSelectedPopup() {
  if (!selectedComponentName || vpPropsPopup.style.display === 'none') return;
  if (rangeDragging) return; // don't yank the slider out from under the pointer
  const name = selectedComponentName;
  const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
  if (!invoke) return;
  try {
    if (name === '__parameters__') {
      const rows = await invoke('describe_parameters', { vehicleRon: editor.value });
      if (selectedComponentName !== name) return;
      vpPropsIcon.textContent = '�';
      vpPropsTitle.textContent = 'Parameters';
      vpPropsKind.textContent = `${(rows || []).length} entries`;
      vpPropsBody.innerHTML = '';
      vpPropsBody.appendChild(buildParamsTable(rows || []));
    } else {
      const tables = await invoke('describe_vehicle', { vehicleRon: wrapRon(editor.value) });
      if (selectedComponentName !== name) return;
      const comp = (tables || []).find(t => t.name === name);
      if (!comp) { deselectComponent(); return; }
      renderComponentPopup(comp);
    }
  } catch {}
}

function buildParamsTable(paramRows) {
  const table = document.createElement('table');
  table.className = 'props-table';
  const tbody = document.createElement('tbody');
  paramRows.forEach(p => {
    const tr = document.createElement('tr');
    const keyTd = document.createElement('td');
    keyTd.textContent = p.name;
    tr.appendChild(keyTd);
    const valTd = document.createElement('td');
    valTd.textContent = p.computed != null && p.value !== String(p.computed) ? `${p.value}  (${p.computed})` : p.value;
    valTd.setAttribute('data-editable', '');
    valTd.setAttribute('data-param-name', p.name);
    tr.appendChild(valTd);
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);
  return table;
}

// Numeric property editing. Keys with known hard bounds render as progress
// bars (slider); all other numbers get a −/+ scrubber (drag horizontally to
// change, cursor shows ew-resize). Double-click still allows exact entry via
// the shared td[data-editable] input path.
const NUM_RANGES = {
  wall: [0.5, 10, 0.1],
  thickness: [0.5, 10, 0.1],
  angle: [0, 360, 1],
  count: [3, 12, 1],
  percent_bell: [0, 100, 1],
  c: [0, 1, 0.01],
  n: [0, 1, 0.01],
  k: [0, 1, 0.01],
  ratio: [1, 5, 0.05],
};

function numStepFor(value, isInt) {
  if (isInt) return 1;
  const v = Math.abs(value);
  if (v >= 1000) return 10;
  if (v >= 100) return 1;
  if (v >= 10) return 0.5;
  if (v >= 1) return 0.1;
  return 0.01;
}

function fmtNum(v, isInt) {
  return isInt ? String(Math.round(v)) : String(Math.round(v * 1000) / 1000);
}

function applyNumberPatch(compName, key, value) {
  const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
  if (!invoke) return Promise.resolve();
  const ron = editor.value;
  const wasComp = isComponentRon(ron);
  return invoke('apply_patch_vehicle', {
    vehicleRon: wrapRon(ron),
    patch: { SetProperty: { component_name: compName, key, value: String(value) } },
  }).then(result => {
    if (result.success && result.vehicle_ron) {
      editor.value = unwrapRon(result.vehicle_ron, wasComp);
      evaluate();
    } else {
      statusText.textContent = 'Patch rejected: ' + ((result.issues || []).map(i => i.message).join('; ') || 'validation failed');
      refreshSelectedPopup();
    }
  }).catch(err => {
    statusText.textContent = 'Patch error: ' + err;
    refreshSelectedPopup();
  });
}

// While a range thumb is held down, the popup must NOT rebuild itself
// (evaluate() would replace the element under the pointer mid-drag).
let rangeDragging = false;

function buildRangeCell(compName, key, value, min, max, step, isInt) {
  const cell = document.createElement('div');
  cell.className = 'num-cell';
  const cur = Math.min(max, Math.max(min, parseFloat(value)));
  const slider = document.createElement('input');
  slider.type = 'range';
  slider.className = 'num-range';
  slider.min = min; slider.max = max; slider.step = step;
  slider.value = isNaN(cur) ? min : cur;
  const label = document.createElement('span');
  label.className = 'num-val';
  label.textContent = fmtNum(parseFloat(slider.value), isInt);
  const paint = () => {
    const pct = ((parseFloat(slider.value) - min) / (max - min)) * 100;
    slider.style.background = `linear-gradient(90deg, var(--teal) ${pct}%, rgba(255,255,255,0.08) ${pct}%)`;
  };
  paint();

  // LIVE updates: patches are throttled (one in flight at a time, latest
  // value always wins) and serialized so a slow response can never apply a
  // stale base over a newer edit. The final exact value flushes on release.
  let chain = Promise.resolve();
  let timer = null;
  let lastCommitAt = 0;
  let lastQueued = NaN;

  const commit = () => {
    const v = parseFloat(slider.value);
    if (Number.isNaN(v) || v === lastQueued) return;
    lastQueued = v;
    lastCommitAt = performance.now();
    chain = chain.then(() => applyNumberPatch(compName, key, v)).catch(() => {});
  };

  slider.addEventListener('input', () => {
    label.textContent = fmtNum(parseFloat(slider.value), isInt);
    paint();
    if (!rangeDragging) rangeDragging = true;
    const since = performance.now() - lastCommitAt;
    if (since >= 140 && !timer) {
      commit(); // leading edge: first movement applies instantly
    } else {
      clearTimeout(timer);
      timer = setTimeout(() => { timer = null; commit(); }, 140 - Math.min(since, 139));
    }
  });

  const endDrag = () => {
    if (timer) { clearTimeout(timer); timer = null; }
    if (chain) {
      chain.then(() => { rangeDragging = false; });
    }
    commit(); // final exact value on release
  };
  slider.addEventListener('change', endDrag);

  slider.addEventListener('pointerdown', () => { rangeDragging = true; });
  slider.addEventListener('pointerup', endDrag);
  slider.addEventListener('lostpointercapture', endDrag);

  cell.appendChild(slider);
  cell.appendChild(label);
  return cell;
}

function buildScrubCell(compName, key, value, isInt) {
  const cell = document.createElement('div');
  cell.className = 'num-cell';
  let cur = parseFloat(value);
  if (isNaN(cur)) cur = 0;

  const mkBtn = txt => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'num-btn';
    b.textContent = txt;
    return b;
  };
  const minus = mkBtn('−');
  const plus = mkBtn('+');

  const val = document.createElement('span');
  val.className = 'num-val num-scrub';
  val.title = 'Drag left/right to change — double-click to type an exact value';
  val.textContent = fmtNum(cur, isInt);

  const commit = () => applyNumberPatch(compName, key, isInt ? Math.round(cur) : parseFloat(cur.toFixed(4)));

  const bump = dir => {
    const s = numStepFor(cur, isInt);
    let nv = cur + dir * s;
    nv = isInt ? Math.round(nv) : parseFloat(nv.toFixed(4));
    if (nv === cur) return;
    cur = nv;
    val.textContent = fmtNum(cur, isInt);
    commit();
  };
  minus.addEventListener('click', () => bump(-1));
  plus.addEventListener('click', () => bump(1));

  // Horizontal drag-scrub: live preview while dragging, one patch on release.
  let dragging = false, startX = 0, startV = 0, moved = false;
  val.addEventListener('pointerdown', e => {
    dragging = true; moved = false;
    startX = e.clientX; startV = cur;
    val.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  val.addEventListener('pointermove', e => {
    if (!dragging) return;
    const dx = e.clientX - startX;
    if (Math.abs(dx) > 2) moved = true;
    const s = numStepFor(startV, isInt);
    let nv = startV + dx * s * 0.25;
    nv = isInt ? Math.round(nv) : parseFloat(nv.toFixed(4));
    if (nv !== cur) { cur = nv; val.textContent = fmtNum(cur, isInt); }
  });
  val.addEventListener('pointerup', () => {
    if (!dragging) return;
    dragging = false;
    if (moved) commit();
  });

  cell.appendChild(minus);
  cell.appendChild(val);
  cell.appendChild(plus);
  return cell;
}

function buildPropsTable(comp) {
  const table = document.createElement('table');
  table.className = 'props-table';
  const tbody = document.createElement('tbody');
  // Color row first: explicit display color ("auto" = derived from material).
  const allRows = [
    { key: 'color', value: comp.color || 'auto', field_type: 'string' },
    ...comp.rows,
  ];
  allRows.forEach(row => {
    const tr = document.createElement('tr');
    const keyTd = document.createElement('td');
    keyTd.textContent = row.key;
    tr.appendChild(keyTd);
    const valTd = document.createElement('td');
    valTd.setAttribute('data-editable', '');
    valTd.setAttribute('data-comp-name', comp.name);
    valTd.setAttribute('data-key', row.key);
    valTd.setAttribute('data-field-type', row.field_type);

    const isNumeric = row.field_type === 'float' || row.field_type === 'int';
    const range = NUM_RANGES[row.key];
    if (isNumeric && range) {
      // Bounded number: progress-bar slider.
      valTd.appendChild(buildRangeCell(comp.name, row.key, row.value, range[0], range[1], range[2], row.field_type === 'int'));
    } else if (isNumeric) {
      // Unbounded number: ± scrubber with horizontal drag.
      valTd.appendChild(buildScrubCell(comp.name, row.key, row.value, row.field_type === 'int'));
    } else {
      valTd.textContent = row.value;
    }
    tr.appendChild(valTd);
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);
  return table;
}

function renderComponentPopup(comp) {
  vpPropsIcon.textContent = KIND_ICONS[comp.kind] || '?';
  vpPropsTitle.textContent = comp.name;
  vpPropsKind.textContent = comp.kind;
  vpPropsBody.innerHTML = '';
  vpPropsBody.appendChild(buildPropsTable(comp));
  if (comp.kind === 'Solid') vpPropsBody.appendChild(buildSolidToolbar(comp));
  const actions = document.createElement('div');
  actions.className = 'op-toolbar';
  const saveBtn = document.createElement('button');
  saveBtn.className = 'op-btn';
  saveBtn.textContent = 'Save to library';
  saveBtn.title = 'Save this component to the library';
  saveBtn.addEventListener('click', () => saveToLibrary(comp.name));
  actions.appendChild(saveBtn);
  vpPropsBody.appendChild(actions);
}

function buildSolidToolbar(comp) {
  const toolbar = document.createElement('div');
  toolbar.className = 'op-toolbar';
  const opCount = comp.rows.find(r => r.key === 'op_count');
  const n = opCount ? parseInt(opCount.value) : 0;

  const addSelect = document.createElement('select');
  const defaultOp = document.createElement('option');
  defaultOp.textContent = '+ Add Op...';
  defaultOp.disabled = true;
  defaultOp.selected = true;
  addSelect.appendChild(defaultOp);
  ['Revolve','RevolveChain','Extrude','Loft','Transform'].forEach(t => {
    const opt = document.createElement('option');
    opt.textContent = t;
    addSelect.appendChild(opt);
  });
  addSelect.addEventListener('change', () => {
    const opType = addSelect.value;
    addSelect.value = '+ Add Op...';
    if (!opType || opType === '+ Add Op...') return;
    const defaultOps = {
      Revolve: 'Revolve(profile:Points([(0.0,0.0),(100.0,0.0),(100.0,50.0),(0.0,50.0)]),angle:360.0,axis:Some(Z))',
      RevolveChain: 'RevolveChain(segments:[[(0.0,0.0),(200.0,50.0),(200.0,50.0)],[(0.0,50.0),(300.0,50.0)]],angle:360.0)',
      Extrude: 'Extrude(profile:Points([(0.0,0.0),(100.0,0.0),(100.0,50.0),(0.0,50.0)]),height:100.0,direction:None,taper:None)',
      Loft: 'Loft(profiles:[Points([(0.0,0.0),(50.0,0.0),(25.0,40.0)]),Points([(0.0,0.0),(80.0,0.0),(40.0,60.0)])],guide_curves:None)',
      Transform: 'TransformOp(translate:Some((0.0,0.0,0.0)),rotate:None,scale:None)',
    }[opType];
    const ron = editor.value;
    const match = ron.match(/Solid\((\s*\[[\s\S]*?)\]/);
    if (match) {
      const existing = match[1];
      const insertAt = existing.lastIndexOf(')');
      if (insertAt > 0) {
        const before = existing.slice(0, insertAt + 1);
        const after = existing.slice(insertAt + 1);
        const sep = after.trim().endsWith(',') || before.endsWith(',') ? '' : ',';
        editor.value = ron.replace(match[1], before + sep + '\n        ' + defaultOps + after);
        evaluate();
      }
    }
  });
  toolbar.appendChild(addSelect);

  if (n > 0) {
    const rmBtn = document.createElement('button');
    rmBtn.className = 'op-btn danger';
    rmBtn.textContent = `Remove op ${n-1}`;
    rmBtn.addEventListener('click', () => {
      const ron = editor.value;
      const solidStart = ron.lastIndexOf('Solid([');
      if (solidStart < 0) return;
      let depth = 0, end = -1;
      for (let i = solidStart + 7; i < ron.length; i++) {
        if (ron[i] === '[') depth++;
        else if (ron[i] === ']') { if (depth === 0) { end = i; break; } depth--; }
      }
      if (end < 0) return;
      const inner = ron.slice(solidStart + 7, end);
      const ops = [];
      let parenDepth = 0, current = '';
      for (const ch of inner) {
        if (ch === '(') parenDepth++;
        else if (ch === ')') parenDepth--;
        if (ch === ',' && parenDepth === 0) { ops.push(current.trim()); current = ''; }
        else { current += ch; }
      }
      if (current.trim()) ops.push(current.trim());
      if (ops.length === 0) return;
      ops.pop();
      editor.value = ron.slice(0, solidStart + 7) + ops.join(',\n        ') + ron.slice(end);
      evaluate();
    });
    toolbar.appendChild(rmBtn);
  }
  return toolbar;
}

// Called after every evaluate(): keep an open popup's values up to date.
function renderPropertyTable(tables) {
  if (!selectedComponentName || selectedComponentName === '__parameters__') return;
  if (rangeDragging) return; // never rebuild the popup under an active slider
  const comp = (tables || []).find(t => t.name === selectedComponentName);
  if (!comp) { deselectComponent(); return; }
  renderComponentPopup(comp);
}
// ===== Component library =====
async function saveToLibrary(componentName) {
  const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
  if (!invoke) {
    showToast('Library requires the desktop app');
    return;
  }
  try {
    const id = await invoke('library_save', {
      vehicleRon: editor.value,
      componentName,
      description: '',
      tags: [],
    });
    showToast(`Saved "${componentName}" to library (${id.slice(0, 8)}…)`);
  } catch (e) {
    showToast('Library save failed: ' + String(e));
  }
}

async function libraryBumpUse(ids) {
  if (!ids || ids.length === 0) return;
  const invoke = tauriInvoke();
  if (!invoke) return;
  try { await invoke('library_bump_use', { ids }); } catch {}
}

// ===== Library manager window =====
let libraryEntries = [];
let libEditingId = null;
let pendingLibDeleteId = null;

const LIB_KIND_ICONS = {
  NoseCone: '△', BodyTube: '▯', Transition: '◮', Tank: '●',
  Nozzle: '▽', FinSet: '▲', Solid: '▧', Assembly: '🗂',
};

// ===== Rotating 3D thumbnails =====
// ONE shared offscreen WebGLRenderer renders each part's scene per frame and
// blits onto the card's 2D canvas — avoids one WebGL context per card.
const LIB_THUMB_W = 320;
const LIB_THUMB_H = 200;
let libRenderer = null;
let libCamera = null;
let libLoopRaf = null;
const libGeoCache = new Map(); // entry id -> {scene, group} | 'failed' (thumbnails)
const libGeomCache = new Map(); // entry id -> {geom, maxDim} | 'failed' (shared geometry)

function ensureLibRenderer() {
  if (libRenderer) return true;
  try {
    // preserveDrawingBuffer keeps the frame valid for the 2D-canvas blit
    // (some WebView2/GPU combos clear it before drawImage otherwise).
    libRenderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, preserveDrawingBuffer: true });
    libRenderer.setSize(LIB_THUMB_W, LIB_THUMB_H, false);
    libRenderer.setClearColor(0x000000, 0);
    libCamera = new THREE.PerspectiveCamera(38, LIB_THUMB_W / LIB_THUMB_H, 0.01, 100);
    libCamera.position.set(0, 1.15, 3.7);
    libCamera.lookAt(0, 0, 0);
    return true;
  } catch (e) {
    console.error('[library] thumbnail renderer failed:', e);
    libRenderer = null;
    return false;
  }
}

function libEntryColor(entry) {
  // A color stored inside the saved RON wins; else derive from material/name.
  const m = entry.ron.match(/color:\s*Some\("([^"]+)"\)/);
  if (m) {
    const c = parseColorString(m[1]);
    if (c != null) return c;
  }
  return componentColor({ material: (entry.params && entry.params.material) || '', name: entry.name });
}

async function ensureLibGeometry(entry) {
  const cached = libGeomCache.get(entry.id);
  if (cached) return cached === 'failed' ? null : cached;
  const invoke = tauriInvoke();
  if (!invoke) return null;
  try {
    const res = await invoke('evaluate', { componentRon: entry.ron });
    if (!res.mesh || !res.mesh.positions || res.mesh.positions.length === 0) {
      throw new Error('no mesh');
    }
    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.BufferAttribute(new Float32Array(res.mesh.positions), 3));
    geom.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(res.mesh.normals), 3));
    geom.setIndex(new THREE.BufferAttribute(new Uint32Array(res.mesh.indices), 1));
    geom.computeBoundingBox();
    const center = geom.boundingBox.getCenter(new THREE.Vector3());
    const sz = geom.boundingBox.getSize(new THREE.Vector3());
    const maxDim = Math.max(sz.x, sz.y, sz.z) || 1;
    geom.translate(-center.x, -center.y, -center.z);
    const info = { geom, maxDim };
    libGeomCache.set(entry.id, info);
    return info;
  } catch (e) {
    console.error('[library] preview evaluate failed for', entry.name, e);
    libGeomCache.set(entry.id, 'failed');
    return null;
  }
}

async function ensureLibScene(entry) {
  const g = await ensureLibGeometry(entry);
  if (!g) return null;
  const cachedScene = libGeoCache.get(entry.id);
  if (cachedScene && cachedScene !== 'failed') return cachedScene;

  const mesh = new THREE.Mesh(g.geom, new THREE.MeshPhysicalMaterial({
    color: libEntryColor(entry),
    metalness: 0.15,
    roughness: 0.45,
    clearcoat: 0.2,
    clearcoatRoughness: 0.4,
    side: THREE.DoubleSide,
  }));
  mesh.scale.setScalar(2.4 / g.maxDim);
  const group = new THREE.Group();
  group.rotation.x = 0.32;
  group.add(mesh);

  const scene = new THREE.Scene();
  scene.add(new THREE.HemisphereLight(0xffffff, 0x30354a, 0.95));
  const dir = new THREE.DirectionalLight(0xffffff, 1.1);
  dir.position.set(3, 5, 4);
  scene.add(dir);
  scene.add(group);

  const info = { scene, group };
  libGeoCache.set(entry.id, info);
  return info;
}

function startLibLoop() {
  stopLibLoop();
  if (!ensureLibRenderer()) return;
  const grid = document.getElementById('lib-grid');
  const tick = () => {
    libLoopRaf = requestAnimationFrame(tick);
    if (!grid || !grid.isConnected) return;
    grid.querySelectorAll('canvas.lib-thumb[data-entry-id]').forEach(cv => {
      const ctx = cv.getContext('2d');
      if (!ctx) return;
      const info = libGeoCache.get(cv.dataset.entryId);
      if (!info || info === 'failed') return;
      info.group.rotation.y += 0.018; // turntable
      libRenderer.render(info.scene, libCamera);
      ctx.clearRect(0, 0, cv.width, cv.height);
      ctx.drawImage(libRenderer.domElement, 0, 0, cv.width, cv.height);
    });
  };
  libLoopRaf = requestAnimationFrame(tick);
}

function stopLibLoop() {
  if (libLoopRaf) cancelAnimationFrame(libLoopRaf);
  libLoopRaf = null;
}

function setLibView(view) {
  const grid = document.getElementById('lib-grid');
  const det = document.getElementById('lib-detail');
  const nw = document.getElementById('lib-new');
  const modal = document.getElementById('library-modal');
  const modalOpen = !!(modal && modal.style.display !== 'none');
  grid.style.display = view === 'grid' ? '' : 'none';
  det.style.display = view === 'detail' ? 'flex' : 'none';
  nw.style.display = view === 'new' ? 'flex' : 'none';
  stopLibLoop();
  stopLibDetailLoop();
  stopLnLoop();
  if (view !== 'detail' && libDetailControls) { libDetailControls.dispose(); libDetailControls = null; }
  if (view !== 'new' && lnControls) { lnControls.dispose(); lnControls = null; }
  if (!modalOpen) return;
  if (view === 'grid') startLibLoop();
  else if (view === 'detail') startLibDetailLoop();
  else if (view === 'new') startLnLoop();
}

function openLibrary() {
  const modal = document.getElementById('library-modal');
  if (!modal) return;
  modal.style.display = 'flex';
  refreshLibrary();
  setLibView('grid');
}

function closeLibrary() {
  const modal = document.getElementById('library-modal');
  if (modal) modal.style.display = 'none';
  libEditingId = null;
  // Return to the grid state and halt every library render loop.
  setLibView('grid');
}

async function refreshLibrary() {
  const invoke = tauriInvoke();
  if (!invoke) return;
  try {
    libraryEntries = await invoke('library_list');
  } catch (e) {
    libraryEntries = [];
    statusText.textContent = 'Library load failed: ' + e;
  }
  renderLibraryGrid();
}

function libSourceBadge(source) {
  if (source === 'Builtin') return ['builtin', 'Builtin'];
  if (source === 'AIGenerated') return ['ai', 'AI'];
  return ['user', 'Saved'];
}

function libChips(entry) {
  const p = entry.params || {};
  const out = [];
  const push = (label, v, unit) => { if (v != null && v !== '') out.push([label, v, unit]); };
  push('OD', p.od_mm, 'mm');
  push('ID', p.id_mm, 'mm');
  push('L', p.length_mm, 'mm');
  push('throat', p.throat_mm, 'mm');
  push('ε', p.expansion_ratio, '');
  push('', p.mass_g != null ? (Math.round(p.mass_g * 1000) / 1000) : null, 'g');
  if (p.material) out.push(['', p.material, '']);
  return out;
}

function escapeHtml(s) {
  return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function renderLibraryGrid() {
  const grid = document.getElementById('lib-grid');
  const countEl = document.getElementById('lib-count');
  if (!grid) return;
  const q = (document.getElementById('lib-search')?.value || '').trim().toLowerCase();
  const kind = document.getElementById('lib-kind-filter')?.value || '';

  const filtered = libraryEntries.filter(e => {
    if (kind && e.kind !== kind) return false;
    if (!q) return true;
    const hay = [e.name, e.description, e.kind, ...(e.tags || [])].join(' ').toLowerCase();
    return hay.includes(q);
  });

  if (countEl) countEl.textContent =
    `${filtered.length} of ${libraryEntries.length} part${libraryEntries.length === 1 ? '' : 's'}`;

  grid.innerHTML = '';
  if (filtered.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'lib-empty';
    empty.innerHTML = libraryEntries.length === 0
      ? 'The library is empty.<br>Select a component in the Manager and press <b>Save</b>, or click <b>Seed builtins</b>.'
      : 'No parts match your search.';
    grid.appendChild(empty);
    return;
  }

  filtered.forEach(entry => {
    const card = document.createElement('div');
    card.className = 'lib-card-el';

    // Rotating 3D preview
    const wrap = document.createElement('div');
    wrap.className = 'lib-el-canvas loading';
    const cv = document.createElement('canvas');
    cv.className = 'lib-thumb';
    cv.width = LIB_THUMB_W;
    cv.height = LIB_THUMB_H;
    cv.dataset.entryId = entry.id;
    wrap.appendChild(cv);
    card.appendChild(wrap);
    ensureLibScene(entry).then(info => {
      wrap.classList.remove('loading');
      if (!info) {
        const ph = document.createElement('div');
        ph.className = 'lib-nopreview';
        ph.textContent = LIB_KIND_ICONS[entry.kind] || '▫';
        ph.title = 'No preview available for this part';
        wrap.appendChild(ph);
      }
    });

    // Name + built-in badge (the always-visible identity row)
    const bar = document.createElement('div');
    bar.className = 'lib-el-namebar';
    const icon = document.createElement('span');
    icon.className = 'lib-el-icon';
    icon.textContent = LIB_KIND_ICONS[entry.kind] || '▫';
    const name = document.createElement('span');
    name.className = 'lib-el-name';
    name.textContent = entry.name;
    name.title = entry.name;
    const [badgeCls, badgeTxt] = libSourceBadge(entry.source);
    const badge = document.createElement('span');
    badge.className = 'lib-badge ' + badgeCls;
    badge.textContent = badgeTxt;
    bar.append(icon, name, badge);
    card.appendChild(bar);

    // Click anywhere on the card opens the full detail screen.
    card.addEventListener('click', e => {
      if (e.target.closest('button')) return;
      openLibDetail(entry);
    });

    grid.appendChild(card);
  });

  filtered.forEach(e => ensureLibScene(e));
}

function uniqueComponentName(base) {
  const docNames = new Set(
    [...getEditorRon().matchAll(/Component\(\s*name:\s*"([^"]+)"/g)].map(m => m[1])
  );
  if (!docNames.has(base)) return base;
  let i = 2;
  while (docNames.has(`${base}_${i}`)) i++;
  return `${base}_${i}`;
}

async function insertLibraryEntry(entry) {
  const invoke = tauriInvoke();
  if (!invoke) { showToast('Library requires the desktop app'); return; }
  const name = uniqueComponentName(entry.name);
  const compRon = name === entry.name
    ? entry.ron
    : entry.ron.replace(/name:\s*"([^"]*)"/, `name: "${name}"`);
  const after = selectedComponentName && selectedComponentName !== '__parameters__'
    ? `Some("${selectedComponentName}")`
    : 'None';
  const patchRon = `AddComponent(after_component: ${after}, component: ${compRon})`;
  try {
    const result = await invoke('apply_patch_ron', {
      vehicleRon: getEditorRon(),
      patchRon,
    });
    if (result.success && result.vehicle_ron) {
      const wasComp = isComponentRon(getEditorRon());
      editor.value = unwrapRon(result.vehicle_ron, wasComp);
      evaluate();
      libraryBumpUse([entry.id]);
      closeLibrary();
      showToast(`Inserted "${name}" from library`);
    } else {
      const msg = (result.issues || []).map(i => i.message).join('; ');
      showToast('Insert rejected: ' + (msg || 'validation failed'));
    }
  } catch (e) {
    showToast('Insert failed: ' + e);
  }
}

// ===== Library detail screen (in-panel, orbit-controlled 3D) =====
let currentLibEntryId = null;
let libDetailRaf = null;
let libDetailRenderer = null;
let libDetailCamera = null;
let libDetailControls = null;
let libDetailScene = null;

function ldSection(title) {
  const el = document.createElement('div');
  el.className = 'ld-section-title';
  el.textContent = title;
  return el;
}

function openLibDetail(entry) {
  currentLibEntryId = entry.id;
  setLibView('detail');
  const iconEl = document.getElementById('ld-icon');
  if (iconEl) iconEl.textContent = LIB_KIND_ICONS[entry.kind] || '▫';
  const nameEl = document.getElementById('ld-name');
  if (nameEl) nameEl.textContent = entry.name;
  const badgeEl = document.getElementById('ld-badge');
  if (badgeEl) {
    const [cls, txt] = libSourceBadge(entry.source);
    badgeEl.className = 'lib-badge ' + cls;
    badgeEl.textContent = txt;
  }
  renderLibDetailSide(entry);
  initLibDetail3D(entry).catch(e => console.error('[library] detail 3D failed:', e));
}

function closeLibDetail() {
  const det = document.getElementById('lib-detail');
  if (!det || det.style.display === 'none') return;
  currentLibEntryId = null;
  setLibView('grid');
}

async function initLibDetail3D(entry) {
  const g = await ensureLibGeometry(entry);
  if (!g || currentLibEntryId !== entry.id) return;

  const canvas = document.getElementById('ld-canvas');
  if (!canvas) return;

  try {
    if (!libDetailRenderer) {
      libDetailRenderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
      libDetailRenderer.setClearColor(0x000000, 0);
    }
  } catch (e) {
    console.error('[library] detail renderer failed:', e);
    const hint = document.querySelector('.ld-hint');
    if (hint) hint.textContent = 'WebGL unavailable — 3D preview disabled';
    return;
  }

  // Dedicated scene per opening; geometry comes from the shared cache.
  libDetailScene = new THREE.Scene();
  libDetailScene.add(new THREE.HemisphereLight(0xffffff, 0x30354a, 0.95));
  const dir = new THREE.DirectionalLight(0xffffff, 1.15);
  dir.position.set(4, 6, 5);
  libDetailScene.add(dir);
  const rim = new THREE.DirectionalLight(0x88aaff, 0.45);
  rim.position.set(-4, -2, -4);
  libDetailScene.add(rim);

  const mesh = new THREE.Mesh(g.geom, new THREE.MeshPhysicalMaterial({
    color: libEntryColor(entry),
    metalness: 0.25,
    roughness: 0.35,
    clearcoat: 0.3,
    clearcoatRoughness: 0.35,
    side: THREE.DoubleSide,
  }));
  mesh.scale.setScalar(2.8 / g.maxDim);
  libDetailScene.add(mesh);

  const vp = document.getElementById('ld-viewport');
  const w = Math.max(1, vp.clientWidth);
  const h = Math.max(1, vp.clientHeight);
  libDetailRenderer.setSize(w, h, false);
  if (!libDetailCamera) {
    libDetailCamera = new THREE.PerspectiveCamera(40, w / h, 0.01, 500);
  }
  libDetailCamera.aspect = w / h;
  libDetailCamera.position.set(2.6, 1.9, 3.2);
  libDetailCamera.lookAt(0, 0, 0);
  libDetailCamera.updateProjectionMatrix();

  if (libDetailControls) { libDetailControls.dispose(); }
  libDetailControls = new OrbitControls(libDetailCamera, canvas);
  libDetailControls.enableDamping = true;
  libDetailControls.dampingFactor = 0.08;
  libDetailControls.autoRotate = true; // gentle spin until the user grabs it
  libDetailControls.autoRotateSpeed = 2.2;
  libDetailControls.addEventListener('start', () => {
    if (libDetailControls) libDetailControls.autoRotate = false;
  });
}

function startLibDetailLoop() {
  if (libDetailRaf) cancelAnimationFrame(libDetailRaf);
  const vp = document.getElementById('ld-viewport');
  let lastW = 0, lastH = 0;
  const tick = () => {
    libDetailRaf = requestAnimationFrame(tick);
    if (!currentLibEntryId || !libDetailScene || !libDetailRenderer || !libDetailControls) return;
    // Keep the drawing buffer matched to the panel size.
    const w = vp.clientWidth, h = vp.clientHeight;
    if ((w !== lastW || h !== lastH) && w > 0 && h > 0) {
      lastW = w; lastH = h;
      libDetailRenderer.setSize(w, h, false);
      libDetailCamera.aspect = w / h;
      libDetailCamera.updateProjectionMatrix();
    }
    libDetailControls.update();
    libDetailRenderer.render(libDetailScene, libDetailCamera);
  };
  libDetailRaf = requestAnimationFrame(tick);
}

function stopLibDetailLoop() {
  if (libDetailRaf) cancelAnimationFrame(libDetailRaf);
  libDetailRaf = null;
}

// ===== Library: new-component designer =====
let libNewInst = null;
let lnRaf = null;
let lnRenderer = null;
let lnCamera = null;
let lnControls = null;
let lnScene = null;
let lnMeshGroup = null;
let lnBusy = false;
let lnPreviewTimer = null;

const LN_TEMPLATE = `Component(
    name: "MyPart",
    material: "Al-6061-T6",
    visible: true,
    transform: (position: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0)),
    kind: Solid([
        Extrude(profile: Points([(0.0, 0.0), (80.0, 0.0), (80.0, 20.0), (0.0, 20.0)]), height: 10.0, direction: None, taper: None),
    ]),
)`;

const LIBNEW_CONTRACT =
  '\n\n## Output contract (component designer — STRICT)\n' +
  'Emit exactly ONE `Component(...)` value and nothing else — NEVER a Vehicle wrapper, never prose, never a Patch.\n' +
  '- Component fields: name, material, color: Some("#hex") optional, visible: true, transform: (position:(x,y,z), rotation:(x,y,z)), kind.\n' +
  '- kind is one of NoseCone(NoseConeParams(...)), BodyTube(BodyTubeParams(...)), Transition(TransitionParams(...)), Tank(TankParams(...)), Nozzle(NozzleParams(...)), FinSet(FinSetParams(...)) or Solid([SolidOp,...]).\n' +
  '- NEW part: design from scratch with realistic dimensions in Millimeters.\n' +
  '- IMPROVE mode: a current draft RON is provided — apply ONLY what the user asked and keep every other field identical.\n';

function buildLibNewSystemPrompt() {
  let p = aiInstructions || 'You are an expert CAD designer for APRO CAD.';
  p += LIBNEW_CONTRACT;
  p += RON_RULES;
  return p;
}

function openLibNew() {
  setLibView('new');
  if (!libNewInst) {
    libNewInst = createSyntaxEditor(document.getElementById('ln-editor'));
    libNewInst.setRon(LN_TEMPLATE);
    document.getElementById('ln-editor').addEventListener('input', scheduleLnPreview);
  }
  refreshLnPreview();
}

function scheduleLnPreview() {
  clearTimeout(lnPreviewTimer);
  lnPreviewTimer = setTimeout(refreshLnPreview, 400);
}

function ensureLnViewer() {
  if (lnRenderer) return;
  const canvas = document.getElementById('ln-canvas');
  lnRenderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  lnRenderer.setClearColor(0x000000, 0);
  lnCamera = new THREE.PerspectiveCamera(40, 1, 0.01, 500);
  lnCamera.position.set(2.6, 1.9, 3.2);
  lnCamera.lookAt(0, 0, 0);
  lnControls = new OrbitControls(lnCamera, canvas);
  lnControls.enableDamping = true;
  lnControls.dampingFactor = 0.08;
  lnControls.autoRotate = true;
  lnControls.autoRotateSpeed = 2.2;
  lnControls.addEventListener('start', () => { if (lnControls) lnControls.autoRotate = false; });
}

async function refreshLnPreview() {
  if (!libNewInst) return;
  const invoke = tauriInvoke();
  if (!invoke) return;
  const hint = document.getElementById('ln-hint');
  const ron = libNewInst.getRon().trim();
  try {
    const res = await invoke('evaluate', { componentRon: ron });
    if (!res.mesh || !res.mesh.positions || res.mesh.positions.length === 0) {
      throw new Error('no geometry produced');
    }
    ensureLnViewer();
    const m = res.mesh;
    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.BufferAttribute(new Float32Array(m.positions), 3));
    geom.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(m.normals), 3));
    geom.setIndex(new THREE.BufferAttribute(new Uint32Array(m.indices), 1));
    geom.computeBoundingBox();
    const center = geom.boundingBox.getCenter(new THREE.Vector3());
    const sz = geom.boundingBox.getSize(new THREE.Vector3());
    const maxDim = Math.max(sz.x, sz.y, sz.z) || 1;
    geom.translate(-center.x, -center.y, -center.z);

    const mesh = new THREE.Mesh(geom, new THREE.MeshPhysicalMaterial({
      color: libEntryColor({ ron, name: '', params: {} }),
      metalness: 0.25,
      roughness: 0.35,
      clearcoat: 0.3,
      clearcoatRoughness: 0.35,
      side: THREE.DoubleSide,
    }));
    mesh.scale.setScalar(2.8 / maxDim);
    lnMeshGroup = new THREE.Group();
    lnMeshGroup.rotation.x = 0.3;
    lnMeshGroup.add(mesh);

    lnScene = new THREE.Scene();
    lnScene.add(new THREE.HemisphereLight(0xffffff, 0x30354a, 0.95));
    const dir = new THREE.DirectionalLight(0xffffff, 1.15);
    dir.position.set(4, 6, 5);
    lnScene.add(dir);
    lnScene.add(lnMeshGroup);

    if (hint) hint.textContent = 'live preview · drag to orbit';
  } catch (e) {
    if (hint) hint.textContent = 'invalid RON — ' + errToMessage(e).slice(0, 70);
  }
}

function startLnLoop() {
  stopLnLoop();
  if (!tauriInvoke()) return;
  try { ensureLnViewer(); } catch (e) {
    console.error('[library] new-component viewer failed:', e);
    return;
  }
  const vp = document.getElementById('ln-viewport');
  let lastW = 0, lastH = 0;
  const tick = () => {
    lnRaf = requestAnimationFrame(tick);
    const w = vp.clientWidth, h = vp.clientHeight;
    if ((w !== lastW || h !== lastH) && w > 0 && h > 0) {
      lastW = w; lastH = h;
      lnRenderer.setSize(w, h, false);
      lnCamera.aspect = w / h;
      lnCamera.updateProjectionMatrix();
    }
    if (!lnScene || !lnControls) return;
    lnControls.update();
    lnRenderer.render(lnScene, lnCamera);
  };
  lnRaf = requestAnimationFrame(tick);
}

function stopLnLoop() {
  if (lnRaf) cancelAnimationFrame(lnRaf);
  lnRaf = null;
}

function appendLnMsg(cls, text) {
  const log = document.getElementById('ln-chat-log');
  if (!log) return;
  const el = document.createElement('div');
  el.className = 'ln-msg ' + cls;
  el.textContent = text;
  log.appendChild(el);
  log.scrollTop = log.scrollHeight;
}

async function lnSend() {
  if (lnBusy || !libNewInst) return;
  const inp = document.getElementById('ln-chat-prompt');
  const sendBtn = document.getElementById('ln-chat-send');
  const userMsg = inp.value.trim();
  if (!userMsg) return;
  inp.value = '';
  appendLnMsg('user', userMsg);
  lnBusy = true;
  if (sendBtn) sendBtn.disabled = true;

  const system = buildLibNewSystemPrompt();
  const ctxUser =
    `${userMsg}\n\nCurrent component RON:\n\`\`\`ron\n${libNewInst.getRon()}\n\`\`\`\n` +
    `Return the COMPLETE updated Component.`;
  let lastErr = null;
  const invoke = tauriInvoke();

  for (let attempt = 1; attempt <= 3; attempt++) {
    try {
      if (!invoke) throw new Error('desktop app required for AI');
      const { content } = await callAi(system, ctxUser, { constrain: 'component' });
      const doc = await extractWholeDoc(content);
      const issues = await invoke('validate', { componentRon: doc });
      const errs = (issues || []).filter(i => (i.severity || '').toLowerCase() === 'error');
      if (errs.length > 0) throw new Error(errs.map(i => i.message).join('; '));
      libNewInst.setRon(doc);
      scheduleLnPreview();
      appendLnMsg('ai ok', '✓ draft updated — preview refreshed');
      lastErr = null;
      break;
    } catch (e) {
      lastErr = errToMessage(e);
      appendLnMsg('ai err', `✗ ${lastErr}${attempt < 3 ? ' — retrying with the error' : ''}`);
    }
  }
  if (lastErr) appendLnMsg('ai err', 'gave up after 3 attempts — edit the RON manually or rephrase.');
  lnBusy = false;
  if (sendBtn) sendBtn.disabled = false;
}

async function lnSave() {
  if (!libNewInst) return;
  const invoke = tauriInvoke();
  if (!invoke) { showToast('Library requires the desktop app'); return; }
  const ron = libNewInst.getRon().trim();
  try {
    const issues = await invoke('validate', { componentRon: ron });
    const errs = (issues || []).filter(i => (i.severity || '').toLowerCase() === 'error');
    if (errs.length > 0) {
      window.__ux?.notify?.('Fix errors first: ' + errs[0].message, 'err', 3000);
      return;
    }
    const nameInput = document.getElementById('ln-name');
    const name = nameInput.value.trim()
      || (ron.match(/name:\s*"([^"]+)"/) || [])[1]
      || 'Part';
    const tags = document.getElementById('ln-tags').value.split(',').map(t => t.trim()).filter(Boolean);
    const desc = document.getElementById('ln-desc').value.trim();
    await invoke('library_save_ron', { name, description: desc, tags, componentRon: ron });
    window.__ux?.notify?.(`Saved "${name}" to library`, 'info', 2000);
    await refreshLibrary();
    setLibView('grid');
  } catch (e) {
    statusText.textContent = 'Library save failed: ' + e;
  }
}

function renderLibDetailSide(entry) {
  const side = document.getElementById('ld-side');
  if (!side) return;
  side.innerHTML = '';

  // Inline metadata editor (Edit action swaps the overview for a form)
  if (libEditingId === entry.id) {
    const form = document.createElement('div');
    form.className = 'lib-edit-form';
    const nameIn = document.createElement('input');
    nameIn.value = entry.name; nameIn.placeholder = 'Name';
    const descIn = document.createElement('textarea');
    descIn.rows = 3; descIn.value = entry.description || ''; descIn.placeholder = 'Description';
    const tagsIn = document.createElement('input');
    tagsIn.value = (entry.tags || []).join(', '); tagsIn.placeholder = 'tags, comma separated';
    const row = document.createElement('div');
    row.style.display = 'flex'; row.style.gap = '6px';
    const saveBtn = document.createElement('button');
    saveBtn.className = 'lib-btn primary';
    saveBtn.textContent = 'Save';
    saveBtn.addEventListener('click', async () => {
      const invoke = tauriInvoke();
      if (!invoke) return;
      const tags = tagsIn.value.split(',').map(t => t.trim()).filter(Boolean);
      try {
        await invoke('library_update', {
          id: entry.id,
          name: nameIn.value.trim() || entry.name,
          description: descIn.value.trim(),
          tags,
        });
        libEditingId = null;
        await refreshLibrary();
        const fresh = libraryEntries.find(x => x.id === entry.id);
        if (fresh && currentLibEntryId === entry.id) {
          const nameEl = document.getElementById('ld-name');
          if (nameEl) nameEl.textContent = fresh.name;
          renderLibDetailSide(fresh);
        } else {
          renderLibraryGrid();
        }
        window.__ux?.notify?.('Part updated', 'info', 1400);
      } catch (e) {
        statusText.textContent = 'Library update failed: ' + e;
      }
    });
    const cancelBtn = document.createElement('button');
    cancelBtn.className = 'lib-btn';
    cancelBtn.textContent = 'Cancel';
    cancelBtn.addEventListener('click', () => { libEditingId = null; renderLibDetailSide(entry); });
    row.append(saveBtn, cancelBtn);
    form.append(nameIn, descIn, tagsIn, row);
    side.appendChild(form);
  }

  side.appendChild(ldSection('Description'));
  const desc = document.createElement('div');
  desc.className = 'lib-el-desc';
  desc.style.webkitLineClamp = 'unset';
  desc.textContent = entry.description || '(no description)';
  side.appendChild(desc);

  side.appendChild(ldSection('Dimensions'));
  const chips = document.createElement('div');
  chips.className = 'lib-chips';
  libChips(entry).forEach(([label, v, unit]) => {
    const chip = document.createElement('span');
    chip.className = 'lib-chip';
    chip.textContent = label ? `${label} ${v}${unit}` : `${v}${unit}`;
    chips.appendChild(chip);
  });
  if (chips.children.length === 0) {
    const none = document.createElement('span');
    none.className = 'at-empty';
    none.textContent = 'No dimensions recorded';
    chips.appendChild(none);
  }
  side.appendChild(chips);

  if ((entry.tags || []).length > 0) {
    side.appendChild(ldSection('Tags'));
    const tagRow = document.createElement('div');
    tagRow.className = 'lib-chips';
    entry.tags.forEach(t => {
      const chip = document.createElement('span');
      chip.className = 'lib-chip tag';
      chip.textContent = '#' + t;
      tagRow.appendChild(chip);
    });
    side.appendChild(tagRow);
  }

  const meta = document.createElement('div');
  meta.className = 'lib-el-meta';
  const d = entry.created ? new Date(entry.created * 1000) : null;
  const dateStr = d && !isNaN(d)
    ? d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
    : '';
  meta.textContent = `${entry.kind} · used ${entry.use_count}×${dateStr ? ' · added ' + dateStr : ''}`;
  side.appendChild(meta);

  side.appendChild(ldSection('RON source'));
  const ronPre = document.createElement('pre');
  ronPre.className = 'lib-ron open';
  ronPre.textContent = entry.ron;
  side.appendChild(ronPre);

  const actions = document.createElement('div');
  actions.className = 'lib-el-actions';

  const insertBtn = document.createElement('button');
  insertBtn.className = 'lib-btn primary';
  insertBtn.textContent = 'Insert ▸';
  insertBtn.title = 'Add this part to the current document';
  insertBtn.addEventListener('click', () => insertLibraryEntry(entry));

  const editBtn = document.createElement('button');
  editBtn.className = 'lib-btn';
  editBtn.textContent = 'Edit';
  editBtn.addEventListener('click', () => {
    libEditingId = libEditingId === entry.id ? null : entry.id;
    renderLibDetailSide(entry);
  });

  const ronBtn = document.createElement('button');
  ronBtn.className = 'lib-btn';
  ronBtn.textContent = 'Copy RON';
  ronBtn.addEventListener('click', () => {
    navigator.clipboard?.writeText(entry.ron).catch(() => {});
    ronBtn.textContent = 'Copied ✓';
    setTimeout(() => { ronBtn.textContent = 'Copy RON'; }, 1200);
  });

  const delBtn = document.createElement('button');
  delBtn.className = 'lib-btn danger';
  delBtn.textContent = 'Delete';
  delBtn.addEventListener('click', () => confirmLibDelete(entry));

  actions.append(insertBtn, editBtn, ronBtn, delBtn);
  side.appendChild(actions);
}

function confirmLibDelete(entry) {
  pendingLibDeleteId = entry.id;
  const msgEl = document.getElementById('ux-lib-del-msg');
  if (msgEl) msgEl.textContent = `"${entry.name}" (${entry.kind}) will be removed from the library permanently.`;
  const modal = document.getElementById('ux-lib-del');
  if (modal) modal.style.display = 'flex';
}

(function initLibraryUi() {
  const closeBtn = document.getElementById('lib-close');
  if (closeBtn) closeBtn.addEventListener('click', closeLibrary);
  const backBtn = document.getElementById('lib-back');
  if (backBtn) backBtn.addEventListener('click', () => setLibView('grid'));
  const newBtn = document.getElementById('lib-new-btn');
  if (newBtn) newBtn.addEventListener('click', openLibNew);
  const newBackBtn = document.getElementById('lib-new-back');
  if (newBackBtn) newBackBtn.addEventListener('click', () => setLibView('grid'));
  const saveBtn = document.getElementById('ln-save');
  if (saveBtn) saveBtn.addEventListener('click', lnSave);
  const chatSend = document.getElementById('ln-chat-send');
  if (chatSend) chatSend.addEventListener('click', lnSend);
  const chatPrompt = document.getElementById('ln-chat-prompt');
  if (chatPrompt) {
    chatPrompt.addEventListener('keydown', e => {
      if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        lnSend();
      }
    });
  }
  const modal = document.getElementById('library-modal');
  if (modal) modal.addEventListener('click', e => { if (e.target === modal) closeLibrary(); });
  const search = document.getElementById('lib-search');
  if (search) search.addEventListener('input', renderLibraryGrid);
  const kindSel = document.getElementById('lib-kind-filter');
  if (kindSel) kindSel.addEventListener('change', renderLibraryGrid);
  const seed = document.getElementById('lib-seed');
  if (seed) seed.addEventListener('click', async () => {
    const invoke = tauriInvoke();
    if (!invoke) return;
    try {
      const n = await invoke('library_seed_builtins');
      await refreshLibrary();
      window.__ux?.notify?.(`Seeded ${n} builtin part${n === 1 ? '' : 's'}`, 'info', 1800);
    } catch (e) {
      statusText.textContent = 'Seed failed: ' + e;
    }
  });
  const delCancel = document.getElementById('ux-lib-del-cancel');
  if (delCancel) delCancel.addEventListener('click', () => {
    pendingLibDeleteId = null;
    const m = document.getElementById('ux-lib-del');
    if (m) m.style.display = 'none';
  });
  const delOk = document.getElementById('ux-lib-del-ok');
  if (delOk) delOk.addEventListener('click', async () => {
    const id = pendingLibDeleteId;
    const m = document.getElementById('ux-lib-del');
    if (m) m.style.display = 'none';
    pendingLibDeleteId = null;
    if (id == null) return;
    const invoke = tauriInvoke();
    if (!invoke) return;
    try {
      await invoke('library_delete', { id });
      libGeoCache.delete(id);
      libGeomCache.delete(id);
      if (currentLibEntryId === id) closeLibDetail();
      await refreshLibrary();
      window.__ux?.notify?.('Part deleted', 'info', 1500);
    } catch (e) {
      statusText.textContent = 'Delete failed: ' + e;
    }
  });
  document.addEventListener('keydown', e => {
    if (e.key !== 'Escape') return;
    const nw = document.getElementById('lib-new');
    if (nw && nw.style.display !== 'none') { setLibView('grid'); return; }
    const det = document.getElementById('lib-detail');
    if (det && det.style.display !== 'none') { closeLibDetail(); return; }
    const lm = document.getElementById('library-modal');
    if (lm && lm.style.display !== 'none') { closeLibrary(); return; }
    const dm = document.getElementById('ux-lib-del');
    if (dm && dm.style.display !== 'none') {
      dm.style.display = 'none';
      pendingLibDeleteId = null;
    }
  });
})();

window.__apro.openLibrary = openLibrary;

// ===== Home tab: document file I/O + viewport screenshot =====
let lastSavedRon = '';
let acResolver = null;

function markSaved() { lastSavedRon = getEditorRon(); }
function isDirty() { return getEditorRon() !== lastSavedRon; }

// Generic app-level confirmation (unsaved changes, destructive actions).
function askConfirm(title, msg, okLabel = 'Discard changes') {
  const modal = document.getElementById('app-confirm');
  if (!modal) return Promise.resolve(false);
  const t = document.getElementById('ac-title');
  if (t) t.textContent = title;
  const m = document.getElementById('ac-msg');
  if (m) m.textContent = msg;
  const ok = document.getElementById('ac-ok');
  if (ok) ok.textContent = okLabel;
  modal.style.display = 'flex';
  return new Promise(resolve => { acResolver = resolve; });
}

function settleAppConfirm(result) {
  const modal = document.getElementById('app-confirm');
  if (modal) modal.style.display = 'none';
  if (acResolver) { acResolver(result); acResolver = null; }
}

(function initAppConfirm() {
  const c = document.getElementById('ac-cancel');
  const o = document.getElementById('ac-ok');
  const m = document.getElementById('app-confirm');
  if (c) c.addEventListener('click', () => settleAppConfirm(false));
  if (o) o.addEventListener('click', () => settleAppConfirm(true));
  if (m) m.addEventListener('click', e => { if (e.target === m) settleAppConfirm(false); });
  // Capture-phase Esc: the confirm dialog always wins, before any other
  // Escape handler (library panel, selection, mention picker...).
  document.addEventListener('keydown', e => {
    if (e.key !== 'Escape') return;
    const modal = document.getElementById('app-confirm');
    if (modal && modal.style.display !== 'none') {
      e.stopPropagation();
      e.preventDefault();
      settleAppConfirm(false);
    }
  }, true);
})();

function downloadDataUrl(url, filename) {
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
}

const NEW_DOC_TEMPLATE =
  'Vehicle(\n    name: "New Design",\n    units: Millimeters,\n    components: [],\n)';

function applyNewDocument() {
  setEditorRon(NEW_DOC_TEMPLATE);
  if (typeof deselectComponent === 'function') deselectComponent();
  evaluate();
  markSaved();
  window.__ux?.notify?.('New document created', 'info', 1600);
}

async function newDocument() {
  if (isDirty()) {
    const ok = await askConfirm(
      'Unsaved changes',
      'The current design has been modified since the last save. Create a new document anyway?',
      'Discard & create'
    );
    if (!ok) return;
  }
  applyNewDocument();
}

async function saveDocument() {
  const invoke = tauriInvoke();
  const ron = getEditorRon();
  const nameMatch = ron.match(/name:\s*"([^"]+)"/);
  const defaultName = `${(nameMatch && nameMatch[1]) || 'design'}.ron`;
  if (invoke) {
    try {
      // Native Save-As dialog (rfd); writes the file and returns the path.
      const res = await invoke('save_document_dialog', { defaultName, contents: ron });
      if (res == null) return; // user cancelled
      markSaved();
      showToast(`Saved to ${res}`);
      return;
    } catch (e) {
      showToast('Save failed: ' + e);
      return;
    }
  }
  // Browser fallback: plain download.
  downloadDataUrl(URL.createObjectURL(new Blob([ron], { type: 'text/plain' })), defaultName);
  markSaved();
  showToast('Document saved to downloads');
}

// Chromium ignores .click() on a file input whose dialog was previously
// cancelled/dismissed — so we mount a BRAND-NEW input before every open.
function mountOpenFileInput() {
  const mount = document.getElementById('open-file-mount');
  if (!mount) return null;
  mount.innerHTML = '';
  const inp = document.createElement('input');
  inp.id = 'open-file-input';
  inp.type = 'file';
  inp.accept = '.ron,.txt,text/plain';
  inp.style.display = 'none';
  inp.addEventListener('change', () => {
    const f = inp.files && inp.files[0];
    if (!f) return;
    const reader = new FileReader();
    reader.onload = () => {
      try {
        setEditorRon(String(reader.result));
        evaluate();
        markSaved();
        window.__ux?.notify?.(`Opened ${f.name}`, 'info', 1800);
      } catch (e) {
        showToast('Open failed: ' + e);
      }
    };
    reader.readAsText(f);
    inp.value = '';
  });
  mount.appendChild(inp);
  return inp;
}

function openDocument() {
  const proceed = () => {
    const inp = mountOpenFileInput() || document.getElementById('open-file-input');
    if (inp) inp.click();
  };
  if (isDirty()) {
    askConfirm(
      'Unsaved changes',
      'Opening another file will discard the current document.',
      'Discard & open'
    ).then(ok => { if (ok) proceed(); });
    return;
  }
  proceed();
}

function screenshotViewport() {
  try {
    // Render immediately before capture so the buffer is fresh even without
    // preserveDrawingBuffer on the main renderer.
    renderer.render(scene, camera);
    const dataUrl = renderer.domElement.toDataURL('image/png');
    const nameMatch = getEditorRon().match(/name:\s*"([^"]+)"/);
    const defaultName = `${(nameMatch && nameMatch[1]) || 'viewport'}.png`;
    const invoke = tauriInvoke();
    if (invoke) {
      invoke('show_save_path_dialog', {
        defaultName,
        filterName: 'PNG image',
        extensions: ['png'],
      }).then(path => {
        if (!path) { showToast('Screenshot cancelled'); return null; }
        return invoke('save_image_file', { path, dataUrl }).then(msg => {
          showToast('Saved ' + msg);
        });
      }).catch(e => {
        console.error('[io] screenshot failed:', e);
        const msg = errToMessage(e);
        showToast(/unknown|not found|no operation/i.test(msg)
          ? 'Image export needs the rebuilt desktop app — close it and restart tauri dev'
          : 'Screenshot failed: ' + msg);
      });
      return;
    }
    downloadDataUrl(dataUrl, defaultName);
    showToast('Screenshot saved');
  } catch (e) {
    showToast('Screenshot failed: ' + e);
  }
}

markSaved(); // the initially-loaded document is the first saved state

window.__apro.getDocRon = () => getEditorRon();
window.__apro.newDocument = newDocument;
window.__apro.saveDocument = saveDocument;
window.__apro.openDocument = openDocument;
window.__apro.screenshotViewport = screenshotViewport;
window.__apro.setGizmoMode = setGizmoMode;
window.__boot && window.__boot.push('main: apro+io ready');

// ===== Selection + property editing wiring =====
vpPropsClose.addEventListener('click', deselectComponent);

// Feature Manager tree clicks drive 3D highlight + the properties popup.
window.addEventListener('ux:select-part', (e) => {
  const detail = e.detail || {};
  if (!detail.name || detail.deselected || detail.name === selectedComponentName) {
    deselectComponent();
    return;
  }
  selectComponent(detail.name);
});

document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape' || !selectedComponentName) return;
  const tag = (e.target && e.target.tagName) || '';
  if (tag === 'INPUT' || tag === 'TEXTAREA') return;
  deselectComponent();
});

// ===== Click-to-select in the 3D view =====
(function initClickSelect() {
  if (!renderer || !renderer.domElement) return;
  const raycaster = new THREE.Raycaster();
  let downX = 0, downY = 0, downT = 0;

  const pickables = () => {
    const arr = [];
    componentMeshes.forEach(list => { for (const m of list) arr.push(m); });
    return arr;
  };

  renderer.domElement.addEventListener('pointerdown', e => {
    if (e.button !== 0) return;
    downX = e.clientX; downY = e.clientY; downT = performance.now();
  });
  renderer.domElement.addEventListener('pointerup', e => {
    if (e.button !== 0) return;
    // A gizmo drag must never fall through to selection.
    if (suppressClickSelect || (transformGizmo && transformGizmo.dragging)) return;
    // Only treat it as a click when the pointer barely moved (not an orbit/pan).
    if (Math.hypot(e.clientX - downX, e.clientY - downY) > 5) return;
    if (performance.now() - downT > 600) return;

    const rect = renderer.domElement.getBoundingClientRect();
    const ndc = new THREE.Vector2(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );
    raycaster.setFromCamera(ndc, camera);
    const hits = raycaster.intersectObjects(pickables(), false);
    if (hits.length === 0) {
      if (selectedComponentName) deselectComponent();
      return;
    }
    let obj = hits[0].object;
    while (obj && !obj.userData.componentName) obj = obj.parent;
    const name = obj && obj.userData.componentName;
    if (!name) {
      if (selectedComponentName) deselectComponent();
      return;
    }
    // Clicking the selected part toggles it off; clicking another part moves
    // the selection (dimming/highlight follow automatically).
    if (name === selectedComponentName) deselectComponent();
    else selectComponent(name);
  });
})();

// Drag the popup around the viewport by its header.
(function initPropsDrag() {
  const head = document.getElementById('vp-props-head');
  if (!head) return;
  let dragging = false, sx = 0, sy = 0, ox = 0, oy = 0;
  head.addEventListener('pointerdown', (e) => {
    if (e.target === vpPropsClose) return;
    dragging = true; sx = e.clientX; sy = e.clientY;
    const wrapR = document.getElementById('viewport-canvas-wrap').getBoundingClientRect();
    const r = vpPropsPopup.getBoundingClientRect();
    ox = r.left - wrapR.left; oy = r.top - wrapR.top;
    head.setPointerCapture(e.pointerId);
  });
  head.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    const wrapR = document.getElementById('viewport-canvas-wrap').getBoundingClientRect();
    const r = vpPropsPopup.getBoundingClientRect();
    let nx = ox + (e.clientX - sx);
    let ny = oy + (e.clientY - sy);
    nx = Math.max(4, Math.min(wrapR.width - r.width - 4, nx));
    ny = Math.max(4, Math.min(wrapR.height - r.height - 4, ny));
    vpPropsPopup.style.right = 'auto';
    vpPropsPopup.style.left = nx + 'px';
    vpPropsPopup.style.top = ny + 'px';
  });
  head.addEventListener('pointerup', () => { dragging = false; });
})();

vpPropsBody.addEventListener('dblclick', (e) => {
  const td = e.target.closest('td[data-editable]');
  if (!td) return;
  const fieldType = td.getAttribute('data-field-type');

  const paramName = td.getAttribute('data-param-name');
  if (paramName) {
    if (td.querySelector('input')) return;
    const current = td.textContent;
    const input = document.createElement('input');
    input.value = current;
    td.textContent = '';
    td.appendChild(input);
    input.focus();
    input.select();

    function commitParam() {
      const newVal = input.value;
      if (newVal === current) { finishParam(); return; }
      const ron = editor.value;
      const wasComp = isComponentRon(ron);
      const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
      invoke('apply_patch_vehicle', {
        vehicleRon: wrapRon(ron),
        patch: { SetParameter: { name: paramName, value: newVal } }
      }).then(result => {
        if (result.success && result.vehicle_ron) {
          editor.value = unwrapRon(result.vehicle_ron, wasComp);
          finishParam();
          evaluate();
        } else {
          const msg = result.issues.map(i => i.message).join('; ');
          statusText.textContent = 'Patch rejected: ' + (msg || 'validation failed');
          td.textContent = current;
          finishParam();
        }
      }).catch(err => {
        statusText.textContent = 'Patch error: ' + err;
        td.textContent = current;
        finishParam();
      });
    }
    function finishParam() { td.textContent = input.value; }
    input.addEventListener('blur', commitParam);
    input.addEventListener('keydown', (ev) => {
      if (ev.key === 'Enter') { ev.preventDefault(); input.blur(); }
      if (ev.key === 'Escape') { ev.preventDefault(); td.innerHTML = ''; td.textContent = current; }
    });
    return;
  }

  if (fieldType === 'points') {
    if (td.querySelector('textarea')) return;
    const current = td.textContent;
    let display = current;
    try {
      const arr = JSON.parse(current.replace(/\[/g, '[').replace(/\]/g, ']'));
      display = arr.map(p => `[${p[0]}, ${p[1]}]`).join('\n');
    } catch {}
    const textarea = document.createElement('textarea');
    textarea.value = display;
    td.textContent = '';
    td.appendChild(textarea);
    textarea.focus();

    function commitPoints() {
      const newVal = textarea.value;
      if (newVal === display) { finishPoints(); return; }
      const lines = newVal.trim().split('\n').filter(l => l.trim());
      const pts = lines.map(line => {
        const s = line.trim().replace(/[[\]()]/g, '');
        const parts = s.split(',').map(p => parseFloat(p.trim()));
        if (parts.length !== 2 || isNaN(parts[0]) || isNaN(parts[1])) throw new Error(`bad point: ${line}`);
        return [parts[0], parts[1]];
      });
      const formatted = '[' + pts.map(p => `(${p[0]},${p[1]})`).join(',') + ']';
      const compName = td.getAttribute('data-comp-name');
      const key = td.getAttribute('data-key');
      const ron = editor.value;
      const wasComp = isComponentRon(ron);
      const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
      invoke('apply_patch_vehicle', {
        vehicleRon: wrapRon(ron),
        patch: { SetProperty: { component_name: compName, key, value: formatted } }
      }).then(result => {
        if (result.success && result.vehicle_ron) {
          editor.value = unwrapRon(result.vehicle_ron, wasComp);
          finishPoints();
          evaluate();
        } else {
          const msg = result.issues.map(i => i.message).join('; ');
          statusText.textContent = 'Patch rejected: ' + (msg || 'validation failed');
          td.textContent = current;
          finishPoints();
        }
      }).catch(err => {
        statusText.textContent = 'Patch error: ' + err;
        td.textContent = current;
        finishPoints();
      });
    }
    function finishPoints() { td.textContent = textarea.value; }
    textarea.addEventListener('blur', commitPoints);
    textarea.addEventListener('keydown', (ev) => {
      if (ev.key === 'Escape') { ev.preventDefault(); td.innerHTML = ''; td.textContent = current; }
      if (ev.key === 'Enter' && (ev.ctrlKey || ev.metaKey)) { ev.preventDefault(); textarea.blur(); }
    });
    return;
  }

  if (td.querySelector('input')) return;
  const current = td.textContent;
  const input = document.createElement('input');
  input.value = current;
  td.textContent = '';
  td.appendChild(input);
  input.focus();
  input.select();

  function commit() {
    const newVal = input.value;
    if (newVal === current) { finish(); return; }
    const compName = td.getAttribute('data-comp-name');
    const key = td.getAttribute('data-key');
    const ron = editor.value;
    const wasComp = isComponentRon(ron);
    const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
    invoke('apply_patch_vehicle', {
      vehicleRon: wrapRon(ron),
      patch: { SetProperty: { component_name: compName, key, value: newVal } }
    }).then(result => {
      if (result.success && result.vehicle_ron) {
        editor.value = unwrapRon(result.vehicle_ron, wasComp);
        finish();
        evaluate();
      } else {
        const msg = result.issues.map(i => i.message).join('; ');
        statusText.textContent = 'Patch rejected: ' + (msg || 'validation failed');
        td.textContent = current;
        finish();
      }
    }).catch(err => {
      statusText.textContent = 'Patch error: ' + err;
      td.textContent = current;
      finish();
    });
  }
  function finish() { td.textContent = input.value; }
  input.addEventListener('blur', commit);
  input.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter') { ev.preventDefault(); input.blur(); }
    if (ev.key === 'Escape') { ev.preventDefault(); td.innerHTML = ''; td.textContent = current; }
  });
});

// ===== Patch helpers (wrap Component RON in Vehicle for apply_patch_vehicle) =====
function stripRonComments(text) {
  return text.replace(/^\s*\/\/.*$/gm, '').trimStart();
}

function isComponentRon(ron) {
  return stripRonComments(ron).startsWith('Component(');
}

function isPatchRon(ron) {
  return /^(Noop|SetProperty|AddComponent|RemoveComponent|ReorderComponents|PatchList)\s*(\(|$)/.test(ron.trim());
}

function wrapRon(ron) {
  if (!isComponentRon(ron)) return ron;
  return `Vehicle( name: "patch", units: Millimeters, components: [${ron}] )`;
}

function unwrapRon(ron, wasWrapped) {
  if (!wasWrapped) return ron;
  const m = ron.match(/components:\s*\[([\s\S]*?)\]\s*\)\s*$/);
  if (!m) return ron;
  const inner = m[1].trim();
  // Only unwrap when the wrapped vehicle holds exactly one component. If a
  // patch added/removed components, the result is a genuine multi-component
  // Vehicle and must stay that way.
  if ((inner.match(/Component\(/g) || []).length !== 1) return ron;
  return inner;
}

// ===== Undo / Redo =====
let ronHistory = [];
let historyIndex = -1;
const MAX_HISTORY = 50;

function pushHistory(ron) {
  if (historyIndex >= 0 && ronHistory[historyIndex] === ron) return;
  ronHistory = ronHistory.slice(0, historyIndex + 1);
  ronHistory.push(ron);
  if (ronHistory.length > MAX_HISTORY) ronHistory.shift();
  historyIndex = ronHistory.length - 1;
}

function undo() {
  if (historyIndex <= 0) return;
  historyIndex--;
  editor.value = ronHistory[historyIndex];
  evaluate();
  showToast('Undo');
}

function redo() {
  if (historyIndex >= ronHistory.length - 1) return;
  historyIndex++;
  editor.value = ronHistory[historyIndex];
  evaluate();
  showToast('Redo');
}

const _origEval = evaluate;
evaluate = function() {
  if (editor.value.trim()) pushHistory(editor.value);
  return _origEval.apply(this, arguments);
};

// ===== Toast =====
let toastTimer = null;
function showToast(msg) {
  const el = document.getElementById('undo-toast');
  el.textContent = msg;
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 1200);
}

// ===== Viewport toolbar =====
let orbitPanMode = 'orbit'; // 'orbit' or 'pan'
document.getElementById('vp-rotate').addEventListener('click', (e) => {
  orbitPanMode = 'orbit';
  controls.mouseButtons.LEFT = THREE.MOUSE.ROTATE;
  document.getElementById('vp-rotate').classList.add('active');
  document.getElementById('vp-pan').classList.remove('active');
});
document.getElementById('vp-pan').addEventListener('click', (e) => {
  orbitPanMode = 'pan';
  controls.mouseButtons.LEFT = THREE.MOUSE.PAN;
  document.getElementById('vp-pan').classList.add('active');
  document.getElementById('vp-rotate').classList.remove('active');
});

document.getElementById('vp-grid-toggle').addEventListener('click', (e) => {
  gridHelper.visible = !gridHelper.visible;
  e.currentTarget.classList.toggle('active');
});
document.getElementById('vp-axes-toggle').addEventListener('click', (e) => {
  axesHelper.visible = !axesHelper.visible;
  e.currentTarget.classList.toggle('active');
});
document.getElementById('vp-wireframe-toggle').addEventListener('click', (e) => {
  showWireframe = !showWireframe;
  e.currentTarget.classList.toggle('active');
  applyDisplayMode(); // single source of truth for edge visibility
});
document.getElementById('vp-zoom-extents').addEventListener('click', () => {
  frameToFit();
  updateViewportInfo({ mesh: { positions: new Float32Array([]), indices: new Float32Array([]) } });
});
document.getElementById('vp-reset-cam').addEventListener('click', () => {
  controls.target.copy(DEFAULT_TARGET);
  camera.position.copy(DEFAULT_CAM_POS);
  controls.update();
});

// ===== Panel resize =====
const panelResize = document.getElementById('panel-resize');
const editorPanel = document.getElementById('editor-panel');
let isResizing = false;
let startX = 0;
let startW = 0;

panelResize.addEventListener('mousedown', (e) => {
  isResizing = true;
  startX = e.clientX;
  startW = editorPanel.getBoundingClientRect().width;
  document.body.style.cursor = 'col-resize';
  document.body.style.userSelect = 'none';
  e.preventDefault();
});

document.addEventListener('mousemove', (e) => {
  if (!isResizing) return;
  const dx = e.clientX - startX;
  const newWidth = Math.max(240, Math.min(700, startW + dx));
  editorPanel.style.width = newWidth + 'px';
  editorPanel.style.minWidth = newWidth + 'px';
  updateSize();
});

document.addEventListener('mouseup', () => {
  if (isResizing) {
    isResizing = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }
});

// ===== Format button =====
function formatRon(text) {
  // Basic indentation cleanup
  let depth = 0;
  let formatted = text.replace(/([\(\[\{,])/g, '$1\n').replace(/([\)\]\},])/g, '\n$1');
  const lines = formatted.split('\n').map(l => l.trim()).filter(l => l);
  return lines.map(l => {
    const opens = (l.match(/[\(\[\{]/g) || []).length;
    const closes = (l.match(/[\)\]\}]/g) || []).length;
    depth -= closes;
    const indent = '    '.repeat(Math.max(0, depth));
    depth += opens;
    return indent + l;
  }).join('\n');
}

formatBtn.addEventListener('click', () => {
  editor.value = formatRon(editor.value);
  showToast('Formatted');
});

// ===== Event wiring =====
evalBtn.addEventListener('click', evaluate);
if (evalBtnSmall) evalBtnSmall.addEventListener('click', evaluate);

const stlBtn = document.getElementById('stl-btn');
const stepBtn = document.getElementById('step-btn');

// Copy the full RON rulebook (AI_INSTRUCTIONS.md) so external AI chatbots
// can produce valid documents for this app.
const copyRulesBtn = document.getElementById('copy-rules-btn');
if (copyRulesBtn) {
  copyRulesBtn.addEventListener('click', async () => {
    let text = aiInstructions || '';
    if (!text) {
      const invoke = tauriInvoke();
      if (invoke) {
        try { text = await invoke('read_ai_instructions'); } catch {}
      }
      if (!text) {
        try {
          const resp = await fetch('AI_INSTRUCTIONS.md');
          if (resp.ok) text = await resp.text();
        } catch {}
      }
    }
    if (!text) { showToast('RON rules unavailable'); return; }
    const label = copyRulesBtn.textContent;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand('copy'); } catch {}
      ta.remove();
    }
    copyRulesBtn.textContent = '✓';
    setTimeout(() => { copyRulesBtn.textContent = label; }, 1400);
  });
}

async function doExport(format) {
  const ron = getEditorRon();
  const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
  if (!invoke) { setStatus('Tauri IPC not available', false); return; }
  const ext = format === 'step' ? 'stp' : 'stl';
  const nameMatch = ron.match(/name:\s*"([^"]+)"/);
  try {
    // Native Save-As: the user picks where the mesh goes.
    const path = await invoke('show_save_path_dialog', {
      defaultName: `${(nameMatch && nameMatch[1]) || 'export'}.${ext}`,
      filterName: format === 'step' ? 'STEP file' : 'STL mesh',
      extensions: format === 'step' ? ['stp', 'step'] : ['stl'],
    });
    if (!path) { setStatus('Export cancelled', false); return; }
    setStatus(`Exporting ${format.toUpperCase()}...`, true);
    const cmd = format === 'step' ? 'export_step' : 'export_stl';
    const msg = await invoke(cmd, { vehicleRon: ron, path });
    setStatus(msg, false);
  } catch (err) {
    setStatus(`Export error: ${err}`, false);
  }
}

stlBtn.addEventListener('click', () => doExport('stl'));
stepBtn.addEventListener('click', () => doExport('step'));

undoBtn.addEventListener('click', undo);
redoBtn.addEventListener('click', redo);

document.getElementById('preset-select').addEventListener('change', (e) => {
  const ron = PRESETS[e.target.value];
  if (ron) {
    editor.value = ron;
    evaluate();
  }
});

// Debounce auto-evaluate
let debounceTimer = null;
editor.addEventListener('input', () => {
  clearTimeout(debounceTimer);
  updateKindBadge();
  debounceTimer = setTimeout(() => evaluate(), 800);
});

// ===== AI Assistant =====
const AI_INSTRUCTIONS_PATH = 'AI_INSTRUCTIONS.md';
let aiMode = 'plan'; // 'plan' or 'edit'

// Settings
const aiSettingsBtn = document.getElementById('ai-settings-btn');
const aiSettingsModal = document.getElementById('ai-settings-modal');
const aiSettingsClose = document.getElementById('ai-settings-close');
const aiSettingsSave = document.getElementById('ai-settings-save');
const aiApiKeyInput = document.getElementById('ai-api-key');
const aiModelSelect = document.getElementById('ai-model-select');
const aiEndpointInput = document.getElementById('ai-endpoint');

// AI / Chat
const aiModeBtn = document.getElementById('ai-mode-btn');
const aiModeLabel = document.getElementById('ai-mode-label');
const aiPrompt = document.getElementById('ai-prompt');
const aiSendBtn = document.getElementById('ai-send-btn');
const chatMessagesEl = document.getElementById('chat-messages');

let chatMessages = []; // ALWAYS the active conversation's messages array
let msgCounter = 0;
let aiChatBusy = false;
let thinkingMsgId = null;

// ===== Conversations (persisted; title comes from the first prompt) =====
const CONV_STORE_KEY = 'apro_chat_conversations_v1';
let conversations = loadConversations();
let activeConvId = null;
let pendingDeleteConvId = null;

function loadConversations() {
  try {
    const raw = localStorage.getItem(CONV_STORE_KEY);
    const arr = raw ? JSON.parse(raw) : [];
    return Array.isArray(arr)
      ? arr.filter(c => c && typeof c.id === 'string' && Array.isArray(c.messages))
      : [];
  } catch { return []; }
}

function persistConversations() {
  try { localStorage.setItem(CONV_STORE_KEY, JSON.stringify(conversations)); } catch {}
}

function activeConv() { return conversations.find(c => c.id === activeConvId); }

function updateChatTitle() {
  const el = document.getElementById('ux-chat-title-text');
  if (el) el.textContent = activeConv()?.name || 'New conversation';
}

function setConversationName(name) {
  const conv = activeConv();
  if (!conv) return;
  conv.name = name;
  updateChatTitle();
  renderChatMenu();
  persistConversations();
}

function bindConversation(conv) {
  activeConvId = conv.id;
  chatMessages = conv.messages;
  renderChatMessages();
  renderAgentLog();
  updateChatTitle();
  if (chatSuggestionsEl) chatSuggestionsEl.style.display = chatMessages.length || aiPrompt.value.trim() ? 'none' : 'flex';
}

function createConversation() {
  if (aiChatBusy) return null;
  const conv = {
    id: 'c' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6),
    name: null,
    messages: [],
    createdAt: Date.now(),
  };
  conversations.unshift(conv);
  persistConversations();
  bindConversation(conv);
  renderChatMenu();
  return conv;
}

function switchConversation(id) {
  if (aiChatBusy) { window.__ux?.notify?.('Wait for the current task to finish', 'warn', 1800); return; }
  const conv = conversations.find(c => c.id === id);
  if (!conv) return;
  bindConversation(conv);
  renderChatMenu();
}

function deleteConversation(id) {
  const idx = conversations.findIndex(c => c.id === id);
  if (idx < 0) return;
  conversations.splice(idx, 1);
  if (id === activeConvId) {
    if (conversations.length === 0) createConversation();
    else bindConversation(conversations[0]);
  }
  persistConversations();
  renderChatMenu();
  updateChatTitle();
  window.__ux?.notify?.('Conversation deleted', 'info', 1500);
}

function confirmDeleteConversation(id) {
  pendingDeleteConvId = id;
  const c = conversations.find(x => x.id === id);
  const msgEl = document.getElementById('ux-del-msg');
  if (msgEl) msgEl.textContent = `"${c?.name || 'New conversation'}" and its ${c?.messages.length || 0} message(s) will be permanently deleted. This cannot be undone.`;
  const modal = document.getElementById('ux-del-confirm');
  if (modal) modal.style.display = 'flex';
}

function closeDelConfirm() {
  pendingDeleteConvId = null;
  const modal = document.getElementById('ux-del-confirm');
  if (modal) modal.style.display = 'none';
}

function renderChatMenu() {
  const menu = document.getElementById('ux-chat-menu');
  if (!menu) return;
  menu.innerHTML = '';
  const newBtn = document.createElement('button');
  newBtn.className = 'conv-new';
  newBtn.textContent = '+ New conversation';
  newBtn.addEventListener('click', () => { createConversation(); toggleChatMenu(false); });
  menu.appendChild(newBtn);
  conversations.forEach(c => {
    const row = document.createElement('div');
    row.className = 'conv-row' + (c.id === activeConvId ? ' active' : '');
    const nameEl = document.createElement('span');
    nameEl.className = 'conv-name';
    nameEl.textContent = c.name || 'New conversation';
    nameEl.title = c.name || '';
    const count = document.createElement('span');
    count.className = 'conv-count';
    count.textContent = String(c.messages.length);
    const del = document.createElement('button');
    del.className = 'conv-del';
    del.textContent = '✕';
    del.title = 'Delete conversation';
    del.addEventListener('click', e => { e.stopPropagation(); confirmDeleteConversation(c.id); });
    row.append(nameEl, count, del);
    row.addEventListener('click', () => { switchConversation(c.id); toggleChatMenu(false); });
    menu.appendChild(row);
  });
}

function toggleChatMenu(open) {
  const menu = document.getElementById('ux-chat-menu');
  if (!menu) return;
  const show = open ?? menu.style.display === 'none';
  menu.style.display = show ? 'block' : 'none';
  if (show) renderChatMenu();
}

(function initConversationsUi() {
  const titleBtn = document.getElementById('ux-chat-title');
  if (titleBtn) {
    titleBtn.addEventListener('click', e => { e.stopPropagation(); toggleChatMenu(); });
  }
  document.addEventListener('click', e => {
    const menu = document.getElementById('ux-chat-menu');
    if (menu && menu.style.display !== 'none' && !menu.contains(e.target)) toggleChatMenu(false);
  });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape') {
      const menu = document.getElementById('ux-chat-menu');
      if (menu && menu.style.display !== 'none') toggleChatMenu(false);
      closeDelConfirm();
    }
  });
  const cancelBtn = document.getElementById('ux-del-cancel');
  const okBtn = document.getElementById('ux-del-ok');
  if (cancelBtn) cancelBtn.addEventListener('click', closeDelConfirm);
  if (okBtn) okBtn.addEventListener('click', () => {
    if (pendingDeleteConvId != null) deleteConversation(pendingDeleteConvId);
    closeDelConfirm();
  });
  const modal = document.getElementById('ux-del-confirm');
  if (modal) modal.addEventListener('click', e => { if (e.target === modal) closeDelConfirm(); });
})();

// Boot: pick the most recent conversation, or start a fresh one. Deferred so
// every module-level element reference (chatSuggestionsEl etc.) exists.
queueMicrotask(() => {
  if (conversations.length > 0) {
    bindConversation(conversations[0]);
  } else {
    createConversation();
  }
});

function addChatMessage(msg) {
  msg.id = 'msg-' + (++msgCounter);
  msg.timestamp = Date.now();
  chatMessages.push(msg);
  renderChatMessages();
  requestAnimationFrame(() => {
    chatMessagesEl.scrollTop = chatMessagesEl.scrollHeight;
  });
  return msg.id;
}

function removeChatMessage(msgId) {
  const idx = chatMessages.findIndex(m => m.id === msgId);
  if (idx >= 0) {
    chatMessages.splice(idx, 1);
    renderChatMessages();
  }
}

function computeDiff(oldText, newText) {
  if (!oldText || !newText) return { added: 0, removed: 0 };
  const oldLines = oldText.split('\n');
  const newLines = newText.split('\n');
  let added = 0, removed = 0;
  const maxLen = Math.max(oldLines.length, newLines.length);
  for (let i = 0; i < maxLen; i++) {
    if (i >= oldLines.length) added++;
    else if (i >= newLines.length) removed++;
    else if (oldLines[i] !== newLines[i]) { added++; removed++; }
  }
  return { added, removed };
}

function formatMessageHtml(text) {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/\n/g, '<br>');
}

function isRonCode(code) {
  const t = code.trim();
  return t.startsWith('Vehicle(') || t.startsWith('Component(') || t.startsWith('{');
}

function implementCode(code, btn) {
  editor.value = code;
  evaluate();
  btn.textContent = 'Implemented';
  btn.disabled = true;
  btn.style.opacity = '0.6';
  switchTab('editor');
}

function renderChatMessages() {
  persistConversations(); // single choke point: every mutation re-renders
  chatMessagesEl.innerHTML = '';
  chatMessages.forEach(msg => {
    const bubble = document.createElement('div');
    bubble.className = 'chat-msg ' + (msg.role === 'user' ? 'user-msg' : 'assistant-msg');

    // Header row: avatar, role label, mode, constraint, timestamp
    const head = document.createElement('div');
    head.className = 'msg-head';
    const avatar = document.createElement('span');
    avatar.className = 'msg-avatar';
    avatar.textContent = msg.role === 'user' ? 'Y' : '✦';
    head.appendChild(avatar);
    const label = document.createElement('span');
    label.className = 'msg-role-label';
    label.textContent = msg.role === 'user' ? 'You' : (msg.role === 'thinking' ? 'AI' : 'AI Studio');
    if (msg.mode) {
      const modeSpan = document.createElement('span');
      modeSpan.className = 'msg-mode-label';
      modeSpan.textContent = msg.mode === 'edit' ? 'Edit' : 'Plan';
      label.appendChild(modeSpan);
    }
    if (msg.constrained) {
      const tag = document.createElement('span');
      tag.className = 'msg-constraint-tag';
      tag.textContent = 'constrained';
      label.appendChild(tag);
    }
    head.appendChild(label);
    const time = document.createElement('span');
    time.className = 'msg-time';
    time.textContent = new Date(msg.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    head.appendChild(time);
    bubble.appendChild(head);

    if (msg.role === 'user') {
      // User messages: plain text only
      const text = document.createElement('div');
      text.className = 'msg-text';
      text.textContent = msg.content;
      bubble.appendChild(text);
    } else if (msg.role === 'thinking') {
      // Thinking indicator with animated dots
      const text = document.createElement('div');
      text.className = 'msg-text thinking';
      const label = document.createElement('span');
      label.textContent = 'Thinking';
      text.appendChild(label);
      for (let i = 0; i < 3; i++) {
        const dot = document.createElement('span');
        dot.className = 'dot';
        text.appendChild(dot);
      }
      bubble.appendChild(text);
    } else {
      // Assistant messages: parse formatting and code blocks
      const content = msg.content;
      // Split by code blocks (```...```)
      const parts = content.split(/(```[\s\S]*?```)/);

      parts.forEach(part => {
        const codeMatch = part.match(/```(\w*)\n?([\s\S]*?)```/);
        if (codeMatch) {
          const lang = codeMatch[1];
          const code = codeMatch[2].trim();
          const isRon = lang === 'ron' || (!lang && isRonCode(code));

          const block = document.createElement('div');
          block.className = 'code-block';

          const bar = document.createElement('div');
          bar.className = 'code-bar';
          const barLabel = document.createElement('span');
          barLabel.className = 'code-lang';
          barLabel.textContent = (lang || (isRon ? 'ron' : 'code')).toUpperCase();
          const copyBtn = document.createElement('button');
          copyBtn.className = 'code-copy';
          copyBtn.textContent = 'Copy';
          copyBtn.addEventListener('click', () => {
            navigator.clipboard.writeText(code);
            copyBtn.textContent = 'Copied ✓';
            setTimeout(() => { copyBtn.textContent = 'Copy'; }, 1200);
          });
          bar.appendChild(barLabel);
          bar.appendChild(copyBtn);
          block.appendChild(bar);

          const toggle = document.createElement('div');
          toggle.className = 'ron-toggle';
          toggle.textContent = isRon ? 'Show RON' : ('Show ' + (lang || 'code'));
          const codeEl = document.createElement('div');
          codeEl.className = 'ron-content';
          codeEl.textContent = code;
          toggle.addEventListener('click', () => {
            toggle.classList.toggle('expanded');
            codeEl.classList.toggle('open');
          });
          block.appendChild(toggle);
          block.appendChild(codeEl);

          // Implement button for RON code
          if (isRon) {
            const actions = document.createElement('div');
            actions.className = 'msg-actions';
            const implBtn = document.createElement('button');
            implBtn.className = 'tb-btn accept';
            implBtn.textContent = 'Implement';
            implBtn.addEventListener('click', () => implementCode(code, implBtn));
            actions.appendChild(implBtn);
            block.appendChild(actions);
          }
          bubble.appendChild(block);
        } else if (part.trim()) {
          // Text segment: apply formatting
          const textDiv = document.createElement('div');
          textDiv.className = 'msg-text';
          textDiv.innerHTML = formatMessageHtml(part);
          if (!isRonCode(part.trim()) && msg.role === 'assistant') {
            const copyBtn = document.createElement('button');
            copyBtn.className = 'msg-copy';
            copyBtn.textContent = 'Copy';
            copyBtn.addEventListener('click', () => {
              navigator.clipboard.writeText(part.trim());
              copyBtn.textContent = 'Copied ✓';
              setTimeout(() => { copyBtn.textContent = 'Copy'; }, 1200);
            });
            textDiv.appendChild(copyBtn);
          }
          bubble.appendChild(textDiv);
        }
      });

      // Plan checklist (msg.todos from plan mode or agent session)
      if (msg.todos && msg.todos.length) {
        const planEl = document.createElement('div');
        planEl.className = 'plan-card';
        const planHead = document.createElement('div');
        planHead.className = 'plan-head';
        const planTitle = document.createElement('span');
        planTitle.textContent = 'PLAN';
        const planCount = document.createElement('span');
        planCount.className = 'plan-count';
        planCount.textContent = `${msg.todos.length} step${msg.todos.length > 1 ? 's' : ''}`;
        planHead.appendChild(planTitle);
        planHead.appendChild(planCount);
        planEl.appendChild(planHead);
        msg.todos.forEach((todo, i) => {
          const item = document.createElement('div');
          item.className = 'plan-item';
          item.style.animationDelay = `${i * 60}ms`;
          const idx = document.createElement('span');
          idx.className = 'plan-idx';
          idx.textContent = i + 1;
          const txt = document.createElement('span');
          txt.className = 'plan-txt';
          txt.textContent = todo;
          item.appendChild(idx);
          item.appendChild(txt);
          planEl.appendChild(item);
        });
        bubble.appendChild(planEl);
      }
    }

    // Diff stats for edit responses with stored RON
    if (msg.ron && msg.ronDiff) {
      const stats = document.createElement('div');
      stats.className = 'diff-stats';
      if (msg.ronDiff.added > 0) {
        const addSpan = document.createElement('span');
        addSpan.className = 'diff-added';
        addSpan.textContent = `+${msg.ronDiff.added}`;
        stats.appendChild(addSpan);
      }
      if (msg.ronDiff.removed > 0) {
        const remSpan = document.createElement('span');
        remSpan.className = 'diff-removed';
        remSpan.textContent = `-${msg.ronDiff.removed}`;
        stats.appendChild(remSpan);
      }
      const summary = document.createElement('span');
      summary.className = 'diff-summary';
      summary.textContent = msg.ronSummary || 'RON updated';
      stats.appendChild(summary);
      bubble.appendChild(stats);
    }

    // Collapsible stored RON section (edit mode)
    if (msg.ron) {
      const toggle = document.createElement('div');
      toggle.className = 'ron-toggle';
      toggle.textContent = 'Show RON';
      const ronContent = document.createElement('div');
      ronContent.className = 'ron-content';
      ronContent.textContent = msg.ron;
      toggle.addEventListener('click', () => {
        toggle.classList.toggle('expanded');
        ronContent.classList.toggle('open');
      });
      bubble.appendChild(toggle);
      bubble.appendChild(ronContent);
    }

    // Action buttons for pending edit
    if (msg.role === 'assistant' && msg.ron && msg.status === 'pending') {
      const actions = document.createElement('div');
      actions.className = 'msg-actions';
      const acceptBtn = document.createElement('button');
      acceptBtn.className = 'tb-btn accept';
      acceptBtn.textContent = 'Accept';
      acceptBtn.addEventListener('click', () => acceptEdit(msg.id));
      const declineBtn = document.createElement('button');
      declineBtn.className = 'tb-btn decline';
      declineBtn.textContent = 'Decline';
      declineBtn.addEventListener('click', () => declineEdit(msg.id));
      actions.appendChild(acceptBtn);
      actions.appendChild(declineBtn);
      bubble.appendChild(actions);
    }

    // Status badge for resolved edits
    if (msg.role === 'assistant' && msg.ron && msg.status && msg.status !== 'pending') {
      const statusDiv = document.createElement('div');
      statusDiv.className = 'status-badge ' + msg.status;
      statusDiv.textContent = msg.status === 'accepted' ? 'Accepted' : 'Declined';
      bubble.appendChild(statusDiv);
    }

    chatMessagesEl.appendChild(bubble);
  });
  renderAgentLog();
}

function acceptEdit(msgId) {
  const msg = chatMessages.find(m => m.id === msgId);
  if (!msg || !msg.ron) return;
  editor.value = msg.ron;
  msg.status = 'accepted';
  renderChatMessages();
  evaluate();
  libraryBumpUse(msg.libraryHitIds);
}

function declineEdit(msgId) {
  const msg = chatMessages.find(m => m.id === msgId);
  if (!msg) return;
  msg.status = 'declined';
  // Agent-loop sessions evolved the document live: declining reverts to the
  // pre-session state.
  if (msg.ronOriginal) {
    editor.value = msg.ronOriginal;
    evaluate();
    showToast('Reverted AI changes');
  }
  renderChatMessages();
}

// ===== Tab switching =====
function switchTab(tabName) {
  document.querySelectorAll('#editor-tabs .tab-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.tab === tabName);
  });
  document.querySelectorAll('#editor-panel .tab-content').forEach(el => {
    el.classList.toggle('active', el.id === tabName + '-tab');
  });
}
document.querySelectorAll('#editor-tabs .tab-btn').forEach(btn => {
  btn.addEventListener('click', () => switchTab(btn.dataset.tab));
});

// Load settings from localStorage
function loadAiSettings() {
  const saved = localStorage.getItem('apro_ai_settings');
  if (saved) {
    try {
      const s = JSON.parse(saved);
      if (s.api_key) aiApiKeyInput.value = s.api_key;
      if (s.model) aiModelSelect.value = s.model;
      if (s.endpoint) aiEndpointInput.value = s.endpoint;
    } catch {}
  }
}
loadAiSettings();

function saveAiSettings() {
  localStorage.setItem('apro_ai_settings', JSON.stringify({
    api_key: aiApiKeyInput.value,
    model: aiModelSelect.value,
    endpoint: aiEndpointInput.value,
  }));
}

// Settings modal
let aiSettingsOpen = false;
aiSettingsBtn.addEventListener('click', () => {
  aiSettingsOpen = !aiSettingsOpen;
  aiSettingsModal.style.display = aiSettingsOpen ? 'flex' : 'none';
});
aiSettingsClose.addEventListener('click', () => {
  aiSettingsOpen = false;
  aiSettingsModal.style.display = 'none';
});
aiSettingsModal.addEventListener('click', (e) => {
  if (e.target === aiSettingsModal || e.target.id === 'ai-settings-backdrop') {
    aiSettingsOpen = false;
    aiSettingsModal.style.display = 'none';
  }
});
aiSettingsSave.addEventListener('click', () => {
  saveAiSettings();
  aiSettingsOpen = false;
  aiSettingsModal.style.display = 'none';
});

// AI mode toggle
function setAiMode(mode) {
  aiMode = mode;
  const isEdit = aiMode === 'edit';
  // The Plan button must never carry edit styling; each button reflects only
  // the active mode (this was the "plan stays highlighted" bug).
  aiModeBtn.classList.toggle('active', !isEdit);
  aiModeBtn.classList.remove('edit-mode');
  const editBtn = document.getElementById('ai-mode-edit');
  if (editBtn) editBtn.classList.toggle('active', isEdit);
  if (aiModeLabel) aiModeLabel.textContent = isEdit ? '(Edit)' : '(Plan)';
  const hint = document.getElementById('ux-mode-hint');
  if (hint) hint.textContent = isEdit ? 'modifies the model, evolving it live' : 'analyzes, never changes the model';
  aiPrompt.placeholder = isEdit
    ? 'Describe what to change in the current design...'
    : 'Ask the AI to plan a design...';
}
aiModeBtn.addEventListener('click', () => setAiMode(aiMode === 'plan' ? 'edit' : 'plan'));
const aiModeEditBtn = document.getElementById('ai-mode-edit');
if (aiModeEditBtn) aiModeEditBtn.addEventListener('click', () => setAiMode('edit'));
setAiMode('edit'); // Edit mode is the default

// Load AI_INSTRUCTIONS.md
let aiInstructions = '';
async function loadAiInstructions() {
  const invoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
  if (!invoke) return;
  try {
    aiInstructions = await invoke('read_ai_instructions');
  } catch {
    // Fallback: try fetch in case we're running in a browser context
    try {
      const resp = await fetch('AI_INSTRUCTIONS.md');
      if (resp.ok) aiInstructions = await resp.text();
    } catch {}
  }
}
loadAiInstructions();

// Build system prompt (async: injects library-retrieved reference parts)
// Output contracts for the agent loop (edit mode only; plan mode stays free text)
const PATCH_CONTRACT =
  '\n\n## Output contract (edit mode — STRICT)\n' +
  'Emit exactly ONE `Patch` value and nothing else. Do NOT emit a whole Vehicle or Component document.\n' +
  'Make the SMALLEST change that satisfies the request — never rewrite, reorder or re-emit components the request did not mention.\n' +
  'Patch variants:\n' +
  '- SetProperty(component_name: "<name>", key: "<param key from inventory>", value: "<string>") — value is ALWAYS a string, even for numbers; use op_<idx>_<field> keys for Solid ops; key `color` sets the part\'s display color ("#ff8800" or a name like "red"; "auto" clears it)\n' +
  '- SetParameter(name: "<param name from inventory>", value: "<string>") — create or edit an entry in the Parameters block; value is ALWAYS a string: a plain number ("98.0") or a quoted equation ("body_od - 2 * wall"). If the vehicle has no Parameters block yet, this CREATES one. Use this for named, reusable dimensions (body_od, wall, ...)\n' +
  '- AddComponent(after_component: Some("<name>") | None, component: Component(...))\n' +
  '- RemoveComponent(component_name: "<name>")\n' +
  '- ReorderComponents(from_index: <usize>, to_index: <usize>)\n' +
  '- PatchList(patches: [<Patch>, <Patch>, ...]) — apply several changes in ONE step. PREFER this when a change touches multiple parameters (e.g. make a fin detailed: one PatchList of SetProperty ops). All-or-nothing: every sub-patch must be valid.\n' +
  '- Noop — emit this when the design already satisfies the todo and no change is needed\n' +
  'Component names must match the inventory exactly.';

// Shared RON + validation rules: appended to every edit-mode prompt so both
// the planner and the patch steps emit parseable, valid output.
const RON_RULES =
  '\n\n## RON syntax rules (your output is parsed by a strict RON parser)\n' +
  '- Enum variants are written BARE — never qualified: write `Hemispherical`, NOT `DomeKind.Hemispherical`; write `Some("Body")` / `None`, NOT `Option.Some(...)`.\n' +
  '- Never use Rust `::` paths, `r#` raw identifiers, or trailing commas.\n' +
  '- All values are literal numbers or quoted strings; expressions/variable names are only allowed inside the Parameters block.\n' +
  '- Struct fields: `Name(field: value, ...)`. Optional fields may be omitted; required fields must be present.\n' +
  '- Parameters block (optional): ALWAYS use the LIST form `parameters: Some([Parameter(name: "body_od", value: 98.0), Parameter(name: "wall", value: 2.0), Parameter(name: "body_id", value: "body_od - 2 * wall")])` — one `Parameter` per entry, names in double quotes. A value is a literal number OR a quoted equation string. Create/edit entries with `SetParameter(name: "...", value: "...")`; a vehicle without a Parameters block gets one created on first SetParameter.\n' +
  '- Equation rules (for Parameters values): bare lowercase names, explicit `*` (write `2 * wall`, NOT `2wall`), operators `+ - * / ^`, parens, functions `abs sqrt sin cos tan min max pow`, constants `pi e`, forward references allowed. Circular or unknown-name references are rejected.\n' +
  '- When a request gives numeric relationships (e.g. "body inner diameter = outer - 2 * wall"), prefer a Parameters equation over computing a fixed number.\n' +
  '- Component colors: each Component accepts an optional `color` field — `color: Some("#ff8800")` or a color NAME like `color: Some("red")`. Omit it (`color: None`) to let the renderer derive a stable color from the material. When the user asks for specific colors, set them on every component in the same patch; distinct parts should get distinct colors.\n' +
  '\n## Validation rules (patches are checked before they are applied)\n' +
  '- All dimensions and wall thicknesses must be positive; wall is typically 1.0–5.0 mm.\n' +
  '- Units: dimensions are in the document Units (default Millimeters) — convert real-world meters to mm (1 m -> 1000.0); avoid sub-mm structural sizes.\n' +
  '- Nozzle: throat_radius must be < chamber_radius.\n' +
  '- FinSet: count must be >= 3; span, chords and thickness positive.\n' +
  '- Solid: must contain at least one op (Revolve, Extrude, Loft, ...) — never an empty list.\n' +
  '- Tank: dome is `Hemispherical` or `Ellipsoidal { ratio: <float> }`; wall positive.\n' +
  '- Mating: when a body tube exists, a Nose base_radius and a Nozzle chamber_radius must match the body radius (within 2.0). Plan the same radius for parts that join!\n' +
  '- Mating dimensions must be set EXPLICITLY when adding a component — never leave a default (e.g. set Nozzle chamber_radius, Nose base_radius, Tank radius, Body radius/wall in the same AddComponent). The engine checks mating AFTER every patch: a mismatch rejects the whole patch.\n' +
  '- Fitting: a Tank (or other internal part) must fit inside the Body it mounts in: tank.radius + tank.wall <= body.radius - body.wall. If a request specifies conflicting radii, adjust the inner part\'s radius (and note it) rather than failing.';

// Tauri rejects with plain strings; browsers reject with Errors. Normalize so
// the repair loop and the log always see the real message.
function errToMessage(err) {
  if (err == null) return 'unknown error';
  if (err instanceof Error) return err.message || String(err);
  return String(err);
}

const PLAN_CONTRACT =
  '\n\n## Output contract (planning step — STRICT)\n' +
  'Think through the user request against the component inventory, then emit exactly ONE `Plan` value and nothing else.\n' +
  'A Plan is: Plan(todos: ["<todo 1>", "<todo 2>", ...])\n' +
  '- DEFAULT: INLINE EDITING. Emit todos that patch ONLY the components the request touches (SetProperty, SetParameter, AddComponent, RemoveComponent, ReorderComponents). NEVER plan rebuilding, re-emitting or reformatting components the user did not ask about — they must remain exactly as they are.\n' +
  '- REGENERATE ROUTE: emit EXACTLY ONE todo `__regenerate__: <short description>` and NOTHING else ONLY when the user explicitly asks for a brand-new or replacement design (e.g. "design me a rocket", "start over with a ...") or the document is empty. Modification requests (@-mentions, "make X bigger", "reduce Y by 20%", "change color") are NEVER regeneration.\n' +
  '- One todo per distinct change; short imperative sentences that mention exact component names and target values (e.g. "Increase Nose length to 300.0", "Change Body material to G10-FR4"). A todo may need several Patch ops (use a PatchList in the execution step).\n' +
  '- Choose all radii BEFORE adding parts: parts that join a body tube must share the body radius (within 2.0); anything mounted INSIDE the body must satisfy tank.radius + tank.wall <= body.radius - body.wall. If the request gives conflicting radii, resolve the conflict in your plan (e.g. shrink the inner part) so every todo is executable.\n' +
  '- Units: all lengths are in the document\'s Units (default Millimeters). Convert real-world sizes to the document units — a real F-1 nozzle chamber is about 1000 mm across, so write 1000.0, NOT 1.0. Never plan sub-mm structural parts.\n' +
  '- Decide the number of todos yourself — 1 to 40, no artificial limit; the request decides. If the design already fully satisfies the request, emit an empty list: Plan(todos: []).';

// Design step: the session starts by generating a COMPLETE new document from
// the request, then refines it via patches. The model must not build designs
// by copying the current design or the reference library — fresh geometry only.
const DESIGN_CONTRACT =
  '\n\n## Output contract (design step — STRICT)\n' +
  'Design a COMPLETE new Vehicle document for this request from scratch. Emit exactly ONE `Vehicle(...)` value and nothing else.\n' +
  '- Build geometry with the general Solid ops (Extrude, Revolve, RevolveChain, Loft, Sweep, Boolean, TransformOp) and a Parameters block for named numeric relationships. Do NOT copy components or part names from the Current Design or the library; design from the request.\n' +
  '- EXCEPTION — modification requests: if the user is clearly asking to MODIFY the current design (e.g. "make it longer", "add fins", "change the material"), keep that design\'s structure and apply the requested change in the emitted document.\n' +
  '- Required fields: name, units, components. Each Component needs name, material, visible (optional), transform, kind. Use the exact RON forms in the syntax rules below.\n' +
  '- Realistic dimensions in the document Units (Millimeters); convert real-world sizes (1 m -> 1000.0); never sub-mm structural parts.\n' +
  '- The design must evaluate cleanly: positive dimensions, mating parts share radii, Boolean is never the first op, no circular references, one part per request is fine.\n' +
  '- 4-space indent; the document must be complete and self-contained.';

async function buildSystemPrompt(userPrompt, opts = {}) {
  let prompt = aiInstructions || 'You are an expert CAD designer for APRO CAD, a rocket design application.';
  if (aiMode === 'edit') {
    const invoke = tauriInvoke();
    let inventory = null;
    if (invoke && opts.skipInventory !== true) {
      try {
        const tables = await invoke('describe_vehicle', { vehicleRon: editor.value });
        if (tables && tables.length > 0) {
          const paramRows = await invoke('describe_parameters', { vehicleRon: editor.value }).catch(() => []);
          inventory = tables.map(t => {
            const rows = t.rows.map(r => `  - ${r.key} = ${r.value} (${r.field_type})`).join('\n');
            return `${t.name} (${t.kind}):\n${rows}`;
          }).join('\n');
          if (paramRows && paramRows.length > 0) {
            const ptext = paramRows.map(p => `  - ${p.name} = ${p.value}${p.computed != null ? `  (computed: ${p.computed})` : ''}`).join('\n');
            inventory = 'Parameters:\n' + ptext + '\n\n' + inventory;
          }
        }
      } catch {}
    }
    if (inventory) {
      prompt += '\n\n## Current Design (component inventory; param keys below are exactly what SetProperty accepts)\n' + inventory;
    } else if (opts.skipCurrentDoc !== true) {
      prompt += '\n\n## Current Design (RON)\n```ron\n' + editor.value + '\n```';
    }
    prompt += RON_RULES;
    if (lastDesignState && opts.skipCurrentDoc !== true) prompt += lastDesignState;
  }

  // Library retrieval: inject top reference parts to guide generation
  const hitIds = [];
  if (opts.retrieve !== false) {
    try {
      const invoke = tauriInvoke();
      if (invoke && userPrompt && userPrompt.trim()) {
        const resp = await invoke('library_retrieve', {
          text: userPrompt,
          kind: null,
          odMm: null,
          lengthMm: null,
          lengthMaxMm: null,
          fitsOdMm: null,
          limit: 5,
        });
        if (resp && resp.hits && resp.hits.length > 0) {
          prompt += '\n\n## Reference Parts from Library (use these as style/param guidance)';
          prompt += '\nREUSE their exact parameter values (dimensions, materials, airfoil families) for AddComponent and SetProperty — do not invent new ones when a library part fits the request.';
          if (resp.dimension_mismatch) {
            prompt += '\nNOTE: no library part matched the requested dimensions; the below are closest matches.';
          }
          resp.hits.forEach((h, i) => {
            prompt += `\n\n### ${i + 1}. ${h.entry.name} (relevance ${h.score.toFixed(2)})\n${h.fewShot}\n`;
            hitIds.push(h.entry.id);
          });
        }
      }
    } catch {}
  }

  // @-mentioned component data (resolved at send time) reaches every call.
  if (mentionContextBlock) prompt += mentionContextBlock;
  return { prompt, hitIds };
}

// Phase 2: schema/GBNF-constrained generation
const aiConstraintBadge = document.getElementById('ai-constraint-badge');
let aiSchemaBundle = null;
let aiUnconstrained = false;

function tauriInvoke() {
  return window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
}

async function loadAiSchemaBundle() {
  if (aiSchemaBundle) return aiSchemaBundle;
  const invoke = tauriInvoke();
  if (!invoke) return null;
  try {
    aiSchemaBundle = await invoke('get_ai_schema');
  } catch (e) {
    console.warn('get_ai_schema failed:', e);
    aiSchemaBundle = null;
  }
  return aiSchemaBundle;
}

function isLocalEndpoint(endpoint) {
  try {
    const host = new URL(endpoint).hostname;
    return host === 'localhost' || host === '127.0.0.1' || host.endsWith('.local');
  } catch {
    return false;
  }
}

function updateConstraintBadge() {
  if (aiConstraintBadge) {
    aiConstraintBadge.style.display = aiUnconstrained ? '' : 'none';
  }
}

// Call AI API
async function callAi(systemPrompt, userPrompt, opts = {}) {
  const endpoint = aiEndpointInput.value || 'https://api.openai.com/v1/chat/completions';
  const apiKey = aiApiKeyInput.value;
  const model = aiModelSelect.value;

  if (!apiKey) {
    throw new Error('AI API key not configured. Click ⚙ to set it up.');
  }

  const body = {
    model,
    messages: [
      { role: 'system', content: systemPrompt },
      { role: 'user', content: userPrompt },
    ],
    temperature: opts.temperature ?? 0.7,
    max_tokens: 4096,
  };

  // Edit mode: constrain output. Cloud endpoints get response_format
  // json_schema (model emits JSON -> converted to RON), local llama.cpp
  // endpoints get a GBNF grammar (model emits RON directly).
  const CONSTRAINT_KINDS = {
    vehicle: { name: 'apro_vehicle', schema: 'vehicle_schema', gbnf: 'gbnf_vehicle' },
    component: { name: 'apro_component', schema: 'component_schema', gbnf: 'gbnf_component' },
    patch: { name: 'apro_patch', schema: 'patch_schema', gbnf: 'gbnf_patch' },
    plan: { name: 'apro_plan', schema: 'plan_schema', gbnf: 'gbnf_plan' },
  };
  const wantConstraint = opts.constrain && opts.constrain !== 'none';
  if (wantConstraint && !aiUnconstrained) {
    const bundle = await loadAiSchemaBundle();
    if (bundle) {
      const kind = CONSTRAINT_KINDS[opts.constrain] || CONSTRAINT_KINDS.vehicle;
      if (isLocalEndpoint(endpoint)) {
        body.grammar = bundle[kind.gbnf];
      } else {
        body.response_format = {
          type: 'json_schema',
          json_schema: {
            name: kind.name,
            schema: bundle[kind.schema],
            strict: false,
          },
        };
      }
    } else {
      // Could not obtain schema (non-Tauri context, etc.): run unconstrained
      // but flag it so the UI warns the user.
      aiUnconstrained = true;
      updateConstraintBadge();
    }
  }

  const postToAi = () =>
    fetch(endpoint, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': 'Bearer ' + apiKey,
      },
      body: JSON.stringify(body),
    });

  let resp = null;
  let strippedConstraint = false;

  // Bounded request loop: rate limits and constraint rejections can arrive in
  // ANY order (a schema-rejection 400 often follows 429 retries — the old
  // sequential checks missed that case and surfaced raw API errors).
  for (let attempt = 0; attempt < 6; attempt++) {
    resp = await postToAi();
    if (resp.ok) break;

    if (resp.status === 429) {
      const errText = await resp.text().catch(() => '');
      const cooldownMatch = errText.match(/try again in ([\d.]+)s/);
      const waitMs = cooldownMatch
        ? Math.ceil(parseFloat(cooldownMatch[1]) * 1000) + 500
        : Math.min(5000 * ((attempt % 3) + 1), 15000);
      addAgentLine(`  ⏳ rate-limited, waiting ${(waitMs / 1000).toFixed(1)}s...`, 'agent-err');
      await new Promise(r => setTimeout(r, waitMs));
      continue;
    }

    if (resp.status === 400 && !strippedConstraint && (body.response_format || body.grammar)) {
      // Endpoint rejected the response schema: degrade to unconstrained once.
      strippedConstraint = true;
      aiUnconstrained = true;
      updateConstraintBadge();
      addAgentLine('  ⚠ response schema rejected by endpoint — finishing unconstrained', 'agent-warn');
      delete body.response_format;
      delete body.grammar;
      continue;
    }

    break;
  }

  if (!resp || !resp.ok) {
    const errText = resp ? await resp.text().catch(() => 'unknown error') : 'no response';
    throw new Error(`API error (${resp ? resp.status : 0}): ${errText}`);
  }

  const data = await resp.json();
  if (!data.choices || !data.choices[0]) {
    throw new Error('API returned empty response');
  }

  return { content: data.choices[0].message.content.trim(), constrained: !aiUnconstrained && wantConstraint };
}

// ===== Agent loop: design from scratch -> plan -> execute -> fix -> review =====
let agentLogLines = []; // {text, cls}
// The model decides how many todos a request needs. 40 is a safety ceiling,
// not a design limit — a big multi-part request may legitimately need 20+.
const MAX_TODOS = 40;
// Stop-and-fix: each step retries against the reported error before moving on.
const MAX_TODO_RETRIES = 5;
const MAX_REVIEW_ROUNDS = 3;
// Measured design state after the last applied change (feedback loop):
// injected into every subsequent prompt so the model sees live mass, CoM and
// warnings instead of designing blind.
let lastDesignState = null;

function formatDesignState(ev, tables) {
  const lines = [];
  if (ev && ev.mass_props) {
    const mp = ev.mass_props;
    lines.push(`- Mass: ${(mp.mass * 1000).toFixed(1)} g`);
    lines.push(`- Center of mass (mm): (${mp.center_of_mass.map(v => v.toFixed(1)).join(', ')})`);
  }
  if (tables && tables.length > 0) {
    lines.push(`- Components: ${tables.length}`);
    const extras = tables.map(t => `${t.name}: ${t.rows.map(r => `${r.key}=${r.value}`).join(', ')}`);
    lines.push(`- Current parameters: ${extras.join(' | ')}`);
  }
  if (ev && ev.issues && ev.issues.length > 0) {
    const warnings = ev.issues.filter(i => (i.severity || '').toLowerCase() === 'warning');
    if (warnings.length) {
      lines.push('- Warnings: ' + warnings.map(w => w.message).join('; '));
    }
  }
  if (lines.length === 0) return null;
  return '\n\n## Design state (measured after the last applied change — use it to verify fits and proportions)\n' + lines.join('\n');
}

async function captureDesignState(ron) {
  const invoke = tauriInvoke();
  if (!invoke) return null;
  try {
    const [ev, tables] = await Promise.all([
      invoke('evaluate_vehicle', { vehicleRon: wrapRon(ron) }),
      invoke('describe_vehicle', { vehicleRon: wrapRon(ron) }),
    ]);
    return formatDesignState(ev, tables);
  } catch {
    return null;
  }
}

function addAgentLine(text, cls) {
  agentLogLines.push({ text, cls: cls || '' });
  renderAgentLog();
}

function renderAgentLog() {
  let logEl = chatMessagesEl.querySelector('.agent-log');
  if (agentLogLines.length === 0) {
    if (logEl) logEl.remove();
    return;
  }
  if (!logEl) {
    logEl = document.createElement('div');
    logEl.className = 'agent-log';
    chatMessagesEl.appendChild(logEl);
  }
  logEl.innerHTML = '';
  agentLogLines.forEach(l => {
    const div = document.createElement('div');
    div.className = 'agent-line' + (l.cls ? ' ' + l.cls : '');
    div.textContent = l.text;
    logEl.appendChild(div);
  });
  requestAnimationFrame(() => {
    logEl.scrollTop = logEl.scrollHeight;
    chatMessagesEl.scrollTop = chatMessagesEl.scrollHeight;
  });
}

// Parse the model's Plan (JSON `{"todos": [...]}` or RON `Plan(todos: [...])`).
function parsePlan(content) {
  const t = content.trim();
  const jsonMatch = t.match(/```json\n([\s\S]*?)```/s) || (t.startsWith('{') ? [null, t] : null);
  if (jsonMatch) {
    try {
      const obj = JSON.parse(jsonMatch[1]);
      const todos = (obj.todos || []).map(s => String(s)).filter(s => s.trim());
      if (todos.length) return todos;
    } catch {}
  }
  const ronMatch = t.match(/todos\s*:\s*\[([\s\S]*?)\]/s);
  if (ronMatch) {
    const items = ronMatch[1].match(/"([^"\\]|\\.)*"/g) || [];
    const todos = items.map(s => s.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, '\\')).filter(s => s.trim());
    if (todos.length) return todos;
  }
  return null;
}

// Turn a model response into { patchRon } (a Patch) or { wholeDoc } (a full
// document, when the model ignores the patch contract). Throws on JSON that
// fails schema validation.
function normalizePatchRon(ron) {
  let t = ron.trim();
  if (t.startsWith('Patch::')) t = t.slice('Patch::'.length).trim();
  if (t.startsWith('Patch(') && t.endsWith(')')) t = t.slice('Patch('.length, -1).trim();
  return t;
}

// Design step: the model must produce a FULL document (Vehicle), never a
// Patch. Reuses the patch-extraction paths; anything that looks like a Patch
// is an error so the generation loop can retry with that feedback.
async function extractWholeDoc(content) {
  const parsed = await contentToPatchRon(content);
  if (parsed && parsed.wholeDoc) return parsed.wholeDoc;
  if (parsed && parsed.patchRon) {
    const t = parsed.patchRon.trim();
    if (!isPatchRon(t)) return t; // fenced/unfenced whole document
    throw new Error('model emitted a Patch instead of a full design document');
  }
  throw new Error('could not extract a design document from the model response');
}

async function contentToPatchRon(content) {
  const ronMatch = content.match(/```ron\n([\s\S]*?)```/s);
  if (ronMatch) return { patchRon: normalizePatchRon(ronMatch[1]) };
  const t = content.trim();
  const jsonMatch = content.match(/```json\n([\s\S]*?)```/s);
  const jsonText = jsonMatch ? jsonMatch[1].trim() : (t.startsWith('{') ? t : null);
  if (jsonText) {
    const invoke = tauriInvoke();
    let ron = null;
    try {
      ron = await invoke('json_to_ron', { jsonStr: jsonText });
    } catch (e) {
      throw new Error('Constrained response was JSON but failed validation: ' + e);
    }
    return { patchRon: normalizePatchRon(ron) };
  }
  if (isPatchRon(normalizePatchRon(t))) return { patchRon: normalizePatchRon(t) };
  if (t.startsWith('Vehicle(') || t.startsWith('Component(')) return { wholeDoc: t };
  const genericMatch = content.match(/```(?:\w+)?\n([\s\S]*?)```/s);
  if (genericMatch) return { patchRon: normalizePatchRon(genericMatch[1]) };
  return null;
}

// A whole-document response is only adopted if it actually parses as a
// Vehicle or Component — otherwise the model's junk would clobber the editor.
// On failure the offending text is attached so the repair prompt can see it.
async function validateDocumentRon(ron) {
  const invoke = tauriInvoke();
  try {
    await invoke('describe_vehicle', { vehicleRon: wrapRon(ron) });
  } catch (e) {
    const err = new Error('response is not a valid Vehicle or Component document: ' + errToMessage(e));
    err.patchRon = ron;
    throw err;
  }
}

// Apply a Patch to the current document; returns the new RON or throws.
// The error carries the validation issues and the rejected patch so the
// repair loop can diagnose the failure.
async function applyPatchRonToEditor(patchRon, baseRon) {
  const wasComp = isComponentRon(baseRon);
  // Structural patches change the component set: in component-editor mode the
  // result is a genuine multi-component Vehicle, so the editor must be
  // promoted to Vehicle mode rather than unwrapped into invalid bare text.
  const structural = /^(AddComponent|RemoveComponent|ReorderComponents)\s*\(/.test(patchRon.trim());
  const invoke = tauriInvoke();
  const res = await invoke('apply_patch_ron', {
    vehicleRon: wrapRon(baseRon),
    patchRon,
  });
  if (!res.success || !res.vehicle_ron) {
    const msg = (res.issues || []).map(i => i.message).join('; ');
    const e = new Error('Patch rejected: ' + (msg || 'validation failed'));
    e.issues = res.issues || [];
    e.patchRon = patchRon;
    throw e;
  }
  return wasComp && !structural ? unwrapRon(res.vehicle_ron, true) : res.vehicle_ron;
}

async function executeTodo(idx, total, todo, ctx) {
  const label = `[${idx}/${total}] ${todo}`;
  let stepPrompt =
    `Current todo (${idx}/${total}): "${todo}"\n\n` +
    `Changes already applied in this session:\n` +
    (ctx.history.length ? ctx.history.map(h => '- ' + h).join('\n') : '(none)') +
    `\n\nRespond with exactly one Patch. If this todo requires no change to the design, respond with Noop.`;

  for (let attempt = 1; attempt <= MAX_TODO_RETRIES; attempt++) {
    addAgentLine(`▸ ${label}`, 'agent-todo');
    let lastModelContent = '';
    try {
      const { content } = await callAi(ctx.systemPromptBase + PATCH_CONTRACT, stepPrompt, { constrain: 'patch' });
      lastModelContent = content;
      const parsed = await contentToPatchRon(content);
      if (!parsed) throw new Error('could not extract a Patch from the model response');

      if (parsed.wholeDoc) {
        // Model ignored the contract: accept the whole-document replacement.
        await validateDocumentRon(parsed.wholeDoc);
        const prev = editor.value;
        editor.value = parsed.wholeDoc;
                await agentEvaluate();
        lastDesignState = await captureDesignState(editor.value);
        const d = computeDiff(prev, editor.value);
        addAgentLine(`  ✓ replaced whole document (+${d.added}/−${d.removed})`, 'agent-ok');
        ctx.history.push(`replaced whole document`);
        return { applied: true };
      }

      const patch = parsed.patchRon.trim();
      if (patch === 'Noop') {
        addAgentLine(`  ✓ no change needed`, 'agent-ok');
        return { applied: true, noop: true };
      }

      if (!isPatchRon(patch)) {
        // Unfenced whole-document RON: only adopt it if it parses as a
        // Vehicle or Component. Anything else (e.g. `Patch::Noop`, prose,
        // half-written RON) is a failed step -> repair.
        await validateDocumentRon(patch);
        const prev = editor.value;
        editor.value = patch;
                await agentEvaluate();
        lastDesignState = await captureDesignState(editor.value);
        const d = computeDiff(prev, editor.value);
        addAgentLine(`  ✓ applied whole document (+${d.added}/−${d.removed})`, 'agent-ok');
        ctx.history.push(`applied whole document`);
        return { applied: true };
      }

      // Structured edit: apply the Patch and evolve the document in real time.
      const prev = editor.value;
      const updatedRon = await applyPatchRonToEditor(patch, prev);
      editor.value = updatedRon;
            await agentEvaluate();
      lastDesignState = await captureDesignState(updatedRon);
      const d = computeDiff(prev, updatedRon);
      const oneLine = patch.split('\n')[0].trim().replace(/\s+/g, ' ');
      const short = oneLine.length > 90 ? oneLine.slice(0, 90) + '…' : oneLine;
      addAgentLine(`  ✓ applied ${short} (+${d.added}/−${d.removed})`, 'agent-ok');
      ctx.history.push(short);
      return { applied: true };
    } catch (err) {
      const msg = errToMessage(err);
      if (msg.startsWith('Failed to parse vehicle:')) {
        // The BASE document is unparseable (apply_patch_ron parses it first) —
        // no patch can fix that; retrying would just repeat the same failure.
        addAgentLine(`  ✗ base document is not parseable (${msg.slice(0, 100)}) — aborting session`, 'agent-err');
        return { applied: false, error: 'The current document is not parseable: ' + msg };
      }
      const rejected = err.patchRon ? `\nThe offending output was:\n\`\`\`\n${err.patchRon}\n\`\`\`` : '';
      const modelSnippet = lastModelContent ? `\nModel output (first 200 chars): ${lastModelContent.slice(0, 200).replace(/\s+/g, ' ')}` : '';
      const issues = (err.issues || []).map(i => `- ${i.message}`).join('\n');
      if (attempt < MAX_TODO_RETRIES) {
        addAgentLine(`  ✗ ${msg} — retrying`, 'agent-err');
        stepPrompt +=
          `\n\nYour previous attempt failed:\nError: ${msg}\n` +
          rejected +
          (issues ? `\nValidation issues:\n${issues}` : '') +
          `\n\nDiagnose the error and produce a corrected Patch that applies cleanly.`;
        if (attempt === MAX_TODO_RETRIES - 1) {
          stepPrompt += '\n\nThis is your LAST attempt. Do not repeat the mistake above.';
        }
      } else {
        addAgentLine(`  ✗ ${msg} — giving up after ${MAX_TODO_RETRIES} attempts${modelSnippet ? modelSnippet : ''}`, 'agent-err');
        return { applied: false, error: msg };
      }
    }
  }
  return { applied: false, error: 'internal retry loop error' };
}

// Review pass: after all todos, the model gets one last look at the finished
// document. Validation issues (errors AND warnings) are surfaced and corrected
// via PatchList, capped at MAX_REVIEW_ROUNDS. A final optimization round then
// asks the model to improve the design (parameterization, op cleanup, fit).
async function runReviewPass(ctx) {
  for (let round = 1; round <= MAX_REVIEW_ROUNDS; round++) {
    const invoke = tauriInvoke();
    let ev = null;
    try {
      ev = await invoke('evaluate_vehicle', { vehicleRon: wrapRon(editor.value) });
    } catch {
      return;
    }
    const issues = (ev.issues || []).filter(i => {
      const s = (i.severity || '').toLowerCase();
      return s === 'warning' || s === 'error';
    });
    if (issues.length === 0) break;

    addAgentLine(`review ${round}/${MAX_REVIEW_ROUNDS}: ${issues.length} issue(s) to address`, 'agent-plan');
    const reviewPrompt =
      'Review pass: the current design has these validation issues:\n' +
      issues.map(i => `- [${i.severity}] ${i.message}`).join('\n') +
      '\n\nEmit ONE Patch — preferably a PatchList — that corrects as many as possible ' +
      'without breaking the design. If an issue is acceptable for this design, emit Noop.';
    try {
      const { content } = await callAi(ctx.systemPromptBase + PATCH_CONTRACT, reviewPrompt, { constrain: 'patch' });
      const parsed = await contentToPatchRon(content);
      const patch = parsed && parsed.patchRon ? parsed.patchRon.trim() : null;
      if (!patch || patch === 'Noop' || !isPatchRon(patch)) {
        addAgentLine('  ✓ review: nothing to change', 'agent-ok');
        break;
      }
      const prev = editor.value;
      const updatedRon = await applyPatchRonToEditor(patch, prev);
      if (updatedRon === prev) {
        addAgentLine('  ✓ review: no effective change', 'agent-ok');
        break;
      }
      editor.value = updatedRon;
      await agentEvaluate();
      lastDesignState = await captureDesignState(updatedRon);
      const d = computeDiff(prev, updatedRon);
      addAgentLine(`  ✓ review applied corrections (+${d.added}/−${d.removed})`, 'agent-ok');
      ctx.history.push(`review correction: ${patch.split('\n')[0].trim()}`);
      const fresh = await buildSystemPrompt('', { retrieve: false });
      ctx.systemPromptBase = fresh.prompt;
    } catch (err) {
      addAgentLine(`  ✗ review correction rejected (${errToMessage(err)}) — stopping`, 'agent-warn');
      break;
    }
  }

  // Optimization round: even a clean design can be improved. The model reviews
  // its own work and emits one final improvement Patch, or Noop.
  addAgentLine('optimize: final quality pass', 'agent-plan');
  const optPrompt =
    'Optimization pass: review the finished design for quality.\n' +
    '- Repeated numeric relationships that should become Parameters equations (e.g. body_od, wall, body_id = "body_od - 2 * wall").\n' +
    '- Redundant or mergeable Solid ops; components that could be repositioned for proper mating/fit.\n' +
    '- Any dimension, radius or spacing that seems off for the request.\n' +
    'Emit ONE Patch — preferably a PatchList — with the improvements, or Noop if the design is already good.';
  try {
    const { content } = await callAi(ctx.systemPromptBase + PATCH_CONTRACT, optPrompt, { constrain: 'patch' });
    const parsed = await contentToPatchRon(content);
    const patch = parsed && parsed.patchRon ? parsed.patchRon.trim() : null;
    if (!patch || patch === 'Noop' || !isPatchRon(patch)) {
      addAgentLine('  ✓ optimize: design already good', 'agent-ok');
      return;
    }
    const prev = editor.value;
    const updatedRon = await applyPatchRonToEditor(patch, prev);
    if (updatedRon === prev) {
      addAgentLine('  ✓ optimize: no effective change', 'agent-ok');
      return;
    }
    editor.value = updatedRon;
    await agentEvaluate();
    lastDesignState = await captureDesignState(updatedRon);
    const d = computeDiff(prev, updatedRon);
    addAgentLine(`  ✓ optimize applied (+${d.added}/−${d.removed})`, 'agent-ok');
  } catch (err) {
    addAgentLine(`  ✗ optimize rejected (${errToMessage(err)}) — keeping the current design`, 'agent-warn');
  }
}

async function runAgentSession(userPrompt, originalRon) {
  addAgentLine(`thinking about "${userPrompt.length > 70 ? userPrompt.slice(0, 70) + '…' : userPrompt}"…`, 'agent-think');
  lastDesignState = null;

  // 0) Preflight: is there a usable document to edit inline?
  const pfInvoke = tauriInvoke();
  let hasDoc = false;
  if (pfInvoke) {
    try {
      await pfInvoke('describe_vehicle', { vehicleRon: getEditorRon() });
      hasDoc = true;
    } catch {}
  } else {
    hasDoc = /Component\s*\(/.test(getEditorRon());
  }

  // @-mentions of existing parts LOCK the session into inline editing —
  // regenerating would destroy exactly what the user pointed at.
  const mentionedNames = [...new Set([...userPrompt.matchAll(/@([\w-]+)/g)].map(m => m[1]))];
  if (hasDoc && mentionedNames.length > 0) {
    addAgentLine(`@-mentions (${mentionedNames.join(', ')}) → inline edit mode locked`, 'agent-plan');
  }

  // Full-document generation. Used ONLY when there is no editable document,
  // or the planner explicitly routes to __regenerate__.
  async function generateFullDesign(desc) {
    const genBase = await buildSystemPrompt(userPrompt, { retrieve: false });
    for (let attempt = 1; attempt <= MAX_TODO_RETRIES; attempt++) {
      addAgentLine(`▸ designing from scratch (attempt ${attempt}/${MAX_TODO_RETRIES})`, 'agent-todo');
      try {
        const { content } = await callAi(genBase.prompt + DESIGN_CONTRACT, desc || userPrompt, { constrain: 'vehicle', temperature: 0.4 });
        const designRon = await extractWholeDoc(content);
        await validateDocumentRon(designRon);
        const prev = editor.value;
        editor.value = designRon;
        await agentEvaluate();
        lastDesignState = await captureDesignState(editor.value);
        const d0 = computeDiff(prev, editor.value);
        addAgentLine(`✓ generated new design from scratch (+${d0.added}/−${d0.removed})`, 'agent-ok');
        return true;
      } catch (err) {
        const msg = errToMessage(err);
        if (attempt >= MAX_TODO_RETRIES) {
          addAgentLine(`  ✗ design generation failed after ${MAX_TODO_RETRIES} attempts — aborting`, 'agent-err');
          throw new Error('The AI could not produce a valid design: ' + msg);
        }
        addAgentLine(`  ✗ ${msg} — retrying with the error`, 'agent-err');
      }
    }
    return false;
  }

  // 1) No usable base document → generation is the only sensible path.
  if (!hasDoc) await generateFullDesign(userPrompt);

  // 2) Planning: inline-edit todos by default; the planner routes explicit
  // redesigns via a single __regenerate__ todo.
  const base = await buildSystemPrompt(userPrompt); // inventory + library hits
  let todos = null;
  let regenerated = false;
  try {
    const { content } = await callAi(base.prompt + PLAN_CONTRACT, userPrompt, { constrain: 'plan', temperature: 0.3 });
    todos = parsePlan(content);
  } catch (err) {
    addAgentLine(`✗ planning failed: ${err.message}`, 'agent-err');
    throw new Error('Planning failed: ' + err.message);
  }

  // 3) Route: honor explicit regeneration ONLY without @-mentions of existing
  // parts; otherwise force the model back onto inline edits.
  if (todos && todos.length === 1 && /^__regenerate__\b/i.test(todos[0].trim())) {
    const desc = todos[0].replace(/^__regenerate__\s*:\s*/i, '').trim();
    if (mentionedNames.length > 0 || hasDoc === false) {
      if (mentionedNames.length > 0) {
        addAgentLine('regeneration refused — @-mentions target existing parts', 'agent-warn');
        todos = null;
        try {
          const fix = await callAi(
            base.prompt + PLAN_CONTRACT +
              '\n\nSTRICT OVERRIDE: do NOT emit __regenerate__. The request references existing components — emit only inline edit todos.',
            userPrompt,
            { constrain: 'plan', temperature: 0.3 }
          );
          const refixed = (parsePlan(fix.content) || []).filter(t => !/^__regenerate__\b/i.test(t.trim()));
          if (refixed.length > 0) todos = refixed;
        } catch {}
        if (!todos || todos.length === 0) {
          addAgentLine('no actionable inline todos', 'agent-warn');
          return { todos: [], failed: 0, failures: [], hitIds: base.hitIds };
        }
      }
    } else {
      addAgentLine(`router: explicit redesign → regenerating ("${desc.slice(0, 60)}")`, 'agent-plan');
      await generateFullDesign(desc);
      regenerated = true;
      todos = [];
    }
  }

  if ((!todos || todos.length === 0) && !regenerated) {
    addAgentLine('model returned no todos — design already satisfies the request', 'agent-warn');
    return { todos: [], failed: 0, failures: [], hitIds: base.hitIds };
  }
  if (!todos) todos = [];
  todos = todos.slice(0, MAX_TODOS);
  if (todos.length > 0) {
    addAgentLine(`plan: ${todos.length} todo${todos.length > 1 ? 's' : ''}`, 'agent-plan');
  }

  // 4) Execute each todo inline, evolving the document in real time. Each step
  // stops and fixes errors (retries inside executeTodo), and the camera
  // auto-fits after every applied change.
  const ctx = { systemPromptBase: base.prompt, history: [] };
  let failed = 0;
  const failures = [];
  for (let i = 0; i < todos.length; i++) {
    // Refresh the inventory after every change so the model sees the live state.
    const fresh = await buildSystemPrompt(userPrompt, { retrieve: false });
    ctx.systemPromptBase = fresh.prompt;
    const result = await executeTodo(i + 1, todos.length, todos[i], ctx);
    if (!result.applied) {
      failed++;
      failures.push({ todo: todos[i], error: result.error || 'unknown error' });
    }
    // Brief pause between todos to avoid rate-limit bursts.
    if (i < todos.length - 1) await new Promise(r => setTimeout(r, 1500));
  }

  // 5) Fix pass: any todo that failed gets a dedicated stop-and-fix round
  // against the live document (which changed since the original attempt).
  if (failures.length > 0) {
    addAgentLine(`fix pass: ${failures.length} failed todo(s) get a dedicated repair round`, 'agent-plan');
    const fresh = await buildSystemPrompt(userPrompt, { retrieve: false });
    ctx.systemPromptBase = fresh.prompt;
    let fixed = 0;
    for (let i = 0; i < failures.length; i++) {
      const f = failures[i];
      const result = await executeTodo(i + 1, failures.length, `Fix: ${f.todo} (previous error: ${f.error})`, ctx);
      if (result.applied) fixed++;
    }
    if (fixed > 0) {
      failed -= fixed;
      addAgentLine(`fix pass: repaired ${fixed}/${failures.length}`, 'agent-ok');
    }
  }

  if (todos.length > 0) {
    const done = todos.length - failed;
    addAgentLine(`done: ${done}/${todos.length} todos completed`, failed ? 'agent-warn' : 'agent-ok');
  }

  // 6) Review + optimize pass: validate what was made and improve it.
  await runReviewPass(ctx);

  // The Rust serializer emits compact single-line RON; restore readable
  // formatting so the final document matches how the user writes it.
  const t = editor.value.trim();
  if (t.startsWith('Vehicle(') || t.startsWith('Component(')) {
    editor.value = formatRon(editor.value);
  }
  return { todos, failed, failures, hitIds: base.hitIds, regenerated };
}

// AI send
const chatCounterEl = document.getElementById('ux-chat-counter');
const chatSuggestionsEl = document.getElementById('chat-suggestions');

function setChatBusy(busy) {
  aiChatBusy = busy;
  aiSendBtn.disabled = busy;
  aiPrompt.disabled = busy;
  aiSendBtn.classList.toggle('busy', busy);
  const glyph = aiSendBtn.querySelector('.send-glyph');
  if (glyph) glyph.textContent = busy ? '…' : '➤';
  const statusDot = document.getElementById('ux-chat-status');
  if (statusDot) statusDot.classList.toggle('busy', busy);
  if (!busy && chatSuggestionsEl && chatMessages.length === 0) {
    chatSuggestionsEl.style.display = 'flex';
  }
}

function autoGrowPrompt() {
  aiPrompt.style.height = 'auto';
  aiPrompt.style.height = Math.min(Math.max(aiPrompt.scrollHeight, 64), 220) + 'px';
  if (chatCounterEl) chatCounterEl.textContent = aiPrompt.value.length;
  if (chatSuggestionsEl) {
    chatSuggestionsEl.style.display = aiPrompt.value.trim() || chatMessages.length ? 'none' : 'flex';
  }
}

// ===== @ mentions: blue tokens + component picker =====
const promptHl = document.getElementById('ai-prompt-hl');
const atPop = document.getElementById('at-mention-pop');

function escapeHl(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// Render the overlay: normal ink for text, blue for @tokens.
function highlightPrompt() {
  if (!promptHl) return;
  const t = aiPrompt.value;
  let out = '';
  let last = 0;
  const re = /@[\w-]+/g;
  let m;
  while ((m = re.exec(t))) {
    out += escapeHl(t.slice(last, m.index));
    out += `<span class="at-token">${escapeHl(m[0])}</span>`;
    last = m.index + m[0].length;
  }
  out += escapeHl(t.slice(last));
  promptHl.innerHTML = out + '\n';
}

function syncPromptScroll() {
  if (!promptHl) return;
  promptHl.scrollTop = aiPrompt.scrollTop;
  promptHl.scrollLeft = aiPrompt.scrollLeft;
}

let atItems = [];
let atSelIdx = -1;

function componentNamesForMentions() {
  const names = [];
  const re = /Component\(\s*name:\s*"([^"]+)"/g;
  let m;
  while ((m = re.exec(getEditorRon()))) names.push(m[1]);
  return [...new Set(names)];
}

function hideAtPop() {
  atItems = [];
  atSelIdx = -1;
  if (atPop) atPop.style.display = 'none';
}

function renderAtPop(query) {
  if (!atPop) return;
  const q = query.toLowerCase();
  const all = componentNamesForMentions();
  atItems = all.filter(n => !q || n.toLowerCase().includes(q)).slice(0, 8);
  atSelIdx = atItems.length ? 0 : -1;
  if (atItems.length === 0) {
    atPop.innerHTML = '<div class="at-empty">No matching components</div>';
  } else {
    atPop.innerHTML = atItems
      .map((n, i) => `<div class="at-item${i === atSelIdx ? ' sel' : ''}" data-i="${i}"><span class="at-icon">@</span><span>${escapeHl(n)}</span></div>`)
      .join('');
    atPop.querySelectorAll('.at-item').forEach(el => {
      el.addEventListener('mousedown', e => {
        e.preventDefault();
        chooseAtMention(atItems[parseInt(el.dataset.i, 10)]);
      });
    });
  }
  atPop.style.display = 'block';
}

// Detect "@partial" immediately before the caret.
function activeAtQuery() {
  const pos = aiPrompt.selectionStart ?? 0;
  const before = aiPrompt.value.slice(0, pos);
  const m = before.match(/(^|[\s(])@([\w-]*)$/);
  return m ? m[2] : null;
}

function updateAtPop() {
  const q = activeAtQuery();
  if (q === null) hideAtPop();
  else renderAtPop(q);
}

function chooseAtMention(name) {
  if (!name) { hideAtPop(); return; }
  const pos = aiPrompt.selectionStart ?? 0;
  const before = aiPrompt.value.slice(0, pos);
  const after = aiPrompt.value.slice(pos);
  const m = before.match(/@([\w-]*)$/);
  if (!m) { hideAtPop(); return; }
  const start = pos - m[0].length;
  const insertion = '@' + name + ' ';
  aiPrompt.value = aiPrompt.value.slice(0, start) + insertion + after;
  const newPos = start + insertion.length;
  aiPrompt.setSelectionRange(newPos, newPos);
  hideAtPop();
  highlightPrompt();
  autoGrowPrompt();
  aiPrompt.focus();
}

if (atPop) {
  // Runs BEFORE the send handler below: when the picker is open it owns
  // navigation keys, so Enter picks instead of sending.
  aiPrompt.addEventListener('keydown', e => {
    if (atPop.style.display === 'none' || atItems.length === 0) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      e.stopImmediatePropagation();
      atSelIdx = (atSelIdx + (e.key === 'ArrowDown' ? 1 : -1) + atItems.length) % atItems.length;
      atPop.querySelectorAll('.at-item').forEach((el, i) => el.classList.toggle('sel', i === atSelIdx));
    } else if (e.key === 'Enter' && !e.ctrlKey && !e.metaKey || e.key === 'Tab') {
      e.preventDefault();
      e.stopImmediatePropagation();
      chooseAtMention(atItems[atSelIdx]);
    } else if (e.key === 'Escape') {
      e.stopImmediatePropagation();
      hideAtPop();
    }
  });
}

aiPrompt.addEventListener('input', () => { highlightPrompt(); updateAtPop(); });
aiPrompt.addEventListener('click', updateAtPop);
aiPrompt.addEventListener('keyup', updateAtPop);
aiPrompt.addEventListener('scroll', syncPromptScroll);
aiPrompt.addEventListener('blur', () => setTimeout(hideAtPop, 120));

aiPrompt.addEventListener('input', autoGrowPrompt);
aiPrompt.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
    e.preventDefault();
    aiSendBtn.click();
  }
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    aiSendBtn.click();
  }
});

const chatClearBtn = document.getElementById('ux-chat-clear');
if (chatClearBtn) {
  chatClearBtn.addEventListener('click', () => {
    // Clear IN PLACE so chatMessages stays bound to the active conversation.
    chatMessages.length = 0;
    agentLogLines = [];
    msgCounter = 0;
    renderChatMessages();
    renderAgentLog();
    if (chatSuggestionsEl) chatSuggestionsEl.style.display = 'flex';
    window.__ux?.notify?.('Conversation cleared', 'info', 1500);
  });
}

// ===== @-mention -> AI context injection =====
let mentionContextBlock = '';

// Extracts @Names from the prompt and builds a data block describing those
// components, consumed by buildSystemPrompt so EVERY AI call in the session
// (design, plan, todos, review) sees them.
async function collectMentionContext(prompt) {
  mentionContextBlock = '';
  const names = [...new Set([...prompt.matchAll(/@([\w-]+)/g)].map(m => m[1]))];
  if (names.length === 0) return;
  const invoke = tauriInvoke();
  if (!invoke) return;
  let tables = null;
  try { tables = await invoke('describe_vehicle', { vehicleRon: getEditorRon() }); } catch { return; }
  const found = (tables || []).filter(t => names.some(n => t.name.toLowerCase() === n.toLowerCase()));
  if (found.length === 0) return;
  const lines = found.map(t => {
    const rows = t.rows.map(r => `  - ${r.key} = ${r.value} (${r.field_type})`).join('\n');
    return `- ${t.name} (${t.kind}${t.material ? `, material: ${t.material}` : ''}${t.color ? `, color: ${t.color}` : ''}):\n${rows}`;
  });
  mentionContextBlock =
    '\n\n## @-mentioned components (the user explicitly referenced these)\n' +
    lines.join('\n') +
    '\nThe user @-mentioned these components by name — prioritize your changes on them.';
}

if (chatSuggestionsEl) {
  chatSuggestionsEl.querySelectorAll('.sug-chip').forEach(chip => {
    chip.addEventListener('click', () => {
      aiPrompt.value = chip.dataset.prompt;
      autoGrowPrompt();
      aiPrompt.focus();
    });
  });
}

aiSendBtn.addEventListener('click', async () => {
  if (aiChatBusy) return;

  const userPrompt = aiPrompt.value.trim();
  if (!userPrompt) return;

  const originalRon = editor.value;

  // First prompt of a fresh conversation names it.
  const conv = activeConv();
  if (conv && !conv.name) {
    const firstLine = userPrompt.split('\n')[0].trim();
    setConversationName(firstLine.length > 42 ? firstLine.slice(0, 42) + '…' : firstLine);
  }

  // Resolve @-mentioned components into AI-readable data for every call.
  await collectMentionContext(userPrompt);

  // Add user message
  addChatMessage({ role: 'user', content: userPrompt, mode: aiMode });

  // Add animated thinking bubble
  thinkingMsgId = addChatMessage({ role: 'thinking', content: '', mode: aiMode });

  setChatBusy(true);

  try {
    aiUnconstrained = false;
    updateConstraintBadge();
    agentLogLines = [];
    renderAgentLog();

    if (aiMode === 'plan') {
      // Plan mode: single free-text analysis call (unchanged).
      const { prompt: systemPrompt } = await buildSystemPrompt(userPrompt);
      const { content: response } = await callAi(systemPrompt, userPrompt, { constrain: 'none' });
      if (thinkingMsgId) { removeChatMessage(thinkingMsgId); thinkingMsgId = null; }
      addChatMessage({ role: 'assistant', content: response, mode: 'plan', todos: parsePlan(response) || undefined });
    } else {
      // Edit mode: agent loop — plan todos, execute each one, evolving the
      // document in real time, repairing errors before moving on.
      if (thinkingMsgId) { removeChatMessage(thinkingMsgId); thinkingMsgId = null; }
      const session = await runAgentSession(userPrompt, originalRon);
      const finalRon = editor.value;
      const diff = originalRon ? computeDiff(originalRon, finalRon) : null;
      const done = session.todos.length - (session.failed || 0);
      let summary = session.regenerated
        ? 'Generated a brand-new design' + (diff ? ` (+${diff.added}/−${diff.removed} lines).` : '.')
        : session.todos.length === 0
          ? 'No changes needed — the design already satisfies the request.'
          : `Completed ${done}/${session.todos.length} todos.` +
            (diff ? ` Net change: +${diff.added}/−${diff.removed} lines.` : '');
      if (session.failures && session.failures.length > 0) {
        summary += '\n\n**Failed todos** (gave up after ' + MAX_TODO_RETRIES + ' attempts each):\n' +
          session.failures.map(f => `- ${f.todo} — ${f.error}`).join('\n');
      }

      addChatMessage({
        role: 'assistant',
        content: summary,
        ron: finalRon,
        ronOriginal: originalRon,
        ronDiff: diff || { added: 0, removed: 0 },
        ronSummary: summary,
        status: 'pending',
        mode: 'edit',
        todos: session.todos.length ? session.todos : undefined,
        libraryHitIds: session.hitIds,
        constrained: true,
      });

      // Switch to chat tab to show the result
      switchTab('chat');
    }

    // Clear prompt
    aiPrompt.value = '';
    highlightPrompt();
    hideAtPop();
    autoGrowPrompt();
  } catch (err) {
    if (thinkingMsgId) { removeChatMessage(thinkingMsgId); thinkingMsgId = null; }
    addChatMessage({ role: 'assistant', content: '**Error:** ' + String(err), mode: aiMode });
  } finally {
    mentionContextBlock = '';
    setChatBusy(false);
    aiPrompt.focus();
  }
});

// Ctrl+Enter sends to AI when AI prompt is focused
aiPrompt.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    aiSendBtn.click();
  }
});

// ===== Keyboard shortcuts =====
document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key === 'Enter') { e.preventDefault(); evaluate(); }
  if (e.ctrlKey && e.key === 'z' && !e.shiftKey) { e.preventDefault(); undo(); }
  if (e.ctrlKey && (e.key === 'y' || (e.key === 'z' && e.shiftKey))) { e.preventDefault(); redo(); }
  if (e.ctrlKey && e.key === 'f') { e.preventDefault(); formatBtn.click(); }
});

// ===== Animation loop =====
function animate() {
  requestAnimationFrame(animate);
  controls.update();
  renderer.render(scene, camera);
}

// Initial load
setTimeout(() => {
  updateSize();
  evaluate();
}, 50);
updateKindBadge();
animate();

// Boot diagnostics: module fully evaluated.
if (window.__boot) window.__boot.push('main: fully loaded');
