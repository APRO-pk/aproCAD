// ============================================================
// RON editor syntax layer: highlighting + code folding +
// selection dimming, built as an overlay behind the textarea.
//
// Architecture ("shadow model"):
//  - `fullLines` holds the REAL document lines (source of truth).
//  - The textarea shows DISPLAY lines; a folded region collapses into one
//    placeholder line crafted to keep heuristic consumers (feature tree)
//    working. All programmatic readers go through getEditorRon().
//  - A <pre> layer under the transparent-text textarea renders the colored,
//    optionally dimmed view of the display lines, scroll-synced.
//
// createSyntaxEditor(textarea) builds an independent instance, so secondary
// editors (e.g. the library "new component" screen) behave exactly like the
// main one. initSyntaxEditor() is the compatibility wrapper for the main
// document editor.
// ============================================================

// ---------- tokenization ----------
const escHtml = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

const TOKEN_RE = new RegExp(
  [
    '(\\/\\/.*$)', // 1 comment
    '("(?:[^"\\\\]|\\\\.)*")', // 2 string
    '(-?\\b\\d+(?:\\.\\d+)?(?:[eE][+-]?\\d+)?\\b)', // 3 number
    '\\b(Some|None|true|false)\\b', // 4 keyword
    '\\b([A-Z]\\w*(?:Params)?)(?=\\s*\\()', // 5 struct/variant call
    '([a-z_]\\w*)(?=\\s*:)', // 6 field key
    '\\b([A-Z]\\w*)\\b', // 7 bare enum variant
    '([()\\[\\]{},:=])', // 8 punctuation
  ].join('|'),
  'g'
);

const TK = {
  1: 'tk-cmt', 2: 'tk-str', 3: 'tk-num', 4: 'tk-kw',
  5: 'tk-type', 6: 'tk-field', 7: 'tk-variant', 8: 'tk-punc',
};

function hlLine(line) {
  let out = '';
  let last = 0;
  TOKEN_RE.lastIndex = 0;
  let m;
  while ((m = TOKEN_RE.exec(line))) {
    out += escHtml(line.slice(last, m.index));
    let cls = 'tk-punc';
    for (let g = 1; g <= 8; g++) {
      if (m[g] !== undefined) { cls = TK[g]; break; }
    }
    out += `<span class="${cls}">${escHtml(m[0])}</span>`;
    last = m.index + m[0].length;
  }
  out += escHtml(line.slice(last));
  return out;
}

// ---------- structure scanning ----------
// Strip strings/comments so paren counting is reliable.
function sanitize(line) {
  let out = '';
  let inStr = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (inStr) {
      if (ch === '\\') { i++; continue; }
      if (ch === '"') inStr = false;
      continue;
    }
    if (ch === '"') { inStr = true; continue; }
    if (ch === '/' && line[i + 1] === '/') break;
    out += ch;
  }
  return out;
}

function computeFolds(lines) {
  const res = new Map();
  const stack = [];
  lines.forEach((raw, li) => {
    for (const ch of sanitize(raw)) {
      if (ch === '(' || ch === '[') stack.push(li);
      else if (ch === ')' || ch === ']') {
        const st = stack.pop();
        if (st !== undefined && li > st) res.set(st, li);
      }
    }
  });
  return res;
}

function computeCompBlocks(lines) {
  const fm = computeFolds(lines);
  const blocks = [];
  lines.forEach((l, i) => {
    if (!/^\s*Component\s*\(/.test(l)) return;
    const end = fm.get(i);
    if (end === undefined) return;
    const seg = lines.slice(i, end + 1).join(' ');
    const nm = seg.match(/name:\s*"([^"]+)"/);
    blocks.push({ name: nm ? nm[1] : null, start: i, end });
  });
  return blocks;
}

function diffRanges(oldArr, newArr) {
  let s = 0;
  const oL = oldArr.length, nL = newArr.length;
  while (s < oL && s < nL && oldArr[s] === newArr[s]) s++;
  let eO = oL, eN = nL;
  while (eO > s && eN > s && oldArr[eO - 1] === newArr[eN - 1]) { eO--; eN--; }
  return { dStart: s, dOldEnd: eO, dNewEnd: eN };
}

// ---------- instance ----------
export function createSyntaxEditor(ta) {
  let fullLines = [];
  let dispEntries = []; // {fold:true,start,end} | {fold:false,src}
  let dispTexts = [];
  let foldable = new Map(); // src start line -> src end line
  let folds = new Set(); // folded source start lines
  let compBlocks = []; // [{name,start,end}]
  let selectedName = null;
  const SYN_DEBUG = true;

  let hlLayer = null;
  let gutter = null;

  function placeholderFor(start, end) {
    const head = fullLines[start].trimEnd();
    const cut = head.indexOf('(');
    const headBase = cut >= 0 ? head.slice(0, cut + 1) : head + '(';
    const seg = fullLines.slice(start, end + 1).join(' ');
    const nm = seg.match(/name:\s*"([^"]+)"/);
    const kd = seg.match(/kind:\s*(\w+)/);
    const n = end - start + 1;
    // name FIRST: the feature-tree heuristic regex expects `Component(name:`.
    const bits = [];
    if (nm) bits.push(`name: "${nm[1]}"`);
    if (kd) bits.push(`kind: ${kd[1]}`);
    const meta = bits.length ? `${bits.join(', ')}, ` : '';
    return `${headBase}${meta}/* \u2937 ${n} lines */ ),`;
  }

  function rebuildDisplay() {
    dispEntries = [];
    dispTexts = [];
    let i = 0;
    while (i < fullLines.length) {
      if (folds.has(i) && foldable.has(i)) {
        const end = foldable.get(i);
        dispEntries.push({ fold: true, start: i, end });
        dispTexts.push(placeholderFor(i, end));
        i = end + 1;
      } else {
        dispEntries.push({ fold: false, src: i });
        dispTexts.push(fullLines[i]);
        i++;
      }
    }
  }

  function dispToSrcRange(i) {
    const e = dispEntries[i];
    if (!e) return { s: Math.max(0, fullLines.length - 1), e: fullLines.length - 1 };
    return e.fold ? { s: e.start, e: e.end } : { s: e.src, e: e.src };
  }

  function lineDimFlags() {
    const flags = new Array(dispEntries.length).fill(false);
    if (!selectedName || selectedName === '__parameters__') return flags;
    const block = compBlocks.find(b => b.name === selectedName);
    if (!block) return flags; // unknown target: keep everything normal
    const inSel = (sl, el) => !(el < block.start || sl > block.end);
    dispEntries.forEach((e, i) => {
      const r = e.fold ? { s: e.start, e: e.end } : { s: e.src, e: e.src };
      flags[i] = !inSel(r.s, r.e);
    });
    return flags;
  }

  function renderHighlight() {
    const flags = lineDimFlags();
    try {
    hlLayer.innerHTML = dispTexts
      .map((t, i) => {
        const html = hlLine(t);
        return flags[i] ? `<span class="ron-dim">${html}</span>` : html;
      })
      .join('\n');
    } catch (e) {
      // Tokenizer failure must NEVER look like an emptied editor.
      console.error('[syn] highlight failed', e);
      hlLayer.textContent = dispTexts.join('\n');
    }
  }

  function renderGutter() {
    gutter.innerHTML = '';
    dispEntries.forEach(e => {
      const row = document.createElement('div');
      row.className = 'gutter-row';
      if (!e.fold && foldable.has(e.src)) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'gutter-fold' + (folds.has(e.src) ? ' closed' : '');
        btn.textContent = folds.has(e.src) ? '\u25B8' : '\u25BE';
        btn.title = 'Toggle section';
        btn.dataset.srcStart = String(e.src);
        row.appendChild(btn);
      } else if (e.fold) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'gutter-fold closed';
        btn.textContent = '\u25B8';
        btn.title = 'Expand section';
        btn.dataset.srcStart = String(e.start);
        row.appendChild(btn);
      }
      gutter.appendChild(row);
    });
    syncScroll();
  }

  function caretDisplayPos() {
    const idx = ta.selectionStart ?? 0;
    const upto = ta.value.slice(0, idx);
    const line = upto.split('\n').length - 1;
    const col = idx - (upto.lastIndexOf('\n') + 1);
    return { line, col };
  }

  function writeTextareaPreserveCaret() {
    const pos = caretDisplayPos();
    const text = dispTexts.join('\n');
    if (ta.value !== text) ta.value = text;
    const line = Math.min(pos.line, dispTexts.length - 1);
    const col = Math.min(pos.col, (dispTexts[line] || '').length);
    let idx = 0;
    for (let i = 0; i < line; i++) idx += dispTexts[i].length + 1;
    idx += col;
    try { ta.setSelectionRange(idx, idx); } catch {}
  }

  function rerenderAll(rewrite) {
    foldable = computeFolds(fullLines);
    compBlocks = computeCompBlocks(fullLines);
    rebuildDisplay();
    if (rewrite) writeTextareaPreserveCaret();
    else {
      const text = dispTexts.join('\n');
      if (ta.value !== text) ta.value = text;
    }
    renderHighlight();
    renderGutter();
  }

  // Source offset just BEFORE display line g (insertion boundary).
  function boundaryBefore(g) {
    if (dispEntries.length === 0 || g <= 0) return 0;
    const e = dispEntries[Math.min(g - 1, dispEntries.length - 1)];
    return e.fold ? e.end + 1 : e.src + 1;
  }

  function onInput() {
    const newTexts = ta.value.split('\n');
    const { dStart, dOldEnd, dNewEnd } = diffRanges(dispTexts, newTexts);
    if (dOldEnd === dNewEnd && dStart >= dOldEnd && dispTexts.length === newTexts.length) {
      renderHighlight(); // caret-only change safety
      return;
    }
    // Map the display-line range to source boundaries. Boundary-based math
    // (not entry clamping!) is what makes pure end-of-document insertions
    // append instead of clobbering line 0.
    const bA = boundaryBefore(Math.min(dStart, dispEntries.length));
    const bB = boundaryBefore(Math.min(dOldEnd, dispEntries.length));
    const srcStart = Math.min(bA, bB);
    const srcEnd = Math.max(bA, bB);
    for (const st of [...folds]) {
      const en = foldable.has(st) ? foldable.get(st) : st;
      if (!(en < srcStart || st > srcEnd)) folds.delete(st);
    }
    const replacement = newTexts.slice(dStart, dNewEnd);
    if (SYN_DEBUG) console.debug('[syn] input', 'd', dStart, dOldEnd, dNewEnd, 'src', srcStart, srcEnd, 'lens', ta.value.length, '->', newTexts.join('\n').length);
    if (newTexts.join('\n').length < ta.value.length * 0.5 && ta.value.length > 20) {
      console.warn('[syn] BIG SHRINK detected — stack:', new Error().stack);
    }
    fullLines.splice(srcStart, srcEnd - srcStart, ...replacement);
    rerenderAll(false);
  }

  function toggleFold(srcStart) {
    if (folds.has(srcStart)) folds.delete(srcStart);
    else folds.add(srcStart);
    rerenderAll(true);
  }

  function syncScroll() {
    if (!hlLayer) return;
    hlLayer.scrollTop = ta.scrollTop;
    hlLayer.scrollLeft = ta.scrollLeft;
    if (gutter) gutter.style.transform = `translateY(${-ta.scrollTop}px)`;
  }

  function attach() {
    ta.setAttribute('wrap', 'off');
    ta.classList.add('syn-ta');

    hlLayer = document.createElement('pre');
    hlLayer.className = 'syn-hl';
    hlLayer.setAttribute('aria-hidden', 'true');

    gutter = document.createElement('div');
    gutter.className = 'syn-gutter';

    const wrapEl = document.createElement('div');
    wrapEl.className = 'syn-wrap';
    ta.parentNode.insertBefore(wrapEl, ta);
    wrapEl.appendChild(hlLayer);
    wrapEl.appendChild(gutter);
    wrapEl.appendChild(ta);

    gutter.addEventListener('click', e => {
      const btn = e.target.closest('.gutter-fold');
      if (!btn) return;
      e.preventDefault();
      toggleFold(parseInt(btn.dataset.srcStart, 10));
    });

    ta.addEventListener('input', onInput);
    ta.addEventListener('scroll', syncScroll);

    // Tab inserts an indent instead of moving focus. With a multi-line
    // selection it block-indents; Shift+Tab outdents. While folds are active
    // block edits fall back to a plain caret insert (placeholder lines must
    // never be rewritten).
    ta.addEventListener('keydown', e => {
      if (e.key !== 'Tab' || e.ctrlKey || e.metaKey || e.altKey) return;
      e.preventDefault();
      const s0 = ta.selectionStart ?? 0;
      const e0 = ta.selectionEnd ?? s0;
      const val = ta.value;

      const commit = (text, ss, se) => {
        ta.value = text;
        try { ta.setSelectionRange(ss, se ?? ss); } catch {}
        onInput();
        // Let external listeners (debounced evaluate, badge...) react too.
        // The factory's own onInput re-runs on this event but its
        // identical-content guard makes that a no-op.
        ta.dispatchEvent(new Event('input', { bubbles: true }));
      };

      const lineStart = val.lastIndexOf('\n', Math.max(0, s0 - 1)) + 1;
      const nlAfter = val.indexOf('\n', e0);
      const lineEnd = nlAfter === -1 ? val.length : nlAfter;
      const multiLine = val.slice(lineStart, e0).includes('\n');

      if (e.shiftKey) {
        const block = val.slice(lineStart, lineEnd);
        const outdented = block.split('\n')
          .map(l => l.replace(/^\t/, '').replace(/^ {1,2}/, ''))
          .join('\n');
        if (outdented === block) return;
        commit(
          val.slice(0, lineStart) + outdented + val.slice(lineEnd),
          lineStart,
          lineStart + outdented.length
        );
        return;
      }

      if (s0 !== e0 && multiLine && folds.size === 0) {
        const block = val.slice(lineStart, lineEnd);
        const indented = block.split('\n').map(l => '\t' + l).join('\n');
        commit(
          val.slice(0, lineStart) + indented + val.slice(lineEnd),
          lineStart,
          lineStart + indented.length
        );
        return;
      }

      // Single-point insert (also the fallback while folds are active).
      commit(val.slice(0, s0) + '\t' + val.slice(e0), s0 + 1, s0 + 1);
    });

    setRon(ta.value);
  }

  function getRon() {
    return fullLines.join('\n');
  }

  function setRon(text) {
    fullLines = String(text).split(/\r?\n/);
    folds.clear();
    rerenderAll(true);
  }

  function setSelected(name) {
    selectedName = name;
    renderHighlight();
  }

  attach();

  return { getRon, setRon, setSelected, focus: () => ta.focus() };
}

// ---------- main-document editor (compatibility singleton) ----------
let mainInst = null;

export function initSyntaxEditor(textarea) {
  mainInst = createSyntaxEditor(textarea);
}

export function getEditorRon() {
  return mainInst ? mainInst.getRon() : '';
}

export function setEditorRon(text) {
  if (mainInst) mainInst.setRon(text);
}

export function selectSyntaxTarget(name) {
  if (mainInst) mainInst.setSelected(name);
}
