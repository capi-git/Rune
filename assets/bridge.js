// Bridge between the page and Rust. Owns the rich-text block editors
// (contenteditable), their keyboard behaviour, inline formatting, links,
// paste, drag and drop, focus, and window dragging; reports events to Rust with
// dioxus.send(). Runs once inside a long-lived eval started by each window.
//
// Block text is stored as inline Markdown (see src/inline.rs). This file mirrors
// that module's `to_md` and `to_html`; keep the two in step.

const send = (message) => {
  try {
    dioxus.send(message);
  } catch (err) {
    console.error('bridge send failed', err);
  }
};

const ZWSP = '​';
const isBlock = (el) => !!(el && el.classList && el.classList.contains('blk'));
const blockOf = (node) => {
  const el = node && (node.nodeType === 1 ? node : node.parentElement);
  return el && el.closest ? el.closest('.blk') : null;
};
const isCode = (el) => el.dataset.kind === 'code';

// ----- inline formatting model (mirrors src/inline.rs) ----------------------

const ORDER = ['bold', 'italic', 'strike'];
const DELIM = { bold: '**', italic: '_', strike: '~~' };
const noMarks = () => ({ bold: false, italic: false, strike: false, code: false });
const sameMarks = (a, b) => a.bold === b.bold && a.italic === b.italic && a.strike === b.strike && a.code === b.code;
const andMarks = (a, b) => ({
  bold: a.bold && b.bold,
  italic: a.italic && b.italic,
  strike: a.strike && b.strike,
  code: a.code && b.code,
});

const merge = (runs) => {
  const out = [];
  for (const r of runs) {
    if (!r.text) continue;
    const last = out[out.length - 1];
    if (last && sameMarks(last.marks, r.marks) && (last.link || null) === (r.link || null)) last.text += r.text;
    else out.push({ text: r.text, marks: { ...r.marks }, link: r.link || null });
  }
  return out;
};

const normalize = (runs) => {
  const merged = merge(runs);
  const split = [];
  merged.forEach((r, i) => {
    if (r.marks.code) {
      split.push(r);
      return;
    }
    const prev = i > 0 ? merged[i - 1].marks : noMarks();
    const next = i + 1 < merged.length ? merged[i + 1].marks : noMarks();
    const strip = (m, o) => ({ ...andMarks(m, o), code: false });
    const trimmedStart = r.text.replace(/^\s+/, '');
    if (!trimmedStart) {
      split.push({ ...r, marks: strip(r.marks, andMarks(prev, next)) });
      return;
    }
    const lead = r.text.slice(0, r.text.length - trimmedStart.length);
    const core = trimmedStart.replace(/\s+$/, '');
    const trail = trimmedStart.slice(core.length);
    if (lead) split.push({ text: lead, marks: strip(r.marks, prev), link: r.link });
    split.push({ text: core, marks: r.marks, link: r.link });
    if (trail) split.push({ text: trail, marks: strip(r.marks, next), link: r.link });
  });
  return merge(split);
};

const escapeText = (s) => s.replace(/[\\*_~`[\]]/g, (c) => '\\' + c);
const escapeCode = (s) => s.replace(/\\/g, '\\\\').replace(/`/g, '\\`');
const encodeUrl = (u) => u.trim().replace(/ /g, '%20').replace(/\(/g, '%28').replace(/\)/g, '%29');

const runsToMd = (input) => {
  const runs = normalize(input);
  let out = '';
  const open = [];
  const emit = (base, run) => {
    while (open.length > base && open.slice(base).some((k) => !run.marks[k])) out += DELIM[open.pop()];
    for (const k of ORDER) {
      if (run.marks[k] && !open.includes(k)) {
        open.push(k);
        out += DELIM[k];
      }
    }
    out += run.marks.code ? '`' + escapeCode(run.text) + '`' : escapeText(run.text);
  };
  const closeTo = (base) => {
    while (open.length > base) out += DELIM[open.pop()];
  };
  let i = 0;
  while (i < runs.length) {
    const url = runs[i].link;
    if (!url) {
      emit(0, runs[i]);
      i++;
      continue;
    }
    let j = i;
    while (j < runs.length && runs[j].link === url) j++;
    const group = runs.slice(i, j);
    const common = group.reduce((acc, r) => andMarks(acc, r.marks), { bold: true, italic: true, strike: true, code: true });
    while (open.some((k) => !common[k])) out += DELIM[open.pop()];
    out += '[';
    const base = open.length;
    for (const r of group) emit(base, r);
    closeTo(base);
    out += '](' + encodeUrl(url) + ')';
    i = j;
  }
  closeTo(0);
  return out;
};

const htmlEscape = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const runHtml = (r) => {
  let s = htmlEscape(r.text);
  if (r.marks.code) s = `<code>${s}</code>`;
  if (r.marks.strike) s = `<s>${s}</s>`;
  if (r.marks.italic) s = `<em>${s}</em>`;
  if (r.marks.bold) s = `<strong>${s}</strong>`;
  return s;
};

const runsToHtml = (runs) => {
  let out = '';
  let i = 0;
  while (i < runs.length) {
    const url = runs[i].link;
    if (!url) {
      out += runHtml(runs[i]);
      i++;
      continue;
    }
    let j = i;
    while (j < runs.length && runs[j].link === url) j++;
    out += `<a href="${htmlEscape(url)}">` + runs.slice(i, j).map(runHtml).join('') + '</a>';
    i = j;
  }
  if (out.endsWith('\n')) out += '<br>';
  return out;
};

const plainOf = (runs) => runs.map((r) => r.text).join('');

// ----- reading the DOM ----------------------------------------------------------

// The last node in document order inside `el`.
const lastLeaf = (el) => {
  let n = el;
  while (n && n.lastChild) n = n.lastChild;
  return n === el ? null : n;
};

const visibleText = (s) => s.replace(/​/g, '').replace(/ /g, ' ');

const domToRuns = (el) => {
  const runs = [];
  const trailing = lastLeaf(el);
  const walk = (node, marks, link) => {
    for (const child of node.childNodes) {
      if (child.nodeType === 3) {
        const text = visibleText(child.nodeValue);
        if (text) runs.push({ text, marks: { ...marks }, link });
        continue;
      }
      if (child.nodeType !== 1) continue;
      const tag = child.tagName;
      if (tag === 'BR') {
        if (child !== trailing) runs.push({ text: '\n', marks: { ...marks }, link });
        continue;
      }
      const m = { ...marks };
      let l = link;
      if (tag === 'STRONG' || tag === 'B') m.bold = true;
      else if (tag === 'EM' || tag === 'I') m.italic = true;
      else if (tag === 'S' || tag === 'STRIKE' || tag === 'DEL') m.strike = true;
      else if (tag === 'CODE') m.code = true;
      else if (tag === 'A') l = child.getAttribute('href') || l;
      const st = child.style;
      if (st) {
        const fw = st.fontWeight;
        if (fw === 'bold' || parseInt(fw, 10) >= 600) m.bold = true;
        if (st.fontStyle === 'italic') m.italic = true;
        if ((st.textDecorationLine || st.textDecoration || '').includes('line-through')) m.strike = true;
      }
      const isBlockEl = tag === 'DIV' || tag === 'P';
      if (isBlockEl && runs.length && !plainOf(runs).endsWith('\n')) runs.push({ text: '\n', marks: noMarks(), link: null });
      walk(child, m, l);
    }
  };
  walk(el, noMarks(), null);
  return merge(runs);
};

const mdOf = (el) => (isCode(el) ? plainOf(domToRuns(el)) : runsToMd(domToRuns(el)));

// Number of visible characters (UTF-16 units) in a DOM fragment.
const fragLen = (root) => {
  let n = 0;
  const walk = (x) => {
    for (const c of x.childNodes) {
      if (c.nodeType === 3) n += visibleText(c.nodeValue).length;
      else if (c.nodeType === 1 && c.tagName === 'BR') n += 1;
      else if (c.nodeType === 1) walk(c);
    }
  };
  walk(root);
  return n;
};

const offsetOf = (el, node, offset) => {
  const r = document.createRange();
  r.setStart(el, 0);
  try {
    r.setEnd(node, offset);
  } catch (_) {
    return 0;
  }
  return fragLen(r.cloneContents());
};

const textLen = (el) => plainOf(domToRuns(el)).length;

// Selection inside a block as visible offsets.
const selIn = (el) => {
  const sel = getSelection();
  if (!sel || !sel.rangeCount) return null;
  const r = sel.getRangeAt(0);
  if (!el.contains(r.startContainer) || !el.contains(r.endContainer)) return null;
  return { start: offsetOf(el, r.startContainer, r.startOffset), end: offsetOf(el, r.endContainer, r.endOffset) };
};

// DOM position for a visible offset. At a boundary it prefers the end of the
// earlier text (so typing continues its formatting, as in most editors).
const locate = (el, target) => {
  let left = target;
  let found = null;
  const walk = (n) => {
    for (let i = 0; i < n.childNodes.length && !found; i++) {
      const c = n.childNodes[i];
      if (c.nodeType === 3) {
        const v = c.nodeValue;
        const len = visibleText(v).length;
        if (left <= len) {
          let raw = 0;
          let seen = 0;
          while (raw < v.length && seen < left) {
            if (v[raw] !== ZWSP) seen++;
            raw++;
          }
          found = { node: c, offset: raw };
          return;
        }
        left -= len;
      } else if (c.nodeType === 1 && c.tagName === 'BR') {
        if (left === 0) {
          found = { node: n, offset: i };
          return;
        }
        left -= 1;
      } else if (c.nodeType === 1) walk(c);
    }
  };
  walk(el);
  return found || { node: el, offset: el.childNodes.length };
};

const setSel = (el, start, end = start) => {
  const a = locate(el, start);
  const b = end === start ? a : locate(el, end);
  const r = document.createRange();
  r.setStart(a.node, a.offset);
  r.setEnd(b.node, b.offset);
  const sel = getSelection();
  sel.removeAllRanges();
  sel.addRange(r);
};

// ----- writing the DOM ------------------------------------------------------

const renderInto = (el, html) => {
  el.innerHTML = html;
  el._init = true;
};

const initBlock = (el) => {
  if (el._init) return;
  renderInto(el, el.dataset.html || '');
};

const initWithin = (root) => {
  if (!root || root.nodeType !== 1) return;
  if (isBlock(root)) initBlock(root);
  else root.querySelectorAll('.blk').forEach(initBlock);
};

// Tell Rust the block's current text.
const commit = (el) => {
  if (!el.textContent && el.querySelector('br') && !el.querySelector('br + *')) el.innerHTML = '';
  send({ t: 'input', page: el.dataset.page, id: el.dataset.id, md: mdOf(el) });
};

const setRuns = (el, runs, start, end = start) => {
  renderInto(el, runsToHtml(runs));
  setSel(el, start, end);
  commit(el);
};

// Split runs so that offsets a and b fall on run edges, then map the middle.
const mapRange = (runs, a, b, f) => {
  const out = [];
  let seen = 0;
  for (const r of runs) {
    const len = r.text.length;
    const s = Math.max(a - seen, 0);
    const e = Math.min(b - seen, len);
    if (e <= 0 || s >= len) out.push(r);
    else {
      if (s > 0) out.push({ ...r, text: r.text.slice(0, s) });
      out.push(f({ ...r, text: r.text.slice(s, e) }));
      if (e < len) out.push({ ...r, text: r.text.slice(e) });
    }
    seen += len;
  }
  return merge(out);
};

const rangeHas = (runs, a, b, test) => {
  let seen = 0;
  let any = false;
  for (const r of runs) {
    const len = r.text.length;
    if (seen + len > a && seen < b && r.text.trim()) {
      any = true;
      if (!test(r)) return false;
    }
    seen += len;
  }
  return any;
};

const deleteRange = (runs, a, b) => mapRange(runs, a, b, (r) => ({ ...r, text: '' }));

// ----- formatting commands ----------------------------------------------------

// The block the caret is in, if any.
const activeBlock = () => {
  const a = document.activeElement;
  return isBlock(a) ? a : null;
};

// With nothing selected, start or stop a format for the next typed text.
const toggleAtCaret = (el, kind) => {
  const sel = getSelection();
  if (!sel.rangeCount) return;
  const range = sel.getRangeAt(0);
  const tags = { bold: 'STRONG', italic: 'EM', strike: 'S', code: 'CODE' };
  let n = range.startContainer.nodeType === 1 ? range.startContainer : range.startContainer.parentElement;
  let inside = null;
  while (n && n !== el) {
    if (n.tagName === tags[kind] || (kind === 'bold' && n.tagName === 'B') || (kind === 'italic' && n.tagName === 'I')) inside = n;
    n = n.parentElement;
  }
  const mark = document.createTextNode(ZWSP);
  if (inside) {
    // Step out: split the formatted element at the caret.
    const tail = document.createRange();
    tail.setStart(range.startContainer, range.startOffset);
    tail.setEnd(inside, inside.childNodes.length);
    const rest = tail.extractContents();
    const clone = inside.cloneNode(false);
    clone.appendChild(rest);
    inside.after(mark);
    if (clone.textContent) mark.after(clone);
  } else {
    const wrap = document.createElement(tags[kind].toLowerCase());
    wrap.appendChild(mark);
    range.insertNode(wrap);
  }
  const r = document.createRange();
  r.setStart(mark, 1);
  r.collapse(true);
  sel.removeAllRanges();
  sel.addRange(r);
};

const toggleMark = (el, kind) => {
  if (isCode(el)) return;
  const s = selIn(el);
  if (!s) return;
  if (s.start === s.end) {
    toggleAtCaret(el, kind);
    return;
  }
  const runs = domToRuns(el);
  const all = rangeHas(runs, s.start, s.end, (r) => r.marks[kind]);
  const next = mapRange(runs, s.start, s.end, (r) => ({ ...r, marks: { ...r.marks, [kind]: !all } }));
  setRuns(el, next, s.start, s.end);
  updateBar();
};

const setLink = (el, start, end, url) => {
  const runs = domToRuns(el);
  const next = mapRange(runs, start, end, (r) => ({ ...r, link: url || null }));
  setRuns(el, next, end);
};

const linkAt = (el, start, end, runs = domToRuns(el)) => {
  let seen = 0;
  for (const r of runs) {
    const len = r.text.length;
    if (r.link && seen < Math.max(end, start + 1) && seen + len > start) return r.link;
    seen += len;
  }
  return null;
};

// Typing `**bold**`, `*italic*`, `_italic_`, `~strike~` or `` `code` `` formats it.
const AUTO = [
  { re: /\*\*([^*\s](?:[^*\n]*[^*\s])?)\*\*$/, kind: 'bold', d: 2 },
  { re: /(?:^|[^*])\*([^*\s](?:[^*\n]*[^*\s])?)\*$/, kind: 'italic', d: 1 },
  { re: /(?:^|[^\w_])_([^_\s](?:[^_\n]*[^_\s])?)_$/, kind: 'italic', d: 1 },
  { re: /(?:^|[^~])~~([^~\s](?:[^~\n]*[^~\s])?)~~$/, kind: 'strike', d: 2 },
  { re: /(?:^|[^~])~([^~\s](?:[^~\n]*[^~\s])?)~$/, kind: 'strike', d: 1 },
  { re: /`([^`\n]+)`$/, kind: 'code', d: 1 },
];

const autoFormat = (el, e) => {
  if (pasting || isCode(el) || e.inputType !== 'insertText' || !e.data || e.isComposing) return false;
  if (!'*_~`'.includes(e.data.slice(-1))) return false;
  const s = selIn(el);
  if (!s || s.start !== s.end) return false;
  const runs = domToRuns(el);
  const before = plainOf(runs).slice(0, s.start);
  for (const rule of AUTO) {
    const m = rule.re.exec(before);
    if (!m) continue;
    const inner = m[1];
    const end = s.start;
    const openAt = end - rule.d * 2 - inner.length;
    let next = deleteRange(runs, end - rule.d, end);
    next = deleteRange(next, openAt, openAt + rule.d);
    next = mapRange(next, openAt, openAt + inner.length, (r) => ({ ...r, marks: { ...r.marks, [rule.kind]: true } }));
    renderInto(el, runsToHtml(next));
    caretAfterFormat(el, openAt + inner.length);
    return true;
  }
  return false;
};

// A web address that was typed becomes a link when a space is typed after
// it. The signs that end a sentence are not part of the address.
const autoLink = (el, e) => {
  if (pasting || isCode(el) || e.inputType !== 'insertText' || e.data !== ' ' || e.isComposing) return false;
  const s = selIn(el);
  if (!s || s.start !== s.end || s.start < 1) return false;
  const runs = domToRuns(el);
  const before = plainOf(runs).slice(0, s.start - 1);
  const m = /(?:^|[\s(])((?:https?:\/\/|www\.)\S*[^\s.,;:!?)\]}'"])([.,;:!?)\]}'"]*)$/i.exec(before);
  if (!m) return false;
  const end = before.length - m[2].length;
  const start = end - m[1].length;
  if (linkAt(el, start, end, runs)) return false;
  const url = /^www\./i.test(m[1]) ? 'https://' + m[1] : m[1];
  renderInto(el, runsToHtml(mapRange(runs, start, end, (r) => ({ ...r, link: url }))));
  setSel(el, s.start);
  return true;
};

// Put the caret just after formatted text, outside the formatting.
const caretAfterFormat = (el, offset) => {
  const at = locate(el, offset);
  let n = at.node.nodeType === 3 && at.offset === at.node.nodeValue.length ? at.node : null;
  let outer = null;
  while (n && n.parentElement && n.parentElement !== el) {
    n = n.parentElement;
    outer = n;
  }
  const sel = getSelection();
  const r = document.createRange();
  if (outer) {
    const mark = document.createTextNode(ZWSP);
    outer.after(mark);
    r.setStart(mark, 1);
  } else {
    r.setStart(at.node, at.offset);
  }
  r.collapse(true);
  sel.removeAllRanges();
  sel.addRange(r);
};

// ----- the floating format bar and link editor ----------------------------------

const bar = () => document.getElementById('fmt-bar');
const linkBox = () => document.getElementById('link-editor');
let linkTarget = null; // { el, start, end }

const hideBar = () => {
  const b = bar();
  if (b && b.classList.contains('show')) b.classList.remove('show');
};

const updateBar = () => {
  const b = bar();
  if (!b || (linkBox() && linkBox().classList.contains('show'))) return;
  const sel = getSelection();
  const el = sel && sel.rangeCount ? blockOf(sel.anchorNode) : null;
  if (!el || sel.isCollapsed || isCode(el) || !blockOf(sel.focusNode) || blockOf(sel.focusNode) !== el) {
    hideBar();
    return;
  }
  const s = selIn(el);
  if (!s || s.start === s.end) {
    hideBar();
    return;
  }
  const runs = domToRuns(el);
  for (const k of ['bold', 'italic', 'strike', 'code']) {
    const btn = b.querySelector(`[data-fmt="${k}"]`);
    if (btn) btn.classList.toggle('on', rangeHas(runs, s.start, s.end, (r) => r.marks[k]));
  }
  const lb = b.querySelector('[data-fmt="link"]');
  if (lb) lb.classList.toggle('on', !!linkAt(el, s.start, s.end, runs));
  const rect = sel.getRangeAt(0).getBoundingClientRect();
  b.classList.add('show');
  const w = b.offsetWidth;
  const left = Math.max(8, Math.min(window.innerWidth - w - 8, rect.left + rect.width / 2 - w / 2));
  const top = rect.top - b.offsetHeight - 8 < 8 ? rect.bottom + 8 : rect.top - b.offsetHeight - 8;
  // (In a zoomed page, its place is in the page's own points.)
  const bz = zoomOf(b);
  b.style.left = left / bz + 'px';
  b.style.top = top / bz + 'px';
};

const openLinkEditor = (el) => {
  const s = selIn(el);
  const box = linkBox();
  if (!s || !box || isCode(el)) return;
  let { start, end } = s;
  if (start === end) {
    // No selection: edit the link under the caret, if any.
    const runs = domToRuns(el);
    let seen = 0;
    for (const r of runs) {
      const len = r.text.length;
      if (r.link && seen <= start && start <= seen + len) {
        start = seen;
        end = seen + len;
        break;
      }
      seen += len;
    }
    if (start === end) return;
  }
  linkTarget = { el, start, end };
  const current = linkAt(el, start, end) || '';
  const rect = getSelection().getRangeAt(0).getBoundingClientRect();
  hideBar();
  box.classList.add('show');
  box.classList.toggle('has-link', !!current);
  const boxZ = zoomOf(box);
  box.style.left = Math.max(8, Math.min(window.innerWidth - box.offsetWidth * boxZ - 8, rect.left)) / boxZ + 'px';
  box.style.top = (rect.bottom + 8) / boxZ + 'px';
  const input = document.getElementById('link-input');
  input.value = current;
  input.focus();
  input.select();
};

const closeLinkEditor = (restore) => {
  const box = linkBox();
  if (box) box.classList.remove('show');
  const t = linkTarget;
  linkTarget = null;
  if (restore && t && document.body.contains(t.el)) {
    t.el.focus({ preventScroll: true });
    setSel(t.el, t.start, t.end);
  }
};

const applyLink = (url) => {
  const t = linkTarget;
  closeLinkEditor(false);
  if (!t || !document.body.contains(t.el)) return;
  let u = (url || '').trim();
  if (u && !/^[a-z][a-z0-9+.-]*:/i.test(u)) u = (u.includes('@') && !u.includes('/') ? 'mailto:' : 'https://') + u;
  t.el.focus({ preventScroll: true });
  setLink(t.el, t.start, t.end, u);
};

const format = (kind) => {
  const el = activeBlock();
  if (!el) return;
  if (kind === 'speak') {
    // The microphone: say what to do with the selected text (the AI does it).
    const s = selIn(el);
    if (s && s.end > s.start) send({ t: 'speak_edit', page: el.dataset.page, id: el.dataset.id, start: s.start, end: s.end });
    hideBar();
  } else if (kind === 'link') openLinkEditor(el);
  else toggleMark(el, kind);
};

document.addEventListener(
  'mousedown',
  (e) => {
    const btn = e.target.closest && e.target.closest('#fmt-bar [data-fmt]');
    if (btn) {
      e.preventDefault(); // keep the text selected
      format(btn.dataset.fmt);
      return;
    }
    const lb = e.target.closest && e.target.closest('#link-editor [data-link]');
    if (lb) {
      e.preventDefault();
      if (lb.dataset.link === 'apply') applyLink(document.getElementById('link-input').value);
      else if (lb.dataset.link === 'remove') applyLink('');
      return;
    }
    if (linkBox() && linkBox().classList.contains('show') && !e.target.closest('#link-editor')) closeLinkEditor(false);
  },
  true,
);

// Selection changes come in bursts; look once per frame.
let barQueued = false;
document.addEventListener('selectionchange', () => {
  if (barQueued) return;
  barQueued = true;
  requestAnimationFrame(() => {
    barQueued = false;
    updateBar();
  });
});

// ----- API used from Rust -----------------------------------------------------

window.__n2 = {
  focus(id, pos) {
    let tries = 0;
    const attempt = () => {
      const el = document.getElementById(id);
      if (!el) {
        if (tries++ < 180) requestAnimationFrame(attempt);
        return;
      }
      initWithin(el);
      el.focus({ preventScroll: true });
      if (isBlock(el)) {
        const len = textLen(el);
        setSel(el, pos < 0 || pos > len ? len : pos);
      } else if (typeof el.setSelectionRange === 'function') {
        const len = el.value.length;
        const p = pos < 0 || pos > len ? len : pos;
        el.setSelectionRange(p, p);
        autosize(el);
      }
      el.scrollIntoView({ block: 'nearest' });
    };
    requestAnimationFrame(attempt);
  },
  // Text changed by Rust (Enter, shortcuts, undo, AI, another window): id -> html.
  setValues(map) {
    for (const [id, html] of Object.entries(map)) {
      const el = document.getElementById(id);
      if (!el) continue; // created in this update; rendered from data-html
      const focused = document.activeElement === el ? selIn(el) : null;
      renderInto(el, html);
      if (focused) {
        const len = textLen(el);
        setSel(el, Math.min(focused.start, len), Math.min(focused.end, len));
      }
    }
  },
  format,
  undo() {
    const a = document.activeElement;
    // A table's cell is part of the page: the app's own undo knows its
    // rows and columns as well as its words.
    const field = a && (a.tagName === 'INPUT' || a.tagName === 'TEXTAREA') && !a.classList.contains('cell');
    if (field) document.execCommand('undo');
    else send({ t: 'undo' });
  },
  redo() {
    const a = document.activeElement;
    const field = a && (a.tagName === 'INPUT' || a.tagName === 'TEXTAREA') && !a.classList.contains('cell');
    if (field) document.execCommand('redo');
    else send({ t: 'redo' });
  },
  sizeAll: () => queueSizeAll(),
  landed: (id) => landed(id),
  // Paste from the short text menu: as a paste would arrive, where the caret is.
  pasteText: (text) => {
    const el = document.activeElement;
    if (!el || !text) return;
    if (isBlock(el)) {
      const data = new DataTransfer();
      data.setData('text/plain', text);
      el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    } else {
      document.execCommand('insertText', false, text);
    }
  },
  find: () => openFind(),
  findStep: (d) => (findOpen() ? findStep(d) : openFind()),
  drawingLoad: (id, json) => drawingLoad(id, json),
  scrollSelected() {
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const el = document.querySelector('.slash-menu .selected');
        if (el) el.scrollIntoView({ block: 'nearest' });
      }),
    );
  },
};

// ----- keyboard ---------------------------------------------------------------

// Whether the caret is on the first / last visual line of a block.
const caretLine = (el) => {
  const sel = getSelection();
  if (!sel.rangeCount) return { first: true, last: true };
  const r = sel.getRangeAt(0).cloneRange();
  r.collapse(true);
  let rect = r.getClientRects()[0] || r.getBoundingClientRect();
  if (!rect || (rect.top === 0 && rect.bottom === 0)) return { first: true, last: true };
  const box = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  const line = parseFloat(cs.lineHeight) || 22;
  const padTop = parseFloat(cs.paddingTop) || 0;
  const padBottom = parseFloat(cs.paddingBottom) || 0;
  return {
    first: rect.top - (box.top + padTop) < line * 0.7,
    last: box.bottom - padBottom - rect.bottom < line * 0.7,
  };
};

document.addEventListener(
  'keydown',
  (e) => {
    if (e.isComposing || e.keyCode === 229) return;
    const t = e.target;
    const cmd = e.metaKey && !e.ctrlKey;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;

    // The link editor's own keys.
    if (t && t.id === 'link-input') {
      if (e.key === 'Enter') {
        e.preventDefault();
        applyLink(t.value);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        closeLinkEditor(true);
      }
      return;
    }

    // The find bar's own keys.
    if (t && t.id === 'find-input') {
      if (e.key === 'Enter') {
        e.preventDefault();
        findStep(e.shiftKey ? -1 : 1);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        closeFind();
      }
      return;
    }

    // App shortcuts that are not on the menu bar.
    if (e.ctrlKey && !e.metaKey && e.key === 'Tab') {
      e.preventDefault();
      send({ t: 'shortcut', name: e.shiftKey ? 'prev_tab' : 'next_tab' });
      return;
    }
    if (cmd && e.altKey && (e.key === 'ArrowRight' || e.key === 'ArrowLeft')) {
      e.preventDefault();
      send({ t: 'shortcut', name: e.key === 'ArrowRight' ? 'next_tab' : 'prev_tab' });
      return;
    }
    if (cmd && !e.shiftKey && !e.altKey && /^[1-9]$/.test(e.key)) {
      e.preventDefault();
      send({ t: 'shortcut', name: 'tab_' + e.key });
      return;
    }
    if (cmd && !e.shiftKey && !e.altKey && key === 'p') {
      e.preventDefault();
      send({ t: 'shortcut', name: 'quick_find' });
      return;
    }
    // While the app listens for a box, Escape stops the listening and no more.
    if (e.key === 'Escape' && hearBox) {
      e.preventDefault();
      e.stopPropagation();
      hearEnd();
      send({ t: 'shortcut', name: 'dictate_stop' });
      return;
    }
    if (e.key === 'Escape' && !(isBlock(t) && t.dataset.menu === '1')) {
      // With nothing open, Escape in a line picks the line (then ⌘C copies
      // it, and Shift with an arrow picks the lines above or below).
      const quiet = isBlock(t) && !document.querySelector('.popover, .modal-backdrop, .slash-menu') && !peeking() && !ctxFor && !findOpen();
      if (quiet && t.dataset.page) {
        e.preventDefault();
        t.blur();
        hideBar();
        const sel = getSelection();
        if (sel) sel.removeAllRanges();
        send({ t: 'pick', page: t.dataset.page, id: t.dataset.id, extend: false });
        return;
      }
      // The list of a choice cell closes, and the caret goes back to its cell.
      const list = t.closest && t.closest('.choice-picker');
      const back = list && list.parentElement.querySelector('.cell.pick');
      if (back) setTimeout(() => back.isConnected && back.focus(), 80);
      if (peeking()) setPeek(false);
      if (ctxFor) closeCtx();
      if (findOpen()) closeFind();
      send({ t: 'shortcut', name: 'escape' });
      if (isBlock(t)) t.blur();
      hideBar();
      return;
    }
    if (!isBlock(t)) return;

    const id = t.dataset.id;
    const page = t.dataset.page;
    const code = isCode(t);

    // Slash menu navigation.
    if (t.dataset.menu === '1') {
      const map = { ArrowDown: 'down', ArrowUp: 'up', Enter: 'enter', Escape: 'close', Tab: e.shiftKey ? 'up' : 'down' };
      if (map[e.key]) {
        e.preventDefault();
        send({ t: 'menu', action: map[e.key] });
        return;
      }
    }

    // Undo and redo cover the whole page, not just this block.
    if (cmd && key === 'z') {
      e.preventDefault();
      send({ t: e.shiftKey ? 'redo' : 'undo' });
      return;
    }
    if (cmd && !e.shiftKey && key === 'y') {
      e.preventDefault();
      send({ t: 'redo' });
      return;
    }

    // Inline formatting.
    if (cmd && !e.altKey) {
      const fmt =
        !e.shiftKey && key === 'b' ? 'bold'
        : !e.shiftKey && key === 'i' ? 'italic'
        : !e.shiftKey && key === 'e' ? 'code'
        : e.shiftKey && (key === 's' || key === 'x') ? 'strike'
        : null;
      if (fmt) {
        e.preventDefault();
        toggleMark(t, fmt);
        return;
      }
      if (!e.shiftKey && key === 'k') {
        const s = selIn(t);
        if (s && (s.start !== s.end || linkAt(t, s.start, s.end))) {
          e.preventDefault();
          openLinkEditor(t);
          return;
        }
        return; // nothing selected: ⌘K opens search from the menu bar
      }
    }

    // Plain typing is handled by the input event; only these keys need the
    // caret position (which isn't free to measure).
    if (!(e.key === 'Enter' || e.key === 'Backspace' || e.key === 'Tab' || e.key.startsWith('Arrow') || (cmd && key === 'd'))) return;
    const s = selIn(t) || { start: 0, end: 0 };

    if (cmd && e.shiftKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault();
      send({ t: 'block', action: e.key === 'ArrowUp' ? 'move_up' : 'move_down', page, id, pos: s.start });
      return;
    }
    if (cmd && !e.shiftKey && !e.altKey && key === 'd') {
      e.preventDefault();
      send({ t: 'block', action: 'duplicate', page, id, pos: s.start });
      return;
    }
    if (cmd && e.key === 'Enter') {
      e.preventDefault();
      send({ t: 'block', action: 'toggle', page, id, pos: s.start });
      return;
    }
    if (e.key === 'Enter' && (e.shiftKey || code) && !e.metaKey && !e.altKey && !e.ctrlKey) {
      e.preventDefault();
      document.execCommand('insertLineBreak');
      return;
    }
    if (e.key === 'Enter' && !e.metaKey && !e.altKey && !e.ctrlKey) {
      e.preventDefault();
      let pos = s.start;
      if (s.start !== s.end) {
        document.execCommand('delete');
        pos = (selIn(t) || s).start;
      }
      send({ t: 'enter', page, id, pos, value: mdOf(t) });
      return;
    }
    if (e.key === 'Backspace' && s.start === 0 && s.end === 0 && !e.metaKey && !e.altKey) {
      e.preventDefault();
      send({ t: 'backspace', page, id, value: mdOf(t) });
      return;
    }
    if (e.key === 'Tab') {
      e.preventDefault();
      if (code && !e.shiftKey) {
        document.execCommand('insertText', false, '  ');
        return;
      }
      send({ t: 'block', action: e.shiftKey ? 'outdent' : 'indent', page, id, pos: s.start });
      return;
    }
    const plain = !e.shiftKey && !e.metaKey && !e.altKey && !e.ctrlKey;
    if (plain && (e.key === 'ArrowUp' || e.key === 'ArrowLeft')) {
      const atStart = s.start === 0 && s.end === 0;
      if (atStart || (e.key === 'ArrowUp' && s.start === s.end && caretLine(t).first)) {
        e.preventDefault();
        send({ t: 'nav', dir: 'up', page, id });
      }
      return;
    }
    if (plain && (e.key === 'ArrowDown' || e.key === 'ArrowRight')) {
      const len = textLen(t);
      const atEnd = s.start === len && s.end === len;
      if (atEnd || (e.key === 'ArrowDown' && s.start === s.end && caretLine(t).last)) {
        e.preventDefault();
        send({ t: 'nav', dir: 'down', page, id });
      }
    }
  },
  true,
);

// The browser's own undo would only know about one block; use the app's.
document.addEventListener(
  'beforeinput',
  (e) => {
    if (!isBlock(e.target)) return;
    if (e.inputType === 'historyUndo' || e.inputType === 'historyRedo') {
      e.preventDefault();
      send({ t: e.inputType === 'historyUndo' ? 'undo' : 'redo' });
    } else if (e.inputType === 'formatBold' || e.inputType === 'formatItalic') {
      e.preventDefault();
      toggleMark(e.target, e.inputType === 'formatBold' ? 'bold' : 'italic');
    } else if (e.inputType === 'formatUnderline') {
      // The app keeps no underline: the browser's own would show and then
      // go at the next redraw.
      e.preventDefault();
    } else if (e.inputType === 'insertFromDrop') {
      // Something dragged into a line goes in as plain text, as a paste does
      // (the browser would put its HTML in, which is not kept).
      const text = e.dataTransfer && e.dataTransfer.getData('text/plain');
      e.preventDefault();
      if (text) document.execCommand('insertText', false, text.replace(/\s*\n+\s*/g, ' '));
    }
  },
  true,
);

document.addEventListener(
  'input',
  (e) => {
    const t = e.target;
    if (isBlock(t)) {
      if (!autoFormat(t, e)) autoLink(t, e);
      commit(t);
    } else if (t && t.tagName === 'TEXTAREA') autosize(t);
  },
  true,
);

document.addEventListener('focusin', (e) => {
  const t = e.target;
  if (isBlock(t)) {
    // A drag that picked whole lines just ended over this line: it stays
    // picked, and the line does not take the caret.
    if ((dragPick && dragPick.on) || performance.now() - dragPickEnded < 500) {
      t.blur();
      return;
    }
    initBlock(t);
    send({ t: 'focus', page: t.dataset.page, id: t.dataset.id });
  }
});

// ----- pictures for the AI ---------------------------------------------------------
// A picture pasted or dropped into the AI bar of a page, or into the box that
// asks all the notes, goes to the AI with the request (a screenshot, say). It
// is made smaller first when it is large: the AI needs no more, and a small
// picture is sent faster.
const aiBoxOf = (el) => el && el.closest && el.closest('.ai-dock, .modal.ask-notes');
const sendAiPicture = (file) => {
  const url = URL.createObjectURL(file);
  const img = new Image();
  img.onload = () => {
    const most = 1568;
    const scale = Math.min(1, most / Math.max(img.naturalWidth, img.naturalHeight));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(img.naturalWidth * scale));
    canvas.height = Math.max(1, Math.round(img.naturalHeight * scale));
    canvas.getContext('2d').drawImage(img, 0, 0, canvas.width, canvas.height);
    URL.revokeObjectURL(url);
    send({ t: 'ai_image', data: canvas.toDataURL('image/jpeg', 0.9) });
  };
  img.onerror = () => URL.revokeObjectURL(url);
  img.src = url;
};
const aiPicturesIn = (data) => [...((data && data.files) || [])].filter((f) => f.type.startsWith('image/')).slice(0, 4);
document.addEventListener(
  'paste',
  (e) => {
    if (!aiBoxOf(e.target)) return;
    const pictures = aiPicturesIn(e.clipboardData);
    if (!pictures.length) return;
    // (Not a picture for the page under the box.)
    e.preventDefault();
    e.stopPropagation();
    pictures.forEach(sendAiPicture);
  },
  true,
);
document.addEventListener(
  'dragover',
  (e) => {
    if (!aiBoxOf(e.target) || !e.dataTransfer || ![...e.dataTransfer.types].includes('Files')) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'copy';
  },
  true,
);
document.addEventListener(
  'drop',
  (e) => {
    if (!aiBoxOf(e.target) || !e.dataTransfer) return;
    const pictures = aiPicturesIn(e.dataTransfer);
    if (!pictures.length) return;
    e.preventDefault();
    e.stopPropagation();
    pictures.forEach(sendAiPicture);
  },
  true,
);

// Paste plain text. Several lines become blocks; a link pasted over selected
// text turns it into a link.
let pasting = false;
document.addEventListener(
  'paste',
  (e) => {
    // A picture on the clipboard (a screenshot, say) becomes an image block:
    // at the line with the caret, or in the empty image box that has focus.
    const holder = e.target.closest && e.target.closest('[data-image-empty]');
    const picture = pictureIn(e.clipboardData);
    if (picture && holder) {
      e.preventDefault();
      sendPicture(picture, holder.dataset.page, holder.dataset.imageEmpty);
      return;
    }
    const t = blockOf(e.target);
    if (!t) return;
    e.preventDefault();
    if (picture) {
      sendPicture(picture, t.dataset.page, t.dataset.id);
      return;
    }
    const text = (e.clipboardData && e.clipboardData.getData('text/plain')) || '';
    if (!text) return;
    const s = selIn(t);
    const trimmed = text.trim();
    if (!isCode(t) && s && s.start !== s.end && /^(https?:\/\/|mailto:|notion2:\/\/)\S+$/i.test(trimmed)) {
      setLink(t, s.start, s.end, trimmed);
      return;
    }
    // A web address pasted by itself goes in as a link.
    if (!isCode(t) && s && /^https?:\/\/\S+$/i.test(trimmed)) {
      pasting = true;
      document.execCommand('insertText', false, trimmed);
      pasting = false;
      const runs = domToRuns(t);
      renderInto(t, runsToHtml(mapRange(runs, s.start, s.start + trimmed.length, (r) => ({ ...r, link: trimmed }))));
      caretAfterFormat(t, s.start + trimmed.length);
      commit(t);
      return;
    }
    if (isCode(t) || !text.includes('\n')) {
      pasting = true;
      document.execCommand('insertText', false, text.replace(/\r\n?/g, '\n'));
      pasting = false;
      return;
    }
    if (s && s.start !== s.end) document.execCommand('delete');
    const pos = (selIn(t) || { start: 0 }).start;
    send({ t: 'paste', page: t.dataset.page, id: t.dataset.id, pos, value: mdOf(t), text: text.replace(/\r\n?/g, '\n') });
  },
  true,
);

// Links open on click (pages inside the app, everything else in the browser).
// Every link of the app goes this one way, and the click goes no further:
// the framework would otherwise hand the same address to the browser a
// second time, without the check of what may be opened.
document.addEventListener(
  'click',
  (e) => {
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a) return;
    e.preventDefault();
    e.stopPropagation();
    const sel = getSelection();
    if (sel && !sel.isCollapsed && a.closest('.blk')) return;
    send({ t: 'open_url', url: a.getAttribute('href') });
  },
  true,
);

// A press with the middle button on a link never lets the page itself go to
// the address.
document.addEventListener(
  'auxclick',
  (e) => {
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a) return;
    e.preventDefault();
    e.stopPropagation();
  },
  true,
);

// ----- window dragging from the empty parts of the top row and sidebar -------

document.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return;
  // The strip above a full-window drawing counts too (the box itself is the
  // target there; its canvas is a frame of its own).
  const strip = e.target.matches && e.target.matches('.drawing[data-expanded]');
  const region = e.target.closest && (strip ? e.target : e.target.closest('.drag-region'));
  if (!region || (!strip && e.target.closest('button, input, textarea, select, a, .tab, .nav-item, .tree-item, [data-nodrag]'))) return;
  if (e.detail === 2) send({ t: 'zoom_window' });
  else send({ t: 'drag' });
});

// ----- sidebar: resize when pinned, peek from the left edge when not ---------

const SIDEBAR = { min: 190, max: 320, reset: 256 };
const clampWidth = (w) => Math.round(Math.max(SIDEBAR.min, Math.min(w, SIDEBAR.max, innerWidth * 0.5, innerWidth - 540)));

document.addEventListener('mousedown', (e) => {
  const handle = e.button === 0 && e.target.closest && e.target.closest('[data-resize="sidebar"]');
  if (!handle) return;
  e.preventDefault();
  const app = handle.closest('.app');
  const side = handle.closest('.sidebar');
  if (!app || !side) return;
  if (e.detail === 2) {
    app.style.setProperty('--sidebar-w', SIDEBAR.reset + 'px');
    send({ t: 'sidebar_width', w: SIDEBAR.reset });
    return;
  }
  const startX = e.clientX;
  const startW = side.getBoundingClientRect().width;
  let w = startW;
  let frame = 0;
  document.body.dataset.resizing = '1';
  // While dragging, only the two things sized by it get the new width (set on
  // the whole app, every element would be restyled each frame).
  const sized = [side, document.querySelector('.nav-group')].filter(Boolean);
  const move = (ev) => {
    w = clampWidth(startW + ev.clientX - startX);
    if (!frame) {
      frame = requestAnimationFrame(() => {
        frame = 0;
        sized.forEach((el) => el.style.setProperty('--sidebar-w', w + 'px'));
      });
    }
  };
  const up = () => {
    document.removeEventListener('mousemove', move);
    document.removeEventListener('mouseup', up);
    cancelAnimationFrame(frame);
    delete document.body.dataset.resizing;
    sized.forEach((el) => el.style.removeProperty('--sidebar-w'));
    app.style.setProperty('--sidebar-w', w + 'px');
    send({ t: 'sidebar_width', w });
  };
  document.addEventListener('mousemove', move);
  document.addEventListener('mouseup', up);
});

// Panes in a row or column: drag the line between two to move their
// boundary; double-click it to share the space evenly. Pull the line on past
// a pane's smallest size and that pane folds to a slim rail (as in Aven);
// pull back and it opens again. One click on a rail opens it at the size it
// had before (see `PaneRail` and `AppState::fold_pane`).
const PANE_MIN_PX = 200; // a pane holds at this width…
const FOLD_PULL_PX = 48; // …until the line is pulled this much further
const RAIL_OPEN_PX = 100; // a rail opens once its line is pulled out this far
const RAIL_PX = 28;

document.addEventListener('mousedown', (e) => {
  const sash = e.button === 0 && e.target.closest && e.target.closest('.sash[data-split]');
  if (!sash) return;
  e.preventDefault();
  const group = sash.closest('.split-group');
  if (!group) return;
  const row = group.classList.contains('split-row');
  const index = Number(sash.dataset.index);
  const split = sash.dataset.split;
  const cellsNow = () => [...group.children].filter((c) => c.classList.contains('split-cell'));
  const railed = (c) => c.classList.contains('railed');
  const growOf = (c) => (railed(c) ? 0 : parseFloat(getComputedStyle(c).flexGrow) || 0);
  const first = cellsNow();
  if (!(index >= 0 && index + 1 < first.length)) return;
  // What is on screen, as shares (a rail has none; the app keeps the share
  // it had).
  const sendSizes = () => {
    const grow = cellsNow().map(growOf);
    const total = grow.reduce((a, b) => a + b, 0);
    if (total > 0) send({ t: 'split_sizes', split, sizes: grow.map((g) => g / total) });
  };
  if (e.detail === 2) {
    first.forEach((c) => railed(c) && c.dataset.leaf && send({ t: 'fold_pane', pane: c.dataset.leaf, on: false }));
    first.forEach((c) => !railed(c) && (c.style.flexGrow = String(1 / first.length)));
    send({ t: 'split_sizes', split, sizes: first.map(() => 1 / first.length) });
    return;
  }
  // How each part began: a pane folded during this drag goes back to the
  // share it began with, so it opens to that later.
  const began = first.map((c) => ({ railed: railed(c), grow: growOf(c) }));
  const span = (c) => {
    const r = c.getBoundingClientRect();
    return row ? [r.left, r.right] : [r.top, r.bottom];
  };
  let pointer = null;
  let waiting = null; // a fold or unfold the app has not drawn yet
  const refused = new Set();
  let changed = false;
  let frame = 0;
  document.body.dataset.resizing = row ? 'cols' : 'rows';

  const step = () => {
    frame = 0;
    const cs = cellsNow();
    if (pointer === null || cs.length !== began.length) return;
    if (waiting) {
      if (railed(cs[waiting.i]) !== waiting.want) {
        if (performance.now() - waiting.since > 500) {
          refused.add(waiting.i);
          waiting = null;
        } else {
          frame = requestAnimationFrame(step);
        }
        return;
      }
      waiting = null;
    }
    const a = cs[index];
    const b = cs[index + 1];
    const [a0, a1] = span(a);
    const [b0, b1] = span(b);
    const pair = b1 - a0;
    const min = Math.min(PANE_MIN_PX, pair / 3);
    const foldAt = Math.max(RAIL_PX + 12, min - FOLD_PULL_PX);
    const openAt = Math.min(RAIL_OPEN_PX, Math.max(RAIL_PX + 12, pair / 3));
    // The width each side would have with the line under the pointer.
    const sides = [
      [index, pointer - a0],
      [index + 1, b1 - pointer],
    ];
    for (const [i, width] of sides) {
      if (refused.has(i) || !cs[i].dataset.leaf) continue;
      const now = railed(cs[i]);
      const limit = began[i].railed ? openAt : foldAt;
      // A little slack either way, so it does not flicker on the line.
      const want = now ? width < limit + 6 : width < limit - 6;
      if (want === now) continue;
      if (want && !cs.some((c, j) => j !== i && !railed(c))) continue; // the last open part stays
      if (want) {
        [index, index + 1].forEach((j) => !railed(cs[j]) && !began[j].railed && (cs[j].style.flexGrow = String(began[j].grow)));
      }
      send({ t: 'fold_pane', pane: cs[i].dataset.leaf, on: want });
      waiting = { i, want, since: performance.now() };
      changed = true;
      frame = requestAnimationFrame(step);
      return;
    }
    // Both open: the line follows the pointer, between their smallest sizes.
    if (!railed(a) && !railed(b)) {
      const both = a1 - a0 + (b1 - b0);
      const grow = growOf(a) + growOf(b);
      if (both > 0 && grow > 0) {
        const width = Math.min(both - min, Math.max(min, pointer - a0));
        a.style.flexGrow = String((grow * width) / both);
        b.style.flexGrow = String(grow - (grow * width) / both);
        changed = true;
      }
    }
  };
  const move = (ev) => {
    pointer = row ? ev.clientX : ev.clientY;
    if (!frame) frame = requestAnimationFrame(step);
  };
  const up = () => {
    document.removeEventListener('mousemove', move);
    document.removeEventListener('mouseup', up);
    cancelAnimationFrame(frame);
    delete document.body.dataset.resizing;
    if (changed) sendSizes();
  };
  document.addEventListener('mousemove', move);
  document.addEventListener('mouseup', up);
});

// The unpinned sidebar slides out as a card while the pointer rests on the
// window's left edge, and tucks away soon after it leaves (as in Aven).
let peekShow = 0;
let peekHide = 0;
const peeking = () => document.body.dataset.peek === '1';
const setPeek = (on) => {
  clearTimeout(peekShow);
  clearTimeout(peekHide);
  peekShow = peekHide = 0;
  if (on) document.body.dataset.peek = '1';
  else delete document.body.dataset.peek;
};
const sidebarBusy = () => {
  const side = document.querySelector('.sidebar.floating');
  return !!(side && (side.querySelector('.popover') || side.contains(document.activeElement)));
};
document.addEventListener('mouseover', (e) => {
  const app = document.querySelector('.app');
  if (!app || app.classList.contains('pinned')) {
    if (peeking()) setPeek(false);
    return;
  }
  const t = e.target;
  const onEdge = t.closest && t.closest('.edge-strip');
  const onCard = t.closest && t.closest('.sidebar.floating');
  if (onEdge || onCard) {
    clearTimeout(peekHide);
    peekHide = 0;
    if (!peeking() && onEdge && !peekShow) peekShow = setTimeout(() => setPeek(true), 100);
  } else {
    clearTimeout(peekShow);
    peekShow = 0;
    if (peeking() && !peekHide) {
      peekHide = setTimeout(() => {
        peekHide = 0;
        if (!sidebarBusy()) setPeek(false);
      }, 90);
    }
  }
});
document.addEventListener('mouseleave', () => {
  if (peeking() && !sidebarBusy()) peekHide = setTimeout(() => setPeek(false), 90);
});
window.addEventListener('blur', () => peeking() && setPeek(false));
document.addEventListener('click', (e) => {
  // Clicking the edge pins the sidebar; choosing a page tucks the card away.
  // A press that opens one of the sidebar's menus (the space name, a row's
  // "more" button, its arrow) does not: the card stays while a menu is open.
  const t = e.target;
  if (!t.closest) return;
  const chose = t.closest('.sidebar.floating .tree-item, .sidebar.floating .tile') && !t.closest('.row-btn, .chevron, .tree-actions, .popover');
  if (t.closest('.edge-strip') || chose) {
    setTimeout(() => setPeek(false), 0);
  }
});

// ----- sliding highlight under the active tab (as in Aven) -------------------

const placeGlider = (animate) => {
  document.querySelectorAll('.tabs').forEach((tabs) => {
    const glider = tabs.querySelector(':scope > .tab-glider');
    if (!glider) return;
    const active = tabs.querySelector(':scope > .tab.active');
    if (!active) {
      glider.style.opacity = '0';
      return;
    }
    // Another space has other tabs: nothing to slide from.
    const jump = !animate || !glider._placed || glider._space !== tabs.dataset.space;
    glider._space = tabs.dataset.space;
    if (jump) glider.style.transition = 'none';
    glider.style.width = active.offsetWidth + 'px';
    glider.style.transform = `translateX(${active.offsetLeft}px)`;
    glider.style.opacity = '1';
    if (jump) {
      void glider.offsetWidth; // apply the jump before turning transitions back on
      glider.style.transition = '';
    }
    glider._placed = true;
    tabs.dataset.glider = '1';
  });
};
let gliderQueued = 0;
const queueGlider = (animate) => {
  cancelAnimationFrame(gliderQueued);
  gliderQueued = requestAnimationFrame(() => placeGlider(animate));
};
// Watch only the tab bar, and only for changes that can move the highlight:
// tabs added or removed, a title changing, or a tab's own classes (not the
// passing drag-and-drop markers).
const lasting = (c) =>
  (c || '')
    .split(/\s+/)
    .filter((x) => x && !/^(tab-drop-.*|tab-dragging|landed|seg-source)$/.test(x))
    .sort()
    .join(' ');
const gliderWatch = new MutationObserver((mutations) => {
  if (mutations.some((m) => m.type !== 'attributes' || lasting(m.oldValue) !== lasting(m.target.getAttribute('class')))) queueGlider(true);
});
let tabsWidth = 0;
const tabsResize = typeof ResizeObserver !== 'undefined'
  ? new ResizeObserver((entries) => {
      const w = Math.round(entries[0].contentRect.width);
      if (w !== tabsWidth) {
        tabsWidth = w;
        queueGlider(false); // follow resizes instantly, slide only on switches
      }
    })
  : null;
const watchTabs = () => {
  const tabs = document.querySelector('.tabs');
  if (!tabs || tabs._watched) return;
  tabs._watched = true;
  if (tabsResize) tabsResize.observe(tabs);
  gliderWatch.observe(tabs, { attributes: true, attributeFilter: ['class'], attributeOldValue: true, childList: true, characterData: true, subtree: true });
  queueGlider(false);
};

// ----- menus inside scrolling areas are lifted out so nothing clips them ------

const containingBlock = (el) => {
  for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) {
    const cs = getComputedStyle(n);
    if (cs.transform !== 'none' || cs.filter !== 'none' || (cs.backdropFilter && cs.backdropFilter !== 'none') || (cs.webkitBackdropFilter && cs.webkitBackdropFilter !== 'none')) {
      return n.getBoundingClientRect();
    }
  }
  return { top: 0, left: 0 };
};

// How much bigger an element is drawn than its own size says: the zoom of
// the page (View → Zoom In / Out), which is CSS `zoom` on the card. The
// window's measures (getBoundingClientRect, innerWidth) are in points of the
// screen; `left` and `top` inside the card are in its zoomed points.
// (Read from the card, not from the element: a menu that opens grows with a
// motion, and its drawn size is not its size yet.)
const zoomOf = (el) => {
  let z = 1;
  for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) {
    const own = parseFloat(getComputedStyle(n).zoom);
    if (own > 0 && own !== 1) z *= own;
  }
  return z;
};

const anchorMenu = (menu) => {
  const anchor = menu.parentElement;
  if (!anchor) return;
  const a = anchor.getBoundingClientRect();
  const cb = containingBlock(menu);
  menu.classList.add('anchored');
  const z = zoomOf(menu);
  const w = menu.offsetWidth * z;
  const h = menu.offsetHeight * z;
  const alignRight = menu.classList.contains('sidebar-menu') || menu.classList.contains('due-menu') || menu.classList.contains('ed-menu-right');
  let left = alignRight ? a.right - w : a.left;
  let top = a.bottom + 4;
  if (top + h > innerHeight - 8) top = Math.max(8, a.top - h - 4); // no room below: open upwards
  left = Math.max(8, Math.min(left, innerWidth - w - 8));
  menu.style.left = (left - cb.left) / z + 'px';
  menu.style.top = (top - cb.top) / z + 'px';
  menu.style.right = 'auto';
};

// A menu that hangs from a cell of a table. The table scrolls sideways in a
// box that would cut the menu at its edges, so the menu is fixed to the
// window: under what it hangs from, moved up as far as it must to end above
// the foot of the window, and inside the window at both sides. With no room
// at its right, its second list opens at its left.
const SUB_ROOM = 244;
const placeTableMenu = (menu) => {
  const anchor = menu.parentElement;
  if (!anchor) return;
  const a = anchor.getBoundingClientRect();
  const cb = containingBlock(menu);
  menu.classList.add('anchored');
  const z = zoomOf(menu);
  const w = menu.offsetWidth * z;
  const h = menu.offsetHeight * z;
  let left = a.left;
  left = Math.max(8, Math.min(left, innerWidth - w - 8));
  const top = Math.max(8, Math.min(a.bottom + 4, innerHeight - h - 8));
  menu.style.left = (left - cb.left) / z + 'px';
  menu.style.top = (top - cb.top) / z + 'px';
  menu.style.right = 'auto';
  menu.dataset.sub = left + w + SUB_ROOM > innerWidth - 8 ? 'left' : 'right';
};
// A second list beside a menu of a table, and a menu of the table itself,
// end above the foot of the window too.
const placeSubMenu = (sub) => {
  sub.style.top = '';
  const over = sub.getBoundingClientRect().bottom - (innerHeight - 8);
  if (over > 0) sub.style.top = `${parseFloat(getComputedStyle(sub).top) - over / zoomOf(sub)}px`;
};
const RAISED_MENUS = '.table-menu .column-sub, .own-menu, .kinds-menu';
// The menu of a line (its handle, or a right click): under the line, at the
// handle, and over the line when there is no room under it, so all of it is
// in the window with no scroll.
const placeBlockMenu = (menu) => {
  const row = menu.parentElement;
  if (!row) return;
  const a = row.getBoundingClientRect();
  const cb = containingBlock(menu);
  menu.classList.add('anchored');
  const z = zoomOf(menu);
  // As tall as all its items, up to the height of the window: no scroll
  // inside it while the window has room.
  menu.style.maxHeight = (innerHeight - 16) / z + 'px';
  const w = menu.offsetWidth * z;
  const h = menu.offsetHeight * z;
  // The first line of the row (a toggle with its children is a tall row).
  const line = Math.min(a.height, 28 * z);
  const left = Math.max(8, Math.min(a.left - 50 * z, innerWidth - w - 8));
  let top = a.top + line;
  if (top + h > innerHeight - 8) top = a.top - h - 4 >= 8 ? a.top - h - 4 : Math.max(8, innerHeight - h - 8);
  menu.style.left = (left - cb.left) / z + 'px';
  menu.style.top = (top - cb.top) / z + 'px';
  menu.style.right = 'auto';
};
const TABLE_MENUS = '.table-block .popover:not(.column-sub)';

// ----- the event panel's menus: the chosen row takes the keyboard -------------
// (the day picker moves its own focus: it knows the days). Arrows, Home and
// End move along the rows; Up from the first row goes to a box to type in.

function focusEdMenu(menu) {
  if (!menu.classList.contains('ed-menu') || menu.classList.contains('ed-days')) return;
  window.__edFocus = null; // a late return of the keyboard to the panel gives way (calendar.rs, focus_button)
  const rows = [...menu.querySelectorAll('.ed-menu-row')];
  const row = rows.find((r) => r.classList.contains('on')) || rows[0];
  if (row) row.focus();
}

document.addEventListener('keydown', (e) => {
  const menu = e.target.closest && e.target.closest('.ed-menu');
  if (!menu || menu.classList.contains('ed-days')) return;
  const inField = e.target.tagName === 'INPUT';
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key) || (inField && e.key !== 'ArrowDown')) return;
  const rows = [...menu.querySelectorAll('.ed-menu-row')];
  if (!rows.length) return;
  const at = rows.indexOf(document.activeElement);
  const field = menu.querySelector('.ed-menu-field');
  if (e.key === 'ArrowUp' && at <= 0 && field) {
    field.focus();
    e.preventDefault();
    return;
  }
  let i = at;
  if (e.key === 'ArrowDown') i = at < 0 ? 0 : Math.min(rows.length - 1, at + 1);
  else if (e.key === 'ArrowUp') i = Math.max(0, at - 1);
  else if (e.key === 'Home') i = 0;
  else i = rows.length - 1;
  rows[i].focus();
  e.preventDefault();
});

// ----- little pops when a to-do is ticked or a page is starred ----------------

document.addEventListener(
  'click',
  (e) => {
    const target = e.target.closest && (e.target.closest('.check') || e.target.closest('.icon-btn.star'));
    if (!target) return;
    delete target.dataset.pop;
    void target.offsetWidth;
    target.dataset.pop = '1';
    setTimeout(() => delete target.dataset.pop, 320);
  },
  true,
);

// ----- drag tabs, pages and panes (as in Aven) ----------------------------------
// In the tab bar: a tab held over the middle of another tab joins it; over a
// tab's edge it moves there. A sidebar page can be dropped among the tabs too.
// Over the page area: a tab, a sidebar page, or one page of a tab (by its part
// of the tab, or by its bar) lands on the nearest edge of the pane under the
// pointer - left, right, top or bottom - so a tab can hold any number of
// pages in any arrangement. A part of a tab that shows several pages takes
// just that page: let go elsewhere in the bar it becomes a tab of its own
// there (or joins the tab it's let go on the middle of); let go on its own
// tab, nothing changes. To move the whole tab, drag it by its edge.

let tabDrag = null; // id of the tab being dragged
let pageDrag = null; // id of the sidebar page being dragged
let segDrag = null; // { tab, pane } while one page of a tab is dragged
const dragging = () => tabDrag || pageDrag || segDrag;
const activeTab = () => document.querySelector('.tab.active')?.dataset.tab || null;
// The drop marker on screen: [element, class]. Drag-over fires many times a
// second even when the pointer is still, so the page only changes when the
// marker moves.
let dropMark = null;
const markDrop = (el, cls) => {
  if (dropMark && el && dropMark[0] === el && dropMark[1] === cls) return;
  if (!dropMark && !el) return;
  if (dropMark) dropMark[0].classList.remove(dropMark[1]);
  dropMark = el ? [el, cls] : null;
  if (el) el.classList.add(cls);
};
const clearTabDrop = () => markDrop(null);

// Where something dragged would land among the tabs. `skip` is a tab that
// can't be a target (the one being dragged).
const tabTarget = (e, skip) => {
  const tab = e.target.closest && e.target.closest('.tab[data-tab]');
  if (tab && tab.dataset.tab !== skip) {
    const r = tab.getBoundingClientRect();
    const x = (e.clientX - r.left) / r.width;
    const mode = x < 0.25 ? 'before' : x > 0.75 ? 'after' : 'split';
    return { el: tab, to: tab.dataset.tab, mode };
  }
  const strip = e.target.closest && e.target.closest('.tabs');
  return strip && !tab ? { el: null, strip, to: null, mode: 'end' } : null;
};

const showTabTarget = (t) => {
  if (t.el) markDrop(t.el, 'tab-drop-' + t.mode);
  else markDrop(t.strip || null, 'tab-drop-end');
};

// The pane under the pointer and its nearest edge: the pane's diagonals cut
// it into four triangles, one per edge (as in Aven).
const paneTarget = (e) => {
  const pane = e.target.closest && e.target.closest('.pane[data-pane]');
  if (!pane) return null;
  const r = pane.getBoundingClientRect();
  const nx = r.width ? (e.clientX - r.left) / r.width - 0.5 : 0;
  const ny = r.height ? (e.clientY - r.top) / r.height - 0.5 : 0;
  const edge = Math.abs(nx) > Math.abs(ny) ? (nx < 0 ? 'left' : 'right') : ny < 0 ? 'top' : 'bottom';
  return { id: pane.dataset.pane, edge, rect: r };
};

// One preview for the whole page area: the half of the pane the drop will
// take. It glides between places, and only moves when the place changes.
let paneMark = '';
const showPaneDrop = (t) => {
  const box = document.querySelector('.work-card > .pane-drop');
  const key = t ? t.id + ':' + t.edge : '';
  if (!box || key === paneMark) return;
  paneMark = key;
  if (!t) {
    box.classList.remove('show');
    return;
  }
  const card = box.parentElement.getBoundingClientRect();
  const r = t.rect;
  let [x, y, w, h] = [r.left - card.left, r.top - card.top, r.width, r.height];
  if (t.edge === 'left') w /= 2;
  else if (t.edge === 'right') (x += w / 2), (w /= 2);
  else if (t.edge === 'top') h /= 2;
  else (y += h / 2), (h /= 2);
  const inset = 6;
  if (!box.classList.contains('show')) box.style.transition = 'none'; // appear in place, then glide
  box.style.transform = `translate(${x + inset}px, ${y + inset}px)`;
  box.style.width = Math.max(0, w - inset * 2) + 'px';
  box.style.height = Math.max(0, h - inset * 2) + 'px';
  box.classList.add('show');
  if (box.style.transition) requestAnimationFrame(() => (box.style.transition = ''));
};

// What a pane drop would do, or null if it can't land here.
const paneDropFor = (e) => {
  const p = paneTarget(e);
  if (!p) return null;
  if (segDrag) return segDrag.pane === p.id ? null : p;
  if (tabDrag) return tabDrag === activeTab() ? null : p;
  return pageDrag ? p : null;
};

// Where one page of a tab would land in the top bar: a place among the tabs,
// the middle of another tab, or - over its own tab - nowhere ({ stay }).
const segBarTarget = (e) => {
  const t = tabTarget(e, null);
  if (t && t.to === segDrag.tab) return { stay: true };
  if (t) return t;
  // Left of the tabs (by back and forward): before the first tab.
  const row = e.target.closest && e.target.closest('.toprow');
  const first = row && row.querySelector('.tab[data-tab]');
  if (first && e.clientX < first.getBoundingClientRect().left) return { el: first, to: first.dataset.tab, mode: 'before' };
  return null;
};

// What a drag of a tab, page or block carries: a type only this app reads.
// (Plain text, even empty, makes the drag look like text to other apps, and
// shelf or drop-zone apps then offer themselves as a place to drop it.)
const OWN_DRAG = 'application/x-notion2-drag';
const markOwnDrag = (dt) => {
  dt.clearData();
  dt.setData(OWN_DRAG, '1');
};

document.addEventListener('dragstart', (e) => {
  const el = e.target && e.target.closest ? e.target : null;
  if (!el) return;
  const seg = el.closest('.tab-seg[data-pane]');
  const tab = el.closest('.tab[data-tab]');
  const page = !tab && el.closest('.tree-item[data-page]');
  // A pane's bar, when its tab shows several pages: moves that pane.
  const bar = !tab && !page && el.closest('.pagebar[draggable="true"][data-pane-drag]');
  if (!tab && !page && !bar) return;
  if (seg) segDrag = { tab: tab.dataset.tab, pane: seg.dataset.pane };
  else if (bar) segDrag = { tab: activeTab(), pane: bar.dataset.paneDrag };
  else if (tab) tabDrag = tab.dataset.tab;
  else pageDrag = page.dataset.page;
  if (segDrag && !segDrag.tab) {
    segDrag = null;
    return;
  }
  e.dataTransfer.effectAllowed = 'move';
  markOwnDrag(e.dataTransfer);
  document.body.dataset.dragging = seg || bar ? 'seg' : tab ? 'tab' : 'page';
  const moving = seg || tab || page || bar.closest('.pane');
  if (seg) e.dataTransfer.setDragImage(seg, e.offsetX || 20, e.offsetY || 10);
  if (bar) e.dataTransfer.setDragImage(bar, e.offsetX || 20, e.offsetY || 10);
  requestAnimationFrame(() => {
    moving.classList.add('tab-dragging');
    if (seg) tab.classList.add('seg-source');
  });
});

// A page of the sidebar dragged over the list of pages: onto a page (its
// middle) it goes inside that page; at a page's top or bottom edge it goes
// before or after it; below the list it goes to the end of the top level.
// The Favorites are not a place to put pages. Returns null off the list.
const SIDE_MARKS = ['page-drop-into', 'page-drop-before', 'page-drop-after'];
const clearSideTarget = () => {
  document.querySelectorAll('.page-drop-into, .page-drop-before, .page-drop-after').forEach((el) => {
    el.classList.remove(...SIDE_MARKS);
    el.style.removeProperty('--drop-left');
  });
};
function sideTarget(e) {
  const under = e.target && e.target.closest ? e.target : null;
  const area = under && under.closest('.side-scroll');
  if (!area || !pageDrag) return null;
  let row = under.closest('.tree-item[data-page]');
  if (!row) {
    // The "No pages inside" line under an open page: into that page.
    const inside = under.closest('.tree-children');
    const owner = inside && inside.parentElement && inside.parentElement.querySelector(':scope > .tree-item[data-page]');
    if (owner) {
      if (owner.dataset.fav === '1' || owner.dataset.page === pageDrag) return { stay: true };
      return { mode: 'into', row: owner, target: owner.dataset.page, mark: 'page-drop-into' };
    }
    // Below the pages (the "Add a page" line, the space under it).
    const last = area.querySelector('.tree-item.muted');
    if (under.closest('.section-label') || (last && e.clientY < last.getBoundingClientRect().top && under === area)) return { stay: true };
    return { mode: 'root', row: last, mark: 'page-drop-before' };
  }
  if (row.dataset.fav === '1') return { stay: true };
  // Not onto itself, nor onto a page inside it.
  const own = row.closest('.tree-children') ? [...document.querySelectorAll(`.tree-item[data-page="${CSS.escape(pageDrag)}"]`)].some((d) => d.parentElement.contains(row)) : row.dataset.page === pageDrag;
  if (own || row.dataset.page === pageDrag) return { stay: true };
  const r = row.getBoundingClientRect();
  const part = (e.clientY - r.top) / Math.max(1, r.height);
  let mode = part < 0.28 ? 'before' : part > 0.72 ? 'after' : 'into';
  // Under an open page are the pages inside it: "after" it means inside.
  if (mode === 'after' && row.parentElement.querySelector(':scope > .tree-children .tree-item[data-page]')) mode = 'into';
  return { mode, row, target: row.dataset.page, mark: `page-drop-${mode}` };
}
const showSideTarget = (s) => {
  const marked = document.querySelector('.page-drop-into, .page-drop-before, .page-drop-after');
  if (s.stay || !s.row) {
    clearSideTarget();
    return;
  }
  if (marked === s.row && s.row.classList.contains(s.mark)) return;
  clearSideTarget();
  s.row.classList.add(s.mark);
  // The line starts where the page's name starts.
  s.row.style.setProperty('--drop-left', getComputedStyle(s.row).paddingLeft);
};

document.addEventListener('dragover', (e) => {
  if (!dragging()) return;
  const side = sideTarget(e);
  if (side) {
    clearTabDrop();
    showPaneDrop(null);
    showSideTarget(side);
    if (!side.stay) {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
    }
    return;
  }
  clearSideTarget();
  // In the tab bar.
  const t = segDrag ? segBarTarget(e) : tabTarget(e, tabDrag);
  if (t) {
    showPaneDrop(null);
    if (t.stay) clearTabDrop();
    else showTabTarget(t);
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    return;
  }
  clearTabDrop();
  // Over the page area.
  const p = paneDropFor(e);
  showPaneDrop(p);
  // One page of a tab can also be let go off the panes (the sidebar, say):
  // it gets a tab of its own.
  if (p || (segDrag && !paneTarget(e))) {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
  }
});

const endTabDrag = () => {
  clearTabDrop();
  clearSideTarget();
  showPaneDrop(null);
  document.querySelectorAll('.tab-dragging').forEach((el) => el.classList.remove('tab-dragging'));
  document.querySelectorAll('.seg-source').forEach((el) => el.classList.remove('seg-source'));
  delete document.body.dataset.dragging;
  tabDrag = null;
  pageDrag = null;
  segDrag = null;
};

document.addEventListener('drop', (e) => {
  if (!dragging()) return;
  e.preventDefault();
  if (segDrag) {
    const { tab, pane } = segDrag;
    const t = segBarTarget(e);
    const p = t ? null : paneDropFor(e);
    if (t && t.stay) {
      // Let go on its own tab: nothing changes.
    } else if (t) {
      send({ t: 'seg_drop', tab, pane, to: t.to, mode: t.mode });
    } else if (p) {
      send({ t: 'place_pane', tab, pane, target: p.id, edge: p.edge });
    } else if (!paneTarget(e)) {
      send({ t: 'seg_drop', tab, pane, mode: 'out' });
    }
  } else if (sideTarget(e)) {
    const side = sideTarget(e);
    if (!side.stay) send({ t: 'page_move', page: pageDrag, target: side.target || null, mode: side.mode });
  } else {
    const t = tabTarget(e, tabDrag);
    const p = t ? null : paneDropFor(e);
    if (pageDrag && t) send({ t: 'page_strip_drop', page: pageDrag, to: t.to, mode: t.mode });
    else if (pageDrag && p) send({ t: 'place_page', page: pageDrag, target: p.id, edge: p.edge });
    else if (tabDrag && t) send({ t: 'tab_drop', from: tabDrag, to: t.to, mode: t.mode });
    else if (tabDrag && p) send({ t: 'place_tab', from: tabDrag, target: p.id, edge: p.edge });
  }
  endTabDrag();
});

document.addEventListener('dragend', () => dragging() && endTabDrag());

// A short glow on a tab that just moved or opened, so you can see where it went.
function landed(id) {
  let tries = 0;
  const attempt = () => {
    const el = document.querySelector(`.tab[data-tab="${CSS.escape(id)}"]`);
    if (!el) {
      if (tries++ < 30) requestAnimationFrame(attempt);
      return;
    }
    el.classList.remove('landed');
    void el.offsetWidth;
    el.classList.add('landed');
    setTimeout(() => el.classList.remove('landed'), 700);
  };
  requestAnimationFrame(attempt);
}

// ----- drag and drop blocks by their handle ---------------------------------

let dragId = null;
let rowMark = null; // [row, class] showing where the block would go
const markRow = (row, cls) => {
  if (rowMark && row && rowMark[0] === row && rowMark[1] === cls) return;
  if (rowMark) rowMark[0].classList.remove(rowMark[1]);
  rowMark = row ? [row, cls] : null;
  if (row) row.classList.add(cls);
};
const clearDrop = () => markRow(null);

// A space dragged to another place in the space menu: the app's own drag
// (the menu itself does the move).
document.addEventListener('dragstart', (e) => {
  const row = e.target && e.target.closest && e.target.closest('.space-menu .space-row[draggable="true"]');
  if (!row) return;
  e.dataTransfer.effectAllowed = 'move';
  markOwnDrag(e.dataTransfer);
});

document.addEventListener('dragstart', (e) => {
  const handle = e.target && e.target.closest && e.target.closest('[data-drag]');
  if (!handle) return;
  dragId = handle.dataset.drag;
  e.dataTransfer.effectAllowed = 'move';
  markOwnDrag(e.dataTransfer);
  const row = handle.closest('.block-row');
  if (row) {
    e.dataTransfer.setDragImage(row, 20, 12);
    requestAnimationFrame(() => row.classList.add('dragging'));
  }
});

const dropTarget = (e) => {
  const row = e.target && e.target.closest && e.target.closest('.block-row');
  if (!row || row.dataset.id === dragId) return null;
  const rect = row.getBoundingClientRect();
  return { row, after: e.clientY >= rect.top + rect.height / 2 };
};

document.addEventListener('dragover', (e) => {
  if (!dragId) return;
  const target = dropTarget(e);
  if (!target) return;
  e.preventDefault();
  e.dataTransfer.dropEffect = 'move';
  markRow(target.row, target.after ? 'drop-after' : 'drop-before');
});

const endDrag = () => {
  clearDrop();
  document.querySelectorAll('.dragging').forEach((el) => el.classList.remove('dragging'));
  dragId = null;
};

document.addEventListener('drop', (e) => {
  if (!dragId) return;
  e.preventDefault();
  const target = dropTarget(e);
  if (target) {
    send({ t: 'move', page: target.row.dataset.page, from: dragId, to: target.row.dataset.id, after: target.after });
  }
  endDrag();
});

document.addEventListener('dragend', endDrag);

// ----- drawings (Excalidraw in a frame of its own) ------------------------------
// The frame says when it's ready and sends its scene as you draw; the page
// keeps the latest scene per drawing, so a frame that opens again (a page
// shown again, the same page in both panes) gets it straight away.

const scenes = Object.assign({}, window.__n2_scenes || {});
delete window.__n2_scenes;
const drawingFrames = (id) =>
  [...document.querySelectorAll('.drawing[data-drawing]')].filter((d) => d.dataset.drawing === id).map((d) => d.querySelector('iframe.drawing-frame')).filter(Boolean);
const loadInto = (frame, json) => frame.contentWindow && frame.contentWindow.postMessage({ n2draw: 'load', json }, '*');

window.addEventListener('message', (e) => {
  const d = e.data;
  if (!d || typeof d !== 'object' || typeof d.n2draw !== 'string') return;
  const frame = [...document.querySelectorAll('iframe.drawing-frame')].find((f) => f.contentWindow === e.source);
  const box = frame && frame.closest('.drawing');
  if (!box) return;
  const id = box.dataset.drawing;
  if (d.n2draw === 'ready') {
    box.dataset.ready = '1';
    if (id in scenes) loadInto(frame, scenes[id]);
    else send({ t: 'drawing_ready', id }); // this window has no scene yet: ask for it
  } else if (d.n2draw === 'save' && typeof d.json === 'string') {
    scenes[id] = d.json;
    box.dataset.saves = String((Number(box.dataset.saves) || 0) + 1);
    send({ t: 'drawing_save', page: box.dataset.page, id, data: d.json });
    // The same drawing open in the other pane follows along.
    drawingFrames(id).forEach((f) => f !== frame && f.closest('.drawing').dataset.ready && loadInto(f, d.json));
  } else if (d.n2draw === 'expand') {
    setExpanded(box, !box.dataset.expanded);
  } else if (d.n2draw === 'probe') {
    box.dataset.probe = JSON.stringify(d.info || {});
  }
});

function drawingLoad(id, json) {
  scenes[id] = json;
  drawingFrames(id).forEach((f) => f.closest('.drawing').dataset.ready && loadInto(f, json));
}

// Expanded, a drawing fills the whole window until you shrink it again.
function setExpanded(box, on) {
  if (on) {
    document.querySelectorAll('.drawing[data-expanded]').forEach((b) => b !== box && setExpanded(b, false));
    box.dataset.expanded = '1';
    box.dataset.active = '1';
  } else {
    delete box.dataset.expanded;
  }
  const frame = box.querySelector('iframe.drawing-frame');
  if (frame && frame.contentWindow) frame.contentWindow.postMessage({ n2draw: 'expanded', on }, '*');
  if (on && frame) frame.focus();
}

// Until you click it, a drawing lets the page scroll past it.
document.addEventListener('click', (e) => {
  const cover = e.target.closest && e.target.closest('.drawing-cover');
  if (!cover) return;
  const box = cover.closest('.drawing');
  box.dataset.active = '1';
  const frame = box.querySelector('iframe.drawing-frame');
  if (frame) frame.focus();
});
document.addEventListener(
  'mousedown',
  (e) => {
    document.querySelectorAll('.drawing[data-active]').forEach((box) => {
      if (box.contains(e.target)) return;
      if (box.dataset.expanded) setExpanded(box, false);
      delete box.dataset.active;
    });
  },
  true,
);

// ----- right-click ---------------------------------------------------------------------
// A right-click shows only what belongs to the thing clicked, and never the
// system's long menu (Look Up, Translate, Share, Reload…):
//   a page in the sidebar → that page's menu; a tab → the tab's menu (Rust);
//   text that is selected, or a text box → Cut, Copy, Paste;
//   a line of a page with nothing selected → that line's menu;
//   a calendar event → it opens; anywhere else → nothing.

const ctxMenu = () => document.getElementById('ctx-menu');
let ctxFor = null; // { el, range } or { el, start, end }
const closeCtx = () => {
  const m = ctxMenu();
  if (m) m.classList.remove('show');
  ctxFor = null;
};
const openCtx = (el, x, y, hasSelection) => {
  const m = ctxMenu();
  if (!m) return;
  const sel = getSelection();
  ctxFor = isBlock(el)
    ? { el, range: sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null }
    : { el, start: el.selectionStart, end: el.selectionEnd };
  m.querySelectorAll('[data-ctx="cut"], [data-ctx="copy"]').forEach((i) => (i.style.display = hasSelection ? '' : 'none'));
  m.classList.add('show');
  const mz = zoomOf(m);
  m.style.left = Math.max(8, Math.min(x, innerWidth - m.offsetWidth * mz - 8)) / mz + 'px';
  m.style.top = Math.max(8, Math.min(y, innerHeight - m.offsetHeight * mz - 8)) / mz + 'px';
};
// Put the caret and selection back where they were when the menu opened.
const restoreCtx = (f) => {
  f.el.focus();
  if (f.range) {
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(f.range);
  } else if (typeof f.start === 'number' && f.el.setSelectionRange) {
    f.el.setSelectionRange(f.start, f.end);
  }
};

document.addEventListener(
  'contextmenu',
  (e) => {
    const t = e.target;
    if (!t || !t.closest || t.closest('.tab[data-tab]')) return;
    e.preventDefault();
    closeCtx();
    const row = t.closest('.tree-item[data-page]');
    if (row) {
      const more = row.querySelector('.tree-actions .row-btn');
      if (more) more.click();
      return;
    }
    const field = t.closest('.blk, input:not([type="checkbox"]):not([type="radio"]):not([type="range"]):not([type="color"]), textarea');
    if (field && !field.disabled) {
      const sel = getSelection();
      const picked = isBlock(field) ? !!sel && !sel.isCollapsed && field.contains(sel.anchorNode) : field.selectionStart !== field.selectionEnd;
      if (isBlock(field) && !picked) {
        // Nothing selected: the line's own menu (turn into, duplicate, delete).
        const handle = field.closest('.block-row') && field.closest('.block-row').querySelector('.handle');
        if (handle) handle.click();
        return;
      }
      openCtx(field, e.clientX, e.clientY, picked);
      return;
    }
    const event = t.closest('.cal-event, .cal-chip');
    if (event) event.click();
  },
  true,
);
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.target.closest && e.target.closest('#ctx-menu')) e.preventDefault(); // keep the selection
    else if (ctxFor) closeCtx();
  },
  true,
);
document.addEventListener('click', (e) => {
  const item = e.target.closest && e.target.closest('#ctx-menu [data-ctx]');
  if (!item || !ctxFor) return;
  const f = ctxFor;
  const kind = item.dataset.ctx;
  closeCtx();
  restoreCtx(f);
  if (kind === 'paste') {
    send({ t: 'shortcut', name: 'paste_clipboard' });
    return;
  }
  const text = f.range ? getSelection().toString() : f.el.value.slice(f.start, f.end);
  if (!text) return;
  send({ t: 'copy_text', text });
  if (kind === 'cut') document.execCommand('delete');
});
document.addEventListener('scroll', () => ctxFor && closeCtx(), true);
window.addEventListener('blur', closeCtx);

// ----- pictures: pasted or dropped -------------------------------------------------
const PICTURE_MOST = 30 * 1024 * 1024;
// The picture a clipboard or a drop holds, if it holds no text to prefer.
function pictureIn(data) {
  if (!data || data.getData('text/plain')) return null;
  const item = [...(data.items || [])].find((i) => i.kind === 'file' && i.type.startsWith('image/'));
  return (item && item.getAsFile()) || [...(data.files || [])].find((f) => f.type.startsWith('image/')) || null;
}
// A picture cell of a table: a screenshot pasted with the cell chosen
// (⌘V), or pictures dropped on it, go into that cell, not into the page.
const picCellOf = (el) => (el && el.closest ? el.closest('[data-pic-cell]') : null);
function sendCellPicture(file, cell) {
  const message = (data) => ({ t: 'cell_picture', page: cell.dataset.page, id: cell.dataset.block, r: Number(cell.dataset.r), c: Number(cell.dataset.c), data });
  if (file.size > PICTURE_MOST) return send(message(''));
  const reader = new FileReader();
  reader.onload = () => send(message(String(reader.result).split(',')[1] || ''));
  reader.readAsDataURL(file);
}
const cellPicturesIn = (data) => [...((data && data.files) || [])].filter((f) => f.type.startsWith('image/')).slice(0, 8);
document.addEventListener(
  'paste',
  (e) => {
    const cell = picCellOf(e.target) || picCellOf(document.activeElement);
    if (!cell) return;
    const picture = pictureIn(e.clipboardData);
    if (!picture) return;
    e.preventDefault();
    e.stopPropagation();
    sendCellPicture(picture, cell);
  },
  true,
);
document.addEventListener(
  'dragover',
  (e) => {
    const cell = picCellOf(e.target);
    if (!cell || !e.dataTransfer || ![...e.dataTransfer.types].includes('Files')) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'copy';
    document.querySelectorAll('[data-pic-cell].pic-drop').forEach((c) => c !== cell && c.classList.remove('pic-drop'));
    cell.classList.add('pic-drop');
  },
  true,
);
document.addEventListener(
  'dragleave',
  (e) => {
    const cell = picCellOf(e.target);
    if (cell && !cell.contains(e.relatedTarget)) cell.classList.remove('pic-drop');
  },
  true,
);
document.addEventListener(
  'drop',
  (e) => {
    const cell = picCellOf(e.target);
    if (!cell || !e.dataTransfer) return;
    cell.classList.remove('pic-drop');
    const pictures = cellPicturesIn(e.dataTransfer);
    if (!pictures.length) return;
    e.preventDefault();
    e.stopPropagation();
    pictures.forEach((p) => sendCellPicture(p, cell));
  },
  true,
);

// Sends a picture file to the app; the answer is kept until it is sent.
function sendPicture(file, page, id) {
  return new Promise((done) => {
    if (!file || !page) return done();
    if (file.size > PICTURE_MOST) {
      send({ t: 'image_data', page, id, data: '' });
      return done();
    }
    const reader = new FileReader();
    reader.onload = () => {
      send({ t: 'image_data', page, id, data: String(reader.result).split(',')[1] || '' });
      done();
    };
    reader.onerror = () => done();
    reader.readAsDataURL(file);
  });
}
// Picture files dropped on a page go in where they are let go: at the line
// under the pointer, or at the end of that page.
const holdsFiles = (e) => e.dataTransfer && [...e.dataTransfer.types].includes('Files');
document.addEventListener('dragover', (e) => {
  if (!holdsFiles(e)) return;
  e.preventDefault();
  e.dataTransfer.dropEffect = 'copy';
});
document.addEventListener('drop', (e) => {
  if (!holdsFiles(e)) return;
  e.preventDefault();
  const files = [...e.dataTransfer.files].filter((f) => f.type.startsWith('image/'));
  if (!files.length) return;
  const under = document.elementFromPoint(e.clientX, e.clientY);
  const holder = under && under.closest('[data-image-empty]');
  let page = holder && holder.dataset.page;
  let id = holder && holder.dataset.imageEmpty;
  if (!id) {
    // The row it was let go on (a picture, a table or a divider too), else
    // the last line of that page.
    const row = under && under.closest('.blocks > .block-row[data-id]');
    const pane = under && under.closest('.pane');
    const lastRow = pane && [...pane.querySelectorAll('.blocks > .block-row[data-id]')].pop();
    const at = row || lastRow;
    if (!at) return;
    page = at.dataset.page;
    id = at.dataset.id;
  }
  // One after another, so they keep their order: the first at the row, each
  // of the others after the picture put just before it ("last").
  files.reduce((before, file, n) => before.then(() => sendPicture(file, page, n === 0 ? id : 'last')).then(() => new Promise((done) => setTimeout(done, 60))), Promise.resolve());
});

// ----- tables ----------------------------------------------------------------------
// Tab goes to the next cell and Return to the cell below, in the order the
// rows show (a table can be sorted); past the last row each adds a row. The
// handle at the left of the row under the pointer adds a row below, opens
// the menu of the row, and moves the row when it is dragged. The card of a
// board is dragged to another column.
const tableOp = (box, op, row, column) => send({ t: 'table_op', page: box.dataset.page, id: box.dataset.table, op, row, column });
const cellAt = (box, r, c) => box.querySelector(`.cell[data-r="${r}"][data-c="${c}"]`);
const rowsOf = (box) => [...box.querySelectorAll('tr[data-r]')];
// The cells of a row that are typed in (not those worked out).
const FILLED_CELLS = 'textarea.cell:not([readonly]), button.cell.pick';
const typedCells = (tr) => [...tr.querySelectorAll(FILLED_CELLS)];
const intoCell = (cell) => {
  if (!cell) return;
  cell.focus();
  // (A choice cell is a button: there is no caret in it.)
  if (cell.setSelectionRange) cell.setSelectionRange(cell.value.length, cell.value.length);
};
// A letter typed on a choice cell opens its list with that letter in the box.
const seedPicker = (letter) => {
  let tries = 0;
  const tick = () => {
    const box = document.querySelector('.choice-picker .pick-input');
    if (box) {
      box.value = letter;
      box.dispatchEvent(new Event('input', { bubbles: true }));
    } else if (tries++ < 30) requestAnimationFrame(tick);
  };
  tick();
};
window.__n2.focusCell = (id, r, c) => {
  const box = document.querySelector(`.table-block[data-table="${id}"]`);
  if (!box) return;
  const tr = box.querySelector(`tr[data-r="${r}"]`);
  intoCell(cellAt(box, r, c) || (tr && typedCells(tr)[0]) || box.querySelector('.cell'));
};
// Where the columns that stay in place are, once the page shows them.
window.__n2.freezeColumns = (id) =>
  requestAnimationFrame(() => {
    const box = document.querySelector(`.table-block[data-table="${id}"]`);
    if (!box) return;
    const heads = box.querySelectorAll('tr.head-row > td');
    let left = 0;
    for (let i = 0; i < (Number(box.dataset.frozen) || 0) && i < heads.length; i += 1) {
      box.style.setProperty(`--f${i}`, `${left}px`);
      left += heads[i].getBoundingClientRect().width;
    }
  });
window.addEventListener('resize', () => {
  for (const box of document.querySelectorAll('.table-block[data-frozen]:not([data-frozen="0"])')) window.__n2.freezeColumns(box.dataset.table);
});
document.addEventListener(
  'keydown',
  (e) => {
    const cell = e.target.closest && e.target.closest('textarea.cell, button.cell.pick');
    // Not while characters are being put together (Japanese, Chinese,
    // Korean), and not the keys that change tab.
    if (!cell || e.isComposing || e.keyCode === 229 || e.ctrlKey || e.metaKey || e.altKey) return;
    // A choice cell: Return and Space open its list by themselves; a letter
    // opens it to search; Tab goes on as in any cell.
    if (cell.tagName === 'BUTTON' && e.key !== 'Tab') {
      if (e.key.length === 1 && e.key !== ' ') {
        e.preventDefault();
        cell.click();
        seedPicker(e.key);
      }
      return;
    }
    // (The values at the top of a row's page are not in a table: there Tab
    // goes to the next field by itself.)
    const box = cell.closest('.table-block');
    const tr = cell.closest('tr');
    if (!box || !tr) {
      if (e.key === 'Enter') e.preventDefault();
      return;
    }
    const trs = rowsOf(box);
    const at = trs.indexOf(tr);
    if (e.key === 'Tab') {
      e.preventDefault();
      e.stopPropagation();
      const step = e.shiftKey ? -1 : 1;
      const cells = typedCells(tr);
      const beside = cells[cells.indexOf(cell) + step];
      const line = trs[at + step];
      if (beside) intoCell(beside);
      else if (line) intoCell(e.shiftKey ? typedCells(line).pop() : typedCells(line)[0]);
      else if (!e.shiftKey) tableOp(box, 'add-row', Number(tr.dataset.r), 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      const below = trs[at + 1];
      if (below) intoCell(below.querySelector(`textarea.cell[data-c="${cell.dataset.c}"]:not([readonly]), button.cell.pick[data-c="${cell.dataset.c}"]`) || typedCells(below)[0]);
      else tableOp(box, 'add-row', Number(tr.dataset.r), Number(cell.dataset.c));
    }
  },
  true,
);
// The table with the caret keeps the order of its rows while the caret is
// in it (see `HELD` in ui/tables.rs): the app is told when it comes and goes.
let heldTable = null;
const letGo = () => {
  if (heldTable) tableOp(heldTable, 'release', 0, 0);
  heldTable = null;
};
document.addEventListener('focusin', (e) => {
  const cell = e.target.closest && e.target.closest('textarea.cell');
  const box = cell && cell.closest('.table-block');
  if (!box) return letGo();
  box.dataset.r = cell.dataset.r;
  box.dataset.c = cell.dataset.c;
  box.classList.add('editing');
  if (heldTable !== box) {
    letGo();
    heldTable = box;
    tableOp(box, 'hold', 0, 0);
  }
});
// A press on a control of the table that has the caret (Open, a checkbox,
// the arrow of nested rows) takes the caret out of its cell first. If the
// rows went to their sorted places at that moment, the press would end on
// another row than the one it was made on: the order is let go only when
// the press is over.
let pressedTable = null;
let owedRelease = null;
const pressOver = () =>
  setTimeout(() => {
    pressedTable = null;
    const box = owedRelease;
    owedRelease = null;
    if (box && heldTable === box && !box.contains(document.activeElement)) letGo();
  }, 0);
document.addEventListener(
  'mousedown',
  (e) => {
    pressedTable = (e.target.closest && e.target.closest('.table-frame')) || null;
  },
  true,
);
document.addEventListener('mouseup', pressOver, true);
document.addEventListener('dragend', pressOver, true);
document.addEventListener('focusout', (e) => {
  const box = e.target.closest && e.target.closest('.table-block');
  if (!box || box.contains(e.relatedTarget)) return;
  box.classList.remove('editing');
  if (heldTable !== box) return;
  if (pressedTable && pressedTable.contains(box)) owedRelease = box;
  else letGo();
});
// The handle goes to the row under the pointer.
document.addEventListener(
  'mousemove',
  (e) => {
    const tr = e.target.closest && e.target.closest('.table-block tr[data-r]');
    if (!tr || tr.dataset.r === '0') return;
    const frame = tr.closest('.table-frame');
    const handle = frame && frame.querySelector(':scope > .row-handle');
    if (!handle) return;
    const top = `${Math.round(tr.getBoundingClientRect().top - frame.getBoundingClientRect().top + (tr.offsetHeight - 24) / 2)}px`;
    if (handle.dataset.r === tr.dataset.r && handle.style.top === top) return;
    handle.dataset.r = tr.dataset.r;
    handle.style.top = top;
    handle.classList.add('on');
  },
  { passive: true },
);
const tableOfHandle = (handle) => handle.parentElement.querySelector(':scope > .table-block');
// The table with picked rows: a press anywhere but on a handle or in a menu
// lets them go (the menu of the rows can close and the rows stay picked, so
// that more rows are added with Shift and a click).
let pickedTable = null;
const unpick = () => {
  if (pickedTable) tableOp(pickedTable, 'unpick', 0, 0);
  pickedTable = null;
};
document.addEventListener(
  'mousedown',
  (e) => {
    if (pickedTable && !(e.target.closest && e.target.closest('.row-handle, .popover, .backdrop'))) unpick();
  },
  true,
);
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !document.querySelector('.popover')) unpick();
});
document.addEventListener(
  'click',
  (e) => {
    const b = e.target.closest && e.target.closest('.row-handle [data-row-op]');
    if (!b) return;
    // (Before the app sees the click: its own click closes an open menu,
    // and the menu of the rows must stay to take more rows.)
    e.stopPropagation();
    const handle = b.closest('.row-handle');
    const row = Number(handle.dataset.r) || 0;
    if (!row) return;
    const box = tableOfHandle(handle);
    if (b.dataset.rowOp === 'row-menu') pickedTable = box;
    tableOp(box, b.dataset.rowOp, row, b.dataset.rowOp === 'row-menu' && e.shiftKey ? 1 : 0);
  },
  true,
);
// A row dragged by its handle, or a card of a board by itself.
let tableDrag = null;
const endTableDrag = () => {
  for (const el of document.querySelectorAll('.drop-before, .drop-after, .board-col.drop')) el.classList.remove('drop-before', 'drop-after', 'drop');
  tableDrag = null;
};
document.addEventListener('dragstart', (e) => {
  const grip = e.target.closest && e.target.closest('.row-handle [draggable="true"]');
  const card = e.target.closest && e.target.closest('.board-card[data-card]');
  if (grip) {
    const handle = grip.closest('.row-handle');
    const box = tableOfHandle(handle);
    const from = Number(handle.dataset.r) || 0;
    // (A sorted table shows its rows in the order of the sort: there is no place to move a row to.)
    if (!from || box.classList.contains('sorted')) return e.preventDefault();
    tableDrag = { box, from };
    const tr = box.querySelector(`tr[data-r="${from}"]`);
    if (tr) e.dataTransfer.setDragImage(tr, 20, 12);
  } else if (card) {
    tableDrag = { board: card.closest('.table-board'), from: Number(card.dataset.card) };
  } else {
    return;
  }
  e.dataTransfer.effectAllowed = 'move';
  e.dataTransfer.setData('text/plain', '');
  markOwnDrag(e.dataTransfer);
});
document.addEventListener('dragover', (e) => {
  if (!tableDrag) return;
  const tr = tableDrag.box && e.target.closest && e.target.closest('tr[data-r]');
  const col = tableDrag.board && e.target.closest && e.target.closest('.board-col');
  const before = tableDrag.to;
  if (tr && tr.dataset.r !== '0' && tr.closest('.table-block') === tableDrag.box) {
    const rect = tr.getBoundingClientRect();
    tableDrag.to = { row: Number(tr.dataset.r), after: e.clientY > rect.top + rect.height / 2, el: tr };
  } else if (col && col.closest('.table-board') === tableDrag.board) {
    tableDrag.to = { el: col };
  } else {
    return;
  }
  e.preventDefault();
  e.dataTransfer.dropEffect = 'move';
  if (before && before.el === tableDrag.to.el && before.after === tableDrag.to.after) return;
  if (before) before.el.classList.remove('drop-before', 'drop-after', 'drop');
  tableDrag.to.el.classList.add(tableDrag.board ? 'drop' : tableDrag.to.after ? 'drop-after' : 'drop-before');
});
document.addEventListener('drop', (e) => {
  if (!tableDrag || !tableDrag.to) return;
  e.preventDefault();
  const { box, board, from, to } = tableDrag;
  if (box && to.row !== from) tableOp(box, to.after ? 'move-after' : 'move-row', from, to.row);
  if (board) send({ t: 'table_set', page: board.dataset.page, id: board.dataset.board, row: from, column: Number(board.dataset.group), text: to.el.dataset.value || '' });
  endTableDrag();
});
document.addEventListener('dragend', endTableDrag);

// ----- a cover picture, dragged to show another part ------------------------------
let coverDrag = null;
document.addEventListener('mousedown', (e) => {
  const cover = e.target.closest && e.target.closest('.page-cover.moving');
  const img = cover && cover.querySelector('img');
  if (!img || e.target.closest('.cover-tools')) return;
  e.preventDefault();
  const at = parseFloat((img.style.objectPosition || '50% 50%').split(' ')[1]);
  const y = Number.isFinite(at) ? at : 50;
  coverDrag = { cover, img, startY: e.clientY, y, now: y };
});
document.addEventListener('mousemove', (e) => {
  if (!coverDrag) return;
  // Dragging the picture down shows more of its top.
  const moved = ((e.clientY - coverDrag.startY) / coverDrag.cover.clientHeight) * 60;
  coverDrag.now = Math.min(100, Math.max(0, coverDrag.y - moved));
  coverDrag.img.style.objectPosition = `50% ${coverDrag.now}%`;
});
document.addEventListener('mouseup', () => {
  if (!coverDrag) return;
  send({ t: 'cover_y', page: coverDrag.cover.dataset.cover, y: coverDrag.now });
  coverDrag = null;
});

// ----- pictures: a click shows one large, a drag puts it anywhere, the corner sizes it --
// A picture that is let go off its line becomes free: it goes to the side of
// the page it was let go at, level with the line there, and the lines after
// it run beside it or below it (never under it). It keeps to that line, so it
// moves with that text. Rust keeps the place (see Place in model.rs).
const PICTURE_NARROWEST = 40;
const PICTURE_WIDEST = 4000;
const stillMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

// The large view: the picture as big as the window lets it be, over the app.
let large = null;
const picturesWith = (img) => {
  const blocks = img && img.isConnected && img.closest('.blocks');
  return blocks ? [...blocks.querySelectorAll('.image-block > img')] : [];
};
function fillLarge(img, grow) {
  const { big } = large;
  if (img !== big) {
    large.from = img;
    big.src = img.currentSrc || img.src;
  }
  // A picture far down a long page may not have loaded yet: then the large
  // one is sized when it has loaded itself.
  if (!img.naturalWidth) {
    big.onload = () => {
      if (large && large.big === big && big.naturalWidth) fillLarge(big, false);
    };
  }
  const nw = img.naturalWidth || img.width || 1;
  const nh = img.naturalHeight || img.height || 1;
  const room = 28;
  // Small pictures grow to twice their size at most, so they stay sharp.
  const fit = Math.min((innerWidth - room * 2) / nw, (innerHeight - room * 2) / nh, 2);
  const w = Math.max(1, Math.round(nw * fit));
  const h = Math.max(1, Math.round(nh * fit));
  big.style.width = w + 'px';
  big.style.height = h + 'px';
  if (grow && !stillMotion() && big.animate) {
    // It grows out of the picture in the page.
    const a = img.getBoundingClientRect();
    const left = (innerWidth - w) / 2;
    const top = (innerHeight - h) / 2;
    big.animate(
      [{ transform: `translate(${a.left - left}px, ${a.top - top}px) scale(${a.width / w}, ${a.height / h})` }, { transform: 'none' }],
      { duration: 220, easing: 'cubic-bezier(0.23, 1, 0.32, 1)' },
    );
  }
}
function hideLarge(now) {
  if (!large) return;
  const { shade } = large;
  large = null;
  shade.classList.remove('open');
  if (now || stillMotion()) shade.remove();
  else setTimeout(() => shade.remove(), 170);
}
function showLarge(img) {
  hideLarge(true);
  const shade = document.createElement('div');
  shade.className = 'lightbox';
  const big = document.createElement('img');
  big.alt = '';
  big.draggable = false;
  shade.appendChild(big);
  document.body.appendChild(shade);
  large = { shade, big, from: img };
  fillLarge(img, true);
  // The keys are for the large view: a drawing that had them gives them up.
  const had = document.activeElement;
  if (had && had.tagName === 'IFRAME') {
    had.blur();
    window.focus();
  }
  // A press anywhere puts it away (not the second press of a double click).
  const opened = performance.now();
  shade.addEventListener('mousedown', (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.detail > 1 || performance.now() - opened < 350) return;
    hideLarge();
  });
  setTimeout(() => shade.classList.add('open'), 0);
}
function stepLarge(by) {
  const all = picturesWith(large.from);
  const at = all.indexOf(large.from);
  const next = all[at + by];
  if (at >= 0 && next) fillLarge(next, false);
}
window.addEventListener('resize', () => {
  if (large && large.from.isConnected) fillLarge(large.from, false);
});

let picDrag = null;
const endPicDrag = (d) => {
  if (d.ghost) d.ghost.remove();
  d.box.classList.remove('lifting', 'sizing');
  document.body.classList.remove('picture-moving');
};
// Keys while a picture is large, or while one is dragged: they do not reach the page.
window.addEventListener('keydown', (e) => {
  if (picDrag && e.key === 'Escape') {
    const d = picDrag;
    picDrag = null;
    if (d.size && d.moved) {
      d.sizeEl.style.width = d.was;
      d.box.classList.toggle('sized', d.wasSized);
    }
    endPicDrag(d);
    e.preventDefault();
    e.stopImmediatePropagation();
    return;
  }
  if (!large) return;
  if (e.key === 'Meta' || e.key === 'Control' || e.key === 'Shift' || e.key === 'Alt') return;
  if (e.metaKey || e.ctrlKey) {
    hideLarge();
    return;
  }
  e.preventDefault();
  e.stopImmediatePropagation();
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') stepLarge(1);
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') stepLarge(-1);
  else if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') hideLarge();
  else if (e.key === 'Backspace' || e.key === 'Delete') {
    // Delete takes the picture that is shown out of its page (Undo brings it back).
    const box = large.from.isConnected && large.from.closest('.image-block[data-image]');
    hideLarge();
    if (box) send({ t: 'image_delete', page: box.dataset.page, id: box.dataset.image });
  }
}, true);

document.addEventListener('mousedown', (e) => {
  if (e.button !== 0 || e.ctrlKey || !e.target.closest) return;
  const handle = e.target.closest('.image-size');
  const img = handle ? null : e.target.closest('.image-block > img');
  const box = (handle || img || e.target).closest('.image-block');
  if ((!handle && !img) || !box || !box.dataset.image) return;
  e.preventDefault();
  const shown = box.querySelector('img');
  picDrag = {
    box,
    img: shown,
    page: box.dataset.page,
    id: box.dataset.image,
    size: !!handle,
    x: e.clientX,
    y: e.clientY,
    rect: shown.getBoundingClientRect(),
    // A free picture fills its row, which has the width.
    // The page can be zoomed (View): what the pointer moves is then not
    // what the page counts in. One page point is this many screen points.
    zoom: shown.offsetWidth ? shown.getBoundingClientRect().width / shown.offsetWidth || 1 : 1,
    sizeEl: box.classList.contains('free') ? box.closest('.block-row') : box,
    was: (box.classList.contains('free') ? box.closest('.block-row') : box).style.width,
    wasSized: box.classList.contains('sized'),
    moved: false,
    ghost: null,
  };
});
document.addEventListener('mousemove', (e) => {
  const d = picDrag;
  if (!d) return;
  const dx = e.clientX - d.x;
  const dy = e.clientY - d.y;
  if (!d.moved) {
    // A small slip of the hand is still a click.
    if (Math.hypot(dx, dy) < 5) return;
    d.moved = true;
    if (d.size) {
      d.box.classList.add('sizing');
    } else {
      const ghost = d.img.cloneNode();
      ghost.className = 'image-ghost';
      ghost.removeAttribute('loading');
      ghost.style.width = d.rect.width + 'px';
      ghost.style.height = d.rect.height + 'px';
      document.body.appendChild(ghost);
      d.ghost = ghost;
      d.box.classList.add('lifting');
      document.body.classList.add('picture-moving');
    }
  }
  if (d.size) {
    // A picture in line is as wide as its column at most.
    const most = d.box.classList.contains('free') ? PICTURE_WIDEST : Math.max(PICTURE_NARROWEST, d.box.parentElement.clientWidth);
    d.w = Math.round(Math.min(most, Math.max(PICTURE_NARROWEST, (d.rect.width + dx) / d.zoom)));
    d.box.classList.add('sized');
    d.sizeEl.style.width = d.w + 'px';
  } else {
    d.ghost.style.left = d.rect.left + dx + 'px';
    d.ghost.style.top = d.rect.top + dy + 'px';
  }
});
document.addEventListener('mouseup', (e) => {
  const d = picDrag;
  if (!d) return;
  picDrag = null;
  if (!d.moved) {
    if (!d.size) showLarge(d.img);
    return;
  }
  if (d.size) {
    endPicDrag(d);
    send({ t: 'image_size', page: d.page, id: d.id, w: d.w });
    return;
  }
  dropPicture(d, e.clientX - d.x, e.clientY - d.y);
});
// Let a dragged picture go: work out the line it is level with and the side
// of the page it is at, tell Rust, and keep the dragged copy up until the
// page shows it.
function dropPicture(d, dx, dy) {
  const blocks = d.box.closest('.blocks');
  const scroller = d.box.closest('.scroller');
  if (!blocks || !scroller) {
    endPicDrag(d);
    return;
  }
  const s = scroller.getBoundingClientRect();
  const b = blocks.getBoundingClientRect();
  const w = Math.max(PICTURE_NARROWEST, Math.round(d.rect.width / d.zoom));
  // On screen it is this wide (the page may be zoomed).
  const shownW = d.rect.width;
  // It stays inside the part of the page that shows.
  const gx = Math.min(Math.max(d.rect.left + dx, s.left + 4), Math.max(s.left + 4, s.right - shownW - 4));
  const gy = Math.min(Math.max(d.rect.top + dy, s.top + 4), Math.max(s.top + 4, s.bottom - 24));
  const own = d.box.closest('.block-row');
  const wasFree = d.box.classList.contains('free');
  // A small slip of a picture that is in line is not a move.
  if (!wasFree && Math.hypot(dx, dy) < 24) {
    endPicDrag(d);
    return;
  }
  // A picture that was in line leaves its line: the lines after it move up.
  const ownRect = own.getBoundingClientRect();
  const lift = wasFree ? 0 : ownRect.height + 1;
  const rows = [...blocks.children]
    .filter((r) => r.classList.contains('block-row') && !r.classList.contains('free') && r !== own)
    .map((r) => {
      const at = r.getBoundingClientRect();
      const up = at.top > ownRect.top ? lift : 0;
      return { id: r.dataset.id, top: at.top - up, bottom: at.bottom - up };
    });
  let to = null;
  let after = false;
  let from = ownRect.top;
  if (rows.length) {
    const last = rows[rows.length - 1];
    const under = rows.filter((r) => r.top <= gy).pop();
    if (gy >= last.bottom) {
      to = last.id;
      after = true;
      from = last.bottom;
    } else {
      const row = under || rows[0];
      to = row.id;
      from = row.top;
    }
  }
  // The side of the page its middle is at, and how far from that side's edge.
  const right = gx + shownW / 2 > b.left + b.width / 2;
  const x = Math.round((right ? b.right - (gx + shownW) : gx - b.left) / d.zoom);
  d.ghost.style.left = gx + 'px';
  d.ghost.style.top = Math.max(from, s.top + 4) + 'px';
  send({ t: 'image_place', page: d.page, id: d.id, x, w, right, to, after });
  const want = `true,${right ? 'right' : 'left'},${x},${w}`;
  let tries = 0;
  const settle = () => {
    const now = blocks.querySelector(`.image-block[data-image="${d.id}"]`);
    if ((now && now.dataset.place === want) || ++tries > 40) {
      endPicDrag(d);
      if (now) now.classList.remove('lifting');
    } else {
      setTimeout(settle, 16);
    }
  };
  setTimeout(settle, 16);
}

// ----- a picture that was just added: the typing line under it stays in view --------
// The picture takes its room only when it has loaded, which pushes the line
// with the caret (the empty one right under it) down, perhaps out of view.
document.addEventListener(
  'load',
  (e) => {
    const img = e.target;
    if (!img || img.tagName !== 'IMG' || !img.closest || !img.closest('.image-block')) return;
    const at = document.activeElement;
    const row = img.closest('.block-row');
    const mine = at && at.closest && at.closest('.block-row');
    const scroller = at && at.closest && at.closest('.scroller');
    if (!row || !mine || !scroller || row.nextElementSibling !== mine) return;
    const line = at.getBoundingClientRect();
    const view = scroller.getBoundingClientRect();
    const room = 28;
    if (line.bottom > view.bottom - room) scroller.scrollTop += line.bottom - (view.bottom - room);
  },
  true,
);

// ----- blocks picked as a whole: Command-A, then copy, cut or delete ------------------
// In a line, Command-A first selects the text of that line (the browser does
// that). When all of it is selected already, or the caret is in no line, it
// picks every block of the page. A press on a block that is not text (a
// divider, the space beside a picture) picks that block; with Shift, all the
// blocks up to it. Rust keeps what is picked and marks the rows.
const pickedRows = () => document.querySelectorAll('.block-row.picked');
const inField = (t) => !!(t && t.matches && t.matches('input, textarea, select'));
window.addEventListener(
  'keydown',
  (e) => {
    if (e.isComposing || e.keyCode === 229 || large) return;
    const cmd = e.metaKey && !e.ctrlKey && !e.altKey;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const t = e.target;
    if (cmd && !e.shiftKey && key === 'a') {
      if (inField(t) || document.querySelector('.modal-backdrop')) return;
      let page = null;
      if (isBlock(t)) {
        const sel = selIn(t);
        const len = textLen(t);
        // The first press is for the text of the line.
        if (len > 0 && !(sel && sel.start === 0 && sel.end >= len)) return;
        page = t.dataset.page;
      } else {
        const pane = document.querySelector('.pane.focused') || document.querySelector('.pane');
        const row = pane && pane.querySelector('.blocks > .block-row[data-page]');
        page = row && row.dataset.page;
      }
      if (!page) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (isBlock(t)) t.blur();
      const sel = getSelection();
      if (sel) sel.removeAllRanges();
      hideBar();
      send({ t: 'pick_all', page });
      return;
    }
    if (!pickedRows().length || inField(t)) return;
    const act = (name) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      send(name === 'pick_clear' ? { t: 'pick_clear' } : { t: 'shortcut', name });
    };
    if (e.shiftKey && !cmd && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      const all = [...pickedRows()[0].parentElement.querySelectorAll(':scope > .block-row[data-id]')];
      const on = all.map((r, i) => (r.classList.contains('picked') ? i : -1)).filter((i) => i >= 0);
      // Keys pressed fast, before the marks of the last press are drawn: go
      // on from where the last press reached.
      const recent = pickReach && performance.now() - pickReach.at < 800 ? pickReach : null;
      const top = Math.min(...on, recent ? recent.top : Infinity);
      const end = Math.max(...on, recent ? recent.end : -Infinity);
      const down = e.key === 'ArrowDown';
      const i = down ? end + 1 : top - 1;
      const to = all[i];
      e.preventDefault();
      e.stopImmediatePropagation();
      if (to) {
        pickReach = { top: Math.min(top, i), end: Math.max(end, i), at: performance.now() };
        send({ t: 'pick', page: to.dataset.page, id: to.dataset.id, extend: true });
      }
      return;
    }
    if (cmd && key === 'c') act('picked_copy');
    else if (cmd && key === 'x') act('picked_cut');
    else if (e.key === 'Backspace' || e.key === 'Delete') act('picked_delete');
    else if (e.key === 'Escape') act('pick_clear');
  },
  true,
);
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.button !== 0 || e.ctrlKey || !e.target.closest) return;
    const t = e.target;
    const row = t.closest('.blocks > .block-row');
    // What has a use of its own for a press: text, controls, pictures, tables, drawings.
    const own = t.closest('.blk, input, textarea, select, button, a, .gutter, .marker, .image-block, .table-wrap, .row-values, .drawing, .video-embed, .popover, .ai-panel, .page-link, .code-wrap, .callout-icon, .check, .toggle-arrow');
    const some = pickedRows().length > 0;
    if (row && (!own || (e.shiftKey && some))) {
      e.preventDefault();
      const a = document.activeElement;
      if (a && isBlock(a)) a.blur();
      send({ t: 'pick', page: row.dataset.page, id: row.dataset.id, extend: e.shiftKey && some });
      return;
    }
    if (some) send({ t: 'pick_clear' });
  },
  true,
);

// A drag of the text selection out of one line and into another picks the
// lines it goes over, as whole blocks (a line's text cannot be selected
// together with another's): then ⌘C copies them, ⌘X cuts them, Delete takes
// them out. Back in the first line, it is a text selection again.
let dragPick = null;
let dragPickEnded = -Infinity;
// How far Shift with an arrow has picked, while its marks are not drawn yet.
let pickReach = null;
document.addEventListener(
  'mousedown',
  (e) => {
    dragPick = null;
    if (e.button !== 0 || e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) return;
    const blk = blockOf(e.target);
    const row = blk && blk.closest('.block-row[data-id]');
    if (!row || !row.dataset.page) return;
    dragPick = { page: row.dataset.page, id: row.dataset.id, x: e.clientX, y: e.clientY, at: '', on: false };
  },
  true,
);
document.addEventListener(
  'mousemove',
  (e) => {
    const d = dragPick;
    if (!d || !(e.buttons & 1)) return;
    const under = document.elementFromPoint(e.clientX, e.clientY);
    const row = under && under.closest && under.closest('.block-row[data-id]');
    if (!row || row.dataset.page !== d.page) return;
    const id = row.dataset.id;
    if (id === d.at) return;
    d.at = id;
    if (id === d.id) {
      // Back in the line it began in: its text again.
      if (d.on) {
        d.on = false;
        document.body.classList.remove('drag-picking');
        send({ t: 'pick_clear' });
      }
      return;
    }
    if (!d.on) {
      d.on = true;
      document.body.classList.add('drag-picking');
      const a = document.activeElement;
      if (a && isBlock(a)) a.blur();
      hideBar();
    }
    const sel = getSelection();
    if (sel) sel.removeAllRanges();
    send({ t: 'pick', page: d.page, id: d.id, extend: false });
    send({ t: 'pick', page: d.page, id, extend: true });
  },
  true,
);
window.addEventListener(
  'mouseup',
  () => {
    if (dragPick && dragPick.on) {
      const sel = getSelection();
      if (sel) sel.removeAllRanges();
      dragPickEnded = performance.now();
      const a = document.activeElement;
      if (a && isBlock(a)) a.blur();
    }
    dragPick = null;
    document.body.classList.remove('drag-picking');
  },
  true,
);

// ----- the AI box: a click outside puts it away while nothing is typed in it ----------
// On the click, not on the press: a press on a line of the page must do its
// own work first.
document.addEventListener('click', (e) => {
  const box = document.getElementById('ai-input');
  if (!box || box.value.trim() || (e.target.closest && e.target.closest('.ai-panel'))) return;
  send({ t: 'shortcut', name: 'ai_close_idle' });
});

// The picker's menu opens under the AI box, or over it when there is no room under.
const placeAiMenu = (menu) => {
  const scroller = menu.closest('.scroller');
  if (!scroller) return;
  const m = menu.getBoundingClientRect();
  const s = scroller.getBoundingClientRect();
  const box = menu.closest('.ai-panel').getBoundingClientRect();
  if (m.bottom > s.bottom - 8 && box.top - s.top > s.bottom - box.bottom) menu.classList.add('up');
};

// ----- microphones beside text boxes ------------------------------------------------
// A button with data-dictate="<id of a text box>" puts the caret in that box
// and has the Mac's Dictation type what is said into it.
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.target.closest && e.target.closest('[data-dictate]')) e.preventDefault();
  },
  true,
);
document.addEventListener('click', (e) => {
  const b = e.target.closest && e.target.closest('[data-dictate]');
  const box = b && document.getElementById(b.dataset.dictate);
  if (!box) return;
  // Pressed while the app's own listener types into this box: stop. (The
  // last words still arrive.)
  if (hearBox === box) {
    b.classList.remove('hearing');
    send({ t: 'shortcut', name: 'dictate_stop' });
    return;
  }
  box.focus();
  document.body.dataset.dictating = b.dataset.dictate;
  send({ t: 'shortcut', name: 'dictate' });
});

// With the app's own listener, the words are put into the box from here as
// they are heard (after what the box already held).
let hearBox = null;
let hearBase = '';
// How long it listens, for a small clock (an element with data-hear-clock).
let hearClock = 0;
const hearTime = (since) => {
  const s = Math.max(0, Math.floor((Date.now() - since) / 1000));
  const text = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  document.querySelectorAll('[data-hear-clock]').forEach((c) => (c.textContent = text));
};
const hearEnd = () => {
  if (!hearBox) return;
  clearInterval(hearClock);
  if (hearBox.dataset.hint !== undefined) {
    hearBox.placeholder = hearBox.dataset.hint;
    delete hearBox.dataset.hint;
  }
  hearBox = null;
  document.querySelectorAll('.mic-btn.hearing').forEach((b) => b.classList.remove('hearing'));
  delete document.body.dataset.dictating;
};
// The box is gone (its panel was put away), or the caret is no longer in
// it: nothing is listening for it any more.
const hearLost = () => {
  hearEnd();
  delete document.body.dataset.dictating;
  send({ t: 'shortcut', name: 'dictate_stop' });
};
window.__n2.hearStart = () => {
  const id = document.body.dataset.dictating;
  const box = id && document.getElementById(id);
  if (!box || document.activeElement !== box) return hearLost();
  hearBox = box;
  hearBase = box.value || '';
  const since = Date.now();
  clearInterval(hearClock);
  hearTime(since);
  hearClock = setInterval(() => hearTime(since), 500);
  // While it listens, the box says so where its hint is.
  if (box.placeholder !== undefined && box.dataset.hint === undefined) {
    box.dataset.hint = box.placeholder;
    box.placeholder = 'Listening…';
  }
  const mic = document.querySelector(`[data-dictate="${id}"]`);
  if (mic) mic.classList.add('hearing');
};
window.__n2.hear = (text) => {
  if (!hearBox) return;
  if (!hearBox.isConnected || document.activeElement !== hearBox) return hearLost();
  hearBox.value = hearBase + (hearBase && !/\s$/.test(hearBase) ? ' ' : '') + text;
  hearBox.dispatchEvent(new Event('input', { bubbles: true }));
  hearBox.scrollTop = hearBox.scrollHeight;
};
window.__n2.hearEnd = hearEnd;
// Leaving the box, or Return in it, ends the listening at once.
const hearLeave = (e) => {
  if (!hearBox || e.target !== hearBox) return;
  hearEnd();
  send({ t: 'shortcut', name: 'dictate_stop' });
};
document.addEventListener('focusout', hearLeave);
document.addEventListener(
  'keydown',
  (e) => {
    if (e.key === 'Enter' && !e.shiftKey) hearLeave(e);
  },
  true,
);

// ----- the microphone on an empty line -------------------------------------------
// An empty text line with the caret shows a hint ("Type '/' for commands…").
// A small microphone sits just after that hint: click it, speak, and the AI
// writes what you said into the page at that line (see `LineMic`, VoiceModal).

const lineMic = () => document.getElementById('line-mic');
let micQueued = false;
let hintFont = '';
let hintText = '';
let hintWidth = 0;
const hintMeasure = document.createElement('canvas').getContext('2d');

const placeLineMic = () => {
  micQueued = false;
  const mic = lineMic();
  if (!mic) return;
  const el = document.activeElement;
  const row = isBlock(el) ? el.closest('.block-row.kind-text') : null;
  const busy = document.querySelector('.slash-menu, .modal-backdrop, .popover');
  if (!row || el.textContent !== '' || busy) {
    mic.classList.remove('show');
    return;
  }
  const r = el.getBoundingClientRect();
  const style = getComputedStyle(el);
  // The hint is drawn by CSS, so its width is measured here (once per font).
  const hint = el.dataset.placeholder || '';
  if (style.font !== hintFont || hint !== hintText) {
    hintFont = style.font;
    hintText = hint;
    hintMeasure.font = style.font;
    hintWidth = hintMeasure.measureText(hint).width;
  }
  // The page may be zoomed; the measured width is not.
  const zoom = r.width / (el.offsetWidth || r.width) || 1;
  const size = 22;
  const left = Math.min(r.left + (parseFloat(style.paddingLeft) || 0) * zoom + hintWidth * zoom + 8, r.right - size);
  mic.style.left = Math.round(left) + 'px';
  mic.style.top = Math.round(r.top + (r.height - size) / 2) + 'px';
  // With no AI tool the words go in as they are said.
  mic.title = document.body.dataset.ai === '0' ? 'Speak, and your words are typed here' : 'Speak, and the AI writes it here';
  mic.classList.add('show');
};
const queueLineMic = () => {
  if (micQueued) return;
  micQueued = true;
  requestAnimationFrame(placeLineMic);
};
['focusin', 'focusout', 'input'].forEach((name) => document.addEventListener(name, queueLineMic));
document.addEventListener('scroll', queueLineMic, true);
window.addEventListener('resize', queueLineMic);
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.target.closest && e.target.closest('#line-mic')) e.preventDefault(); // the caret stays in the line
  },
  true,
);
document.addEventListener('click', (e) => {
  if (e.target.closest && e.target.closest('#line-mic')) send({ t: 'shortcut', name: 'voice_here' });
});

// ----- find in page (⌘F) ---------------------------------------------------------
// Searches the text of the page in front. Matches are marked with CSS
// highlights, so the page's own text and its editing are never touched.
// Return / ⌘G go to the next match, ⇧Return / ⇧⌘G back, Escape closes.

const canHighlight = typeof CSS !== 'undefined' && !!CSS.highlights && typeof Highlight !== 'undefined';
const findBar = () => document.getElementById('find-bar');
const findInput = () => document.getElementById('find-input');
const findOpen = () => !!findBar() && findBar().classList.contains('show');
let findMatches = [];
let findAt = -1;
let findWatch = null;
let findTimer = 0;

// The page area in front: the focused pane when the tab shows several.
const findRoot = () => document.querySelector('.pane.focused .scroller') || document.querySelector('.pane .scroller');

const collectMatches = (root, query) => {
  const out = [];
  if (!root || !query) return out;
  const needle = query.toLowerCase();
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => {
      const p = n.parentElement;
      if (!p || p.closest('textarea, input, select, button, .gutter, .drawing, .popover, [aria-hidden="true"]')) return NodeFilter.FILTER_REJECT;
      return n.nodeValue && n.nodeValue.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP;
    },
  });
  for (let n = walker.nextNode(); n && out.length < 2000; n = walker.nextNode()) {
    const text = n.nodeValue.toLowerCase();
    for (let i = text.indexOf(needle); i !== -1 && out.length < 2000; i = text.indexOf(needle, i + needle.length)) {
      const r = document.createRange();
      r.setStart(n, i);
      r.setEnd(n, i + needle.length);
      out.push(r);
    }
  }
  return out;
};

const paintFind = () => {
  const count = document.getElementById('find-count');
  if (count) {
    count.textContent = !findInput() || !findInput().value ? '' : findMatches.length ? `${findAt + 1} of ${findMatches.length}` : 'No matches';
  }
  if (!canHighlight) return;
  if (findMatches.length) CSS.highlights.set('n2-find', new Highlight(...findMatches));
  else CSS.highlights.delete('n2-find');
  const current = findMatches[findAt];
  if (current) CSS.highlights.set('n2-find-current', new Highlight(current));
  else CSS.highlights.delete('n2-find-current');
};

const revealMatch = () => {
  const r = findMatches[findAt];
  const el = r && r.startContainer.parentElement;
  if (el) el.scrollIntoView({ block: 'center', behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });
};

// `keepPlace`: the page changed under an open search; stay on the same match.
const runFind = (keepPlace) => {
  findMatches = collectMatches(findRoot(), findInput() ? findInput().value : '');
  if (!keepPlace || findAt >= findMatches.length || findAt < 0) findAt = findMatches.length ? 0 : -1;
  paintFind();
  if (!keepPlace) revealMatch();
};

function findStep(d) {
  if (!findMatches.length) return;
  findAt = (findAt + d + findMatches.length) % findMatches.length;
  paintFind();
  revealMatch();
}

// Sits at the top right of the page area in front.
const placeFind = () => {
  const bar = findBar();
  const pane = document.querySelector('.pane.focused') || document.querySelector('.pane');
  if (!bar || !pane) return;
  const r = pane.getBoundingClientRect();
  bar.style.top = Math.round(r.top + 50) + 'px';
  bar.style.right = Math.round(innerWidth - r.right + 14) + 'px';
};

function openFind() {
  const bar = findBar();
  const input = findInput();
  if (!bar || !input) return;
  // Selected text (a short single line) is what to look for.
  const sel = getSelection();
  const picked = sel && !sel.isCollapsed ? sel.toString().trim() : '';
  if (picked && picked.length <= 80 && !picked.includes('\n')) input.value = picked;
  placeFind();
  bar.classList.add('show');
  input.focus();
  input.select();
  runFind(false);
  // While open, follow the page as it changes (typing, another tab or pane).
  if (!findWatch) {
    findWatch = new MutationObserver(() => {
      clearTimeout(findTimer);
      findTimer = setTimeout(() => {
        placeFind();
        runFind(true);
      }, 150);
    });
    findWatch.observe(document.querySelector('.work-card') || document.body, { childList: true, subtree: true, characterData: true });
  }
}

function closeFind() {
  const bar = findBar();
  if (bar) bar.classList.remove('show');
  if (findWatch) findWatch.disconnect();
  findWatch = null;
  clearTimeout(findTimer);
  findMatches = [];
  findAt = -1;
  paintFind();
}

document.addEventListener('input', (e) => {
  if (e.target && e.target.id === 'find-input') runFind(false);
  if (e.target && e.target.matches && e.target.matches('textarea.ed-title')) autosize(e.target);
});
document.addEventListener('click', (e) => {
  const b = e.target.closest && e.target.closest('[data-find]');
  if (!b) return;
  if (b.dataset.find === 'next') findStep(1);
  else if (b.dataset.find === 'prev') findStep(-1);
  else closeFind();
});
window.addEventListener('resize', () => findOpen() && placeFind());

// ----- page title sizing (a plain text area) -----------------------------------

const fieldSizing = typeof CSS !== 'undefined' && CSS.supports && CSS.supports('field-sizing', 'content');

function autosize(el) {
  if (!el || fieldSizing || el.tagName !== 'TEXTAREA') return;
  el.style.height = 'auto';
  el.style.height = el.scrollHeight + 'px';
}

let sizeQueued = false;
function queueSizeAll() {
  if (fieldSizing || sizeQueued) return; // the browser sizes titles itself

  sizeQueued = true;
  requestAnimationFrame(() => {
    sizeQueued = false;
    document.querySelectorAll('textarea.title-input, textarea.ed-title').forEach(autosize);
  });
}

// ----- new blocks get their text from data-html ----------------------------------

new MutationObserver((mutations) => {
  for (const m of mutations) {
    if (isBlock(m.target) || blockOf(m.target)) continue; // edits inside a block
    m.addedNodes.forEach((n) => {
      initWithin(n);
      if (n.nodeType !== 1) return;
      if (n.tagName === 'TEXTAREA' || (n.querySelector && n.querySelector('textarea.title-input, textarea.ed-title'))) queueSizeAll();
      const menus = n.matches && n.matches('.sidebar-menu, .tab-menu, .due-menu, .ed-menu') ? [n] : n.querySelectorAll ? n.querySelectorAll('.sidebar-menu, .tab-menu, .due-menu, .ed-menu') : [];
      (n.matches && n.matches('.block-menu') ? [n] : n.querySelectorAll ? [...n.querySelectorAll('.block-menu')] : []).forEach(placeBlockMenu);
      menus.forEach(anchorMenu);
      menus.forEach(focusEdMenu);
      // The menus that hang from the cells of a table; one whose rows changed keeps its place in the window.
      const within = n.closest && n.closest(TABLE_MENUS);
      (within ? [within] : n.matches && n.matches(TABLE_MENUS) ? [n] : n.querySelectorAll ? [...n.querySelectorAll(TABLE_MENUS)] : []).forEach(placeTableMenu);
      (n.matches && n.matches(RAISED_MENUS) ? [n] : n.querySelectorAll ? [...n.querySelectorAll(RAISED_MENUS)] : []).forEach(placeSubMenu);
      if ((n.matches && n.matches('.page-outline, .outline-item')) || (n.querySelector && n.querySelector('.page-outline'))) queueOutline();
      (n.matches && n.matches('.graph[data-graph]') ? [n] : n.querySelectorAll ? [...n.querySelectorAll('.graph[data-graph]')] : []).forEach(startGraph);
      const pickers = n.matches && n.matches('.ai-menu') ? [n] : n.querySelectorAll ? n.querySelectorAll('.ai-menu') : [];
      pickers.forEach(placeAiMenu);
    });
  }
  watchTabs();
}).observe(document.body, { childList: true, subtree: true });

window.addEventListener('resize', queueSizeAll);
initWithin(document.body);
queueSizeAll();
watchTabs();
queueGlider(false);

// Used by the in-app self-test.
// ----- the outline at the right edge of a page -----------------------------------
// A click goes to its heading. While the page scrolls, the heading that the
// reader is in is marked.
document.addEventListener('click', (e) => {
  const item = e.target.closest && e.target.closest('.outline-item[data-outline]');
  if (!item) return;
  const head = document.getElementById('blk-' + item.dataset.outline);
  const scroller = head && head.closest('.scroller');
  if (!scroller) return;
  const top = head.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop - 72;
  const calm = matchMedia('(prefers-reduced-motion: reduce)').matches;
  scroller.scrollTo({ top: Math.max(0, top), behavior: calm ? 'auto' : 'smooth' });
});
let outlineQueued = false;
const markOutline = () => {
  outlineQueued = false;
  document.querySelectorAll('.page-outline').forEach((nav) => {
    const scroller = nav.closest('.scroller');
    if (!scroller) return;
    const box = scroller.getBoundingClientRect();
    // At the end of the page the last headings cannot come up to the top:
    // there, the last heading in view is the one the reader is in.
    const atEnd = scroller.scrollTop > 0 && scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2;
    const line = atEnd ? box.bottom : box.top + 96;
    const items = [...nav.querySelectorAll('.outline-item')];
    let on = null;
    for (const item of items) {
      const head = document.getElementById('blk-' + item.dataset.outline);
      if (head && head.getBoundingClientRect().top <= line) on = item;
    }
    for (const item of items) item.classList.toggle('on', item === on);
  });
};
const queueOutline = () => {
  if (outlineQueued) return;
  outlineQueued = true;
  requestAnimationFrame(markOutline);
};
document.addEventListener('scroll', queueOutline, true);

// ----- the graph of pages ---------------------------------------------------------
// Rust gives the pages and what joins them (data-graph on .graph). This draws
// them on a canvas with a small force layout: the dots push each other away,
// a line pulls its two dots together, and then the picture comes to rest.
// Drag a dot to move it, drag the background to move the picture, scroll to
// zoom, double-click the background to see all of it, click a dot to open
// its page.
const GRAPH_REST = 0.02;
const startGraph = (el) => {
  if (el._graph) return;
  const canvas = el.querySelector('canvas');
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  const calm = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const g = { nodes: [], edges: [], near: [], view: { x: 0, y: 0, k: 1 }, alpha: 0, hover: -1, find: '', drag: null, raf: 0, fitted: false, held: false, w: 0, h: 0, sig: '' };
  el._graph = g;
  const off = new AbortController();

  const X = (n) => g.w / 2 + g.view.x + n.x * g.view.k;
  const Y = (n) => g.h / 2 + g.view.y + n.y * g.view.k;
  // The dots grow a little with the zoom, less than the distances do.
  const dotScale = () => Math.max(0.6, Math.min(1.6, Math.sqrt(g.view.k)));

  const tick = () => {
    const ns = g.nodes;
    const a = g.alpha;
    for (let i = 0; i < ns.length; i++) {
      const p = ns[i];
      for (let j = i + 1; j < ns.length; j++) {
        const q = ns[j];
        let dx = p.x - q.x;
        let dy = p.y - q.y;
        let d2 = dx * dx + dy * dy;
        if (d2 > 250000) continue; // far apart: no push
        if (d2 < 1) {
          dx = i % 2 ? 0.5 : -0.5;
          dy = 0.5;
          d2 = 1;
        }
        const f = (1100 / d2) * a;
        p.vx += dx * f;
        p.vy += dy * f;
        q.vx -= dx * f;
        q.vy -= dy * f;
      }
    }
    for (const [i, j, nested] of g.edges) {
      const p = ns[i];
      const q = ns[j];
      const dx = q.x - p.x;
      const dy = q.y - p.y;
      const d = Math.sqrt(dx * dx + dy * dy) || 1;
      // A dot with many lines moves less for each of them, so a hub stays calm.
      const f = ((d - (nested ? 60 : 78)) / d) * a * (0.35 / Math.max(1, Math.min(p.n, q.n)));
      const share = p.n / (p.n + q.n);
      p.vx += dx * f * (1 - share);
      p.vy += dy * f * (1 - share);
      q.vx -= dx * f * share;
      q.vy -= dy * f * share;
    }
    for (const p of ns) {
      if (g.drag && g.drag.node === p) {
        p.vx = 0;
        p.vy = 0;
        continue;
      }
      // A soft pull to the middle keeps the dots with no lines in view.
      // (Stronger for a page with no lines: groups and lone pages stay close.)
      p.vx = (p.vx - p.x * (p.n ? 0.075 : 0.1) * a) * 0.78;
      p.vy = (p.vy - p.y * (p.n ? 0.075 : 0.1) * a) * 0.78;
      p.x += p.vx;
      p.y += p.vy;
    }
    g.alpha *= 0.985;
  };

  // The view that shows all of it, in the middle.
  const whole = () => {
    if (!g.nodes.length) return { x: 0, y: 0, k: 1 };
    let x0 = Infinity;
    let x1 = -Infinity;
    let y0 = Infinity;
    let y1 = -Infinity;
    for (const n of g.nodes) {
      x0 = Math.min(x0, n.x);
      x1 = Math.max(x1, n.x);
      y0 = Math.min(y0, n.y);
      y1 = Math.max(y1, n.y);
    }
    // Room at the edges for the dots themselves and the names under them.
    const k = Math.max(0.12, Math.min(1.15, Math.min((g.w - 160) / (x1 - x0 + 1), (g.h - 170) / (y1 - y0 + 1))));
    return { k, x: -((x0 + x1) / 2) * k, y: -((y0 + y1) / 2) * k + 8 };
  };
  const fit = () => {
    if (!g.w) return;
    g.fitted = true;
    g.held = false;
    g.view = whole();
  };
  // While the dots settle, the view goes with them: none ends out of view.
  // Not once the person has moved or zoomed the picture (`held`).
  const follow = () => {
    if (g.held || !g.w) return;
    const to = whole();
    g.view.k += (to.k - g.view.k) * 0.2;
    g.view.x += (to.x - g.view.x) * 0.2;
    g.view.y += (to.y - g.view.y) * 0.2;
  };

  const draw = () => {
    if (!g.w) return;
    const cs = getComputedStyle(el);
    const ink = cs.color;
    const tints = {};
    const tint = (c) => {
      if (!c || c === 'gray') return '';
      if (!(c in tints)) tints[c] = cs.getPropertyValue('--icon-' + c).trim();
      return tints[c];
    };
    ctx.clearRect(0, 0, g.w, g.h);
    const focus = g.hover;
    const found = g.find ? g.nodes.map((n) => n.t.toLowerCase().includes(g.find)) : null;
    const lit = (i) => (focus >= 0 ? i === focus || g.near[focus].has(i) : found ? found[i] : true);
    const picking = focus >= 0 || !!found;
    // The lines.
    ctx.lineWidth = 1;
    ctx.strokeStyle = ink;
    for (const [a, b, nested] of g.edges) {
      const on = focus >= 0 ? a === focus || b === focus : found ? found[a] && found[b] : false;
      ctx.globalAlpha = on ? 0.8 : picking ? 0.06 : nested ? 0.28 : 0.4;
      ctx.setLineDash(nested ? [3, 4] : []);
      ctx.beginPath();
      ctx.moveTo(X(g.nodes[a]), Y(g.nodes[a]));
      ctx.lineTo(X(g.nodes[b]), Y(g.nodes[b]));
      ctx.stroke();
    }
    ctx.setLineDash([]);
    // The dots.
    const scale = dotScale();
    g.nodes.forEach((n, i) => {
      const color = tint(n.c);
      const on = lit(i);
      if (on && color) {
        ctx.globalAlpha = 0.22;
        ctx.fillStyle = color;
        ctx.beginPath();
        ctx.arc(X(n), Y(n), n.r * scale + 5, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.globalAlpha = on ? (color ? 1 : 0.85) : 0.16;
      ctx.fillStyle = color || ink;
      ctx.beginPath();
      ctx.arc(X(n), Y(n), n.r * scale, 0, Math.PI * 2);
      ctx.fill();
      if (i === focus) {
        ctx.globalAlpha = 1;
        ctx.strokeStyle = ink;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(X(n), Y(n), n.r * scale + 3, 0, Math.PI * 2);
        ctx.stroke();
        ctx.lineWidth = 1;
      }
    });
    // The names: all of them when there are few pages or the view is close;
    // else those of the pages with many lines, and of what is picked.
    ctx.font = '12.5px -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    ctx.fillStyle = ink;
    // A name is drawn only where it has room: what is picked first, then
    // the pages with the most lines. So the names never lie over each other.
    const order = g.nodes.map((_, i) => i).sort((a, b) => (picking ? Number(lit(b)) - Number(lit(a)) : 0) || g.nodes[b].n - g.nodes[a].n || a - b);
    const taken = [];
    for (const i of order) {
      const n = g.nodes[i];
      const strong = picking && lit(i);
      // Very far out, the names would be all there is to see: only what is picked.
      if (!strong && (picking || g.view.k < 0.42)) continue;
      const name = n.t.length > 30 ? n.t.slice(0, 29) + '…' : n.t;
      const w = ctx.measureText(name).width;
      const box = { x0: X(n) - w / 2 - 4, x1: X(n) + w / 2 + 4, y0: Y(n) + n.r * scale + 4, y1: Y(n) + n.r * scale + 22 };
      if (i !== focus && taken.some((t) => box.x0 < t.x1 && box.x1 > t.x0 && box.y0 < t.y1 && box.y1 > t.y0)) continue;
      taken.push(box);
      ctx.globalAlpha = strong ? 1 : n.n > 1 ? 0.9 : 0.72;
      ctx.fillText(name, X(n), box.y0 + 4);
    }
    ctx.globalAlpha = 1;
  };

  const frame = () => {
    g.raf = 0;
    if (!el.isConnected) {
      off.abort(); // the view was closed
      return;
    }
    if (g.alpha > GRAPH_REST || (g.drag && g.drag.node)) {
      tick();
      follow();
      draw();
      g.raf = requestAnimationFrame(frame);
    } else {
      if (!g.held) g.view = whole();
      draw();
    }
  };
  const run = () => {
    if (!g.raf) g.raf = requestAnimationFrame(frame);
  };
  const heat = (to) => {
    g.alpha = Math.max(g.alpha, to);
    if (!g.fitted) {
      // The first time: let it settle before it shows, then put it in view.
      for (let i = 0; i < (calm ? 500 : 160) && g.alpha > GRAPH_REST; i++) tick();
      fit();
    } else if (calm) {
      while (g.alpha > GRAPH_REST) tick();
      if (!g.held) g.view = whole();
    }
    run();
  };

  const read = () => {
    let data = {};
    try {
      data = JSON.parse(el.dataset.graph || '{}');
    } catch (_) {
      data = {};
    }
    const old = new Map(g.nodes.map((n) => [n.id, n]));
    const nodes = (data.nodes || []).map((d, i) => {
      const was = old.get(d.id);
      // A new dot starts on a spiral around the middle: no two in one place.
      const r = 26 * Math.sqrt(i + 0.5);
      const a = i * 2.399963;
      return { id: d.id, t: d.t || 'Untitled', c: d.c || '', x: was ? was.x : Math.cos(a) * r, y: was ? was.y : Math.sin(a) * r, vx: 0, vy: 0, n: 0, r: 4 };
    });
    const edges = (data.edges || []).filter((e) => nodes[e[0]] && nodes[e[1]]);
    const near = nodes.map(() => new Set());
    for (const [a, b] of edges) {
      nodes[a].n++;
      nodes[b].n++;
      near[a].add(b);
      near[b].add(a);
    }
    for (const n of nodes) n.r = 6 + Math.min(9, Math.sqrt(n.n) * 2.6);
    const sig = nodes.map((n) => n.id).join(',') + '|' + edges.map((e) => e.join('-')).join(',');
    const changed = sig !== g.sig;
    g.nodes = nodes;
    g.edges = edges;
    g.near = near;
    g.hover = -1;
    g.sig = sig;
    // New pages or lines: move again, gently. A new name alone: only draw.
    if (changed) heat(g.fitted ? 0.5 : 1);
    else draw();
  };

  const size = () => {
    const r = el.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const first = !g.w && r.width > 0;
    const other = r.width !== g.w || r.height !== g.h;
    g.w = r.width;
    g.h = r.height;
    canvas.width = Math.max(1, Math.round(r.width * dpr));
    canvas.height = Math.max(1, Math.round(r.height * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    // A new size of the pane: all of it in view again, unless the person placed it.
    if (first && g.nodes.length) fit();
    else if (other && !g.held && g.fitted) g.view = whole();
    draw();
  };

  const hit = (cx, cy) => {
    const r = canvas.getBoundingClientRect();
    const scale = dotScale();
    let best = -1;
    let nearest = Infinity;
    g.nodes.forEach((n, i) => {
      const d = Math.hypot(r.left + X(n) - cx, r.top + Y(n) - cy);
      if (d <= n.r * scale + 6 && d < nearest) {
        nearest = d;
        best = i;
      }
    });
    return best;
  };
  const open = (n) => send({ t: 'open_url', url: 'notion2://page/' + n.id });

  canvas.addEventListener('mousemove', (e) => {
    if (g.drag) return;
    const h = hit(e.clientX, e.clientY);
    if (h === g.hover) return;
    g.hover = h;
    canvas.style.cursor = h >= 0 ? 'pointer' : 'grab';
    draw();
  });
  canvas.addEventListener('mouseleave', () => {
    if (g.drag || g.hover < 0) return;
    g.hover = -1;
    draw();
  });
  canvas.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const i = hit(e.clientX, e.clientY);
    g.drag = { node: i >= 0 ? g.nodes[i] : null, x: e.clientX, y: e.clientY, moved: false, vx: g.view.x, vy: g.view.y };
    canvas.style.cursor = 'grabbing';
    e.preventDefault();
  });
  window.addEventListener(
    'mousemove',
    (e) => {
      const d = g.drag;
      if (!d) return;
      const dx = e.clientX - d.x;
      const dy = e.clientY - d.y;
      if (!d.moved && Math.hypot(dx, dy) < 4) return; // a slip of the hand is a click
      d.moved = true;
      g.held = true;
      if (d.node) {
        const r = canvas.getBoundingClientRect();
        d.node.x = (e.clientX - r.left - g.w / 2 - g.view.x) / g.view.k;
        d.node.y = (e.clientY - r.top - g.h / 2 - g.view.y) / g.view.k;
        g.alpha = Math.max(g.alpha, 0.3);
        run();
      } else {
        g.view.x = d.vx + dx;
        g.view.y = d.vy + dy;
        draw();
      }
    },
    { signal: off.signal },
  );
  window.addEventListener(
    'mouseup',
    () => {
      const d = g.drag;
      if (!d) return;
      g.drag = null;
      canvas.style.cursor = g.hover >= 0 ? 'pointer' : 'grab';
      if (!d.moved && d.node) open(d.node);
    },
    { signal: off.signal },
  );
  canvas.addEventListener(
    'wheel',
    (e) => {
      e.preventDefault();
      const r = canvas.getBoundingClientRect();
      const px = e.clientX - r.left - g.w / 2;
      const py = e.clientY - r.top - g.h / 2;
      const k0 = g.view.k;
      // A pinch arrives as a wheel with Control held, in smaller steps.
      const k1 = Math.max(0.2, Math.min(4, k0 * Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0022))));
      // What is under the pointer stays under it.
      g.view.x = px - ((px - g.view.x) / k0) * k1;
      g.view.y = py - ((py - g.view.y) / k0) * k1;
      g.view.k = k1;
      g.held = true;
      draw();
    },
    { passive: false },
  );
  canvas.addEventListener('dblclick', (e) => {
    if (hit(e.clientX, e.clientY) >= 0) return;
    fit();
    draw();
  });
  const find = el.querySelector('.graph-find');
  if (find) {
    find.addEventListener('input', () => {
      g.find = find.value.trim().toLowerCase();
      draw();
    });
    find.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        const first = g.find && g.nodes.find((n) => n.t.toLowerCase().includes(g.find));
        if (first) open(first);
      } else if (e.key === 'Escape') {
        find.value = '';
        g.find = '';
        draw();
        find.blur();
        e.stopPropagation();
      }
    });
  }
  // Where a page's dot is in the window (the self-test clicks there).
  g.screen = (id) => {
    const n = g.nodes.find((x) => x.id === id);
    const r = canvas.getBoundingClientRect();
    return n ? { x: r.left + X(n), y: r.top + Y(n) } : null;
  };

  if (typeof ResizeObserver !== 'undefined') new ResizeObserver(size).observe(el);
  new MutationObserver(read).observe(el, { attributes: true, attributeFilter: ['data-graph'] });
  // The colours follow the light or dark look.
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', draw, { signal: off.signal });
  size();
  read();
};
document.querySelectorAll('.graph[data-graph]').forEach(startGraph);

// ----- the calendar: drag an event --------------------------------------------------------
// An event in the week or the day can be taken to another time or another
// day, and its lower edge pulled to make it longer or shorter, a quarter of
// an hour at a time. A press that does not move is a click (it picks the
// event). An event of the Mac's own calendars is not moved here.
const QUARTER = 12; // points for a quarter of an hour (48 for an hour)
// An event of the app's own can be dragged; one of the Mac's calendars only
// when the person lets the app change those (Settings, Calendar).
const mayDrag = (el) => !el.dataset.event.startsWith('mac:') || !!el.closest('.calendar[data-mac-write="1"]');
let evDrag = null;
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.button !== 0 || e.ctrlKey || !e.target.closest) return;
    const el = e.target.closest('.cal-event[data-event]');
    if (!el || el.classList.contains('draft') || !mayDrag(el)) return;
    const col = el.closest('.cal-col');
    if (!col) return;
    const cols = [...col.parentElement.querySelectorAll('.cal-col')];
    const box = el.getBoundingClientRect();
    // The lowest part of an event is its edge: pulled, it changes the length.
    const grow = box.height >= 20 && e.clientY > box.bottom - 7;
    evDrag = { el, id: el.dataset.event, x: e.clientX, y: e.clientY, grow, moved: false, at: cols.indexOf(col), count: cols.length, wide: col.getBoundingClientRect().width, high: box.height, was: el.style.height, days: 0, mins: 0 };
  },
  true,
);
window.addEventListener('mousemove', (e) => {
  const d = evDrag;
  if (!d) {
    // The pointer over the lower edge of an event shows that it can be pulled.
    const el = e.target.closest && e.target.closest('.cal-event[data-event]:not(.draft)');
    if (el && mayDrag(el)) {
      const box = el.getBoundingClientRect();
      el.classList.toggle('at-edge', box.height >= 20 && e.clientY > box.bottom - 7);
    }
    return;
  }
  // Let go where the page did not see it (outside the window): put it back.
  if (!(e.buttons & 1)) {
    evDrag = null;
    d.el.classList.remove('dragging');
    document.body.classList.remove('ev-dragging');
    d.el.style.transform = '';
    if (d.moved && d.grow) d.el.style.height = d.was;
    return;
  }
  const dx = e.clientX - d.x;
  const dy = e.clientY - d.y;
  if (!d.moved && Math.hypot(dx, dy) < 5) return;
  if (!d.moved) {
    d.moved = true;
    d.el.classList.add('dragging');
    document.body.classList.add('ev-dragging');
  }
  d.mins = Math.round(dy / QUARTER) * 15;
  if (d.grow) {
    d.mins = Math.max(d.mins, 15 - Math.round(d.high / QUARTER) * 15);
    d.el.style.height = Math.max(QUARTER, d.high + (d.mins / 15) * QUARTER) + 'px';
  } else {
    d.days = Math.max(-d.at, Math.min(d.count - 1 - d.at, Math.round(dx / d.wide)));
    d.el.style.transform = 'translate(' + d.days * d.wide + 'px, ' + (d.mins / 15) * QUARTER + 'px)';
  }
});
window.addEventListener('mouseup', () => {
  const d = evDrag;
  if (!d) return;
  evDrag = null;
  if (!d.moved) return;
  d.el.classList.remove('dragging');
  document.body.classList.remove('ev-dragging');
  // The click that ends a drag does not pick the event.
  const eat = (c) => {
    c.stopPropagation();
    c.preventDefault();
  };
  window.addEventListener('click', eat, { capture: true, once: true });
  setTimeout(() => window.removeEventListener('click', eat, true), 0);
  const settle = () => {
    d.el.style.transform = '';
  };
  const changed = d.grow ? d.mins !== 0 : d.days !== 0 || d.mins !== 0;
  if (!changed) {
    settle();
    d.el.style.height = d.was;
    return;
  }
  // The lift stays until the app has drawn the event in its new place (the
  // framework keeps a style it did not set), so it does not jump back first.
  const seen = new MutationObserver(() => {
    seen.disconnect();
    settle();
  });
  seen.observe(d.el, { attributes: true, attributeFilter: ['style'] });
  setTimeout(() => {
    seen.disconnect();
    settle();
  }, 400);
  send({ t: 'event_move', id: d.id, days: d.grow ? 0 : d.days, minutes: d.grow ? 0 : d.mins, grow: d.grow ? d.mins : 0 });
});

// ----- the calendar: draw a new event, and take a chip to another day ---------------------
// A drag over empty time in a day makes a slot of that length for a new
// event (a click still makes a quarter of an hour). An all-day event, and
// an event in the month, is a chip: it can be taken to another day.
const eatNextClick = () => {
  const eat = (c) => {
    c.stopPropagation();
    c.preventDefault();
  };
  window.addEventListener('click', eat, { capture: true, once: true });
  setTimeout(() => window.removeEventListener('click', eat, true), 0);
};
let evDraw = null;
let chipDrag = null;
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.button !== 0 || e.ctrlKey || !e.target.closest) return;
    const chip = e.target.closest('.cal-chip[data-event]');
    if (chip) {
      const cell = chip.closest('[data-day]');
      if (cell && mayDrag(chip)) chipDrag = { chip, id: chip.dataset.event, from: cell.dataset.day, x: e.clientX, y: e.clientY, moved: false, ghost: null, over: null };
      return;
    }
    const col = e.target.closest('.cal-col[data-day]');
    if (!col || e.target.closest('.cal-event')) return;
    evDraw = { col, y: e.clientY, moved: false, ghost: null, from: 0, to: 0 };
  },
  true,
);
// The minute of the day under the pointer in a column of hours, to the quarter hour.
const minuteAt = (col, clientY) => Math.max(0, Math.min(24 * 60, Math.round(((clientY - col.getBoundingClientRect().top) / 48) * 4) * 15));
const dayCellAt = (x, y) => {
  const el = document.elementFromPoint(x, y);
  return el && el.closest ? el.closest('.cal-day[data-day], .cal-allday-cell[data-day]') : null;
};
window.addEventListener('mousemove', (e) => {
  const d = evDraw;
  if (d) {
    if (!(e.buttons & 1)) {
      evDraw = null;
      if (d.ghost) d.ghost.remove();
      return;
    }
    if (!d.moved && Math.abs(e.clientY - d.y) < 6) return;
    if (!d.moved) {
      d.moved = true;
      d.ghost = document.createElement('div');
      d.ghost.className = 'cal-event draft drawing';
      d.col.appendChild(d.ghost);
    }
    const a = minuteAt(d.col, d.y);
    const b = minuteAt(d.col, e.clientY);
    d.from = Math.min(a, b);
    d.to = Math.max(Math.max(a, b), d.from + 15);
    d.ghost.style.top = (d.from / 60) * 48 + 'px';
    d.ghost.style.height = ((d.to - d.from) / 60) * 48 + 'px';
    return;
  }
  const c = chipDrag;
  if (!c) return;
  if (!(e.buttons & 1)) {
    chipDrag = null;
    if (c.ghost) c.ghost.remove();
    c.chip.classList.remove('lifting');
    if (c.over) c.over.classList.remove('drop-day');
    return;
  }
  if (!c.moved && Math.hypot(e.clientX - c.x, e.clientY - c.y) < 5) return;
  if (!c.moved) {
    c.moved = true;
    const box = c.chip.getBoundingClientRect();
    c.ghost = c.chip.cloneNode(true);
    c.ghost.classList.add('chip-ghost');
    c.ghost.style.width = box.width + 'px';
    c.dx = e.clientX - box.left;
    c.dy = e.clientY - box.top;
    document.body.appendChild(c.ghost);
    c.chip.classList.add('lifting');
  }
  c.ghost.style.left = e.clientX - c.dx + 'px';
  c.ghost.style.top = e.clientY - c.dy + 'px';
  const cell = dayCellAt(e.clientX, e.clientY);
  if (cell !== c.over) {
    if (c.over) c.over.classList.remove('drop-day');
    c.over = cell;
    if (cell && cell.dataset.day !== c.from) cell.classList.add('drop-day');
  }
});
window.addEventListener('mouseup', (e) => {
  const d = evDraw;
  evDraw = null;
  if (d && d.moved) {
    d.ghost.remove();
    eatNextClick();
    send({ t: 'event_draw', day: d.col.dataset.day, from: d.from, to: d.to });
    return;
  }
  const c = chipDrag;
  chipDrag = null;
  if (!c || !c.moved) return;
  c.ghost.remove();
  c.chip.classList.remove('lifting');
  if (c.over) c.over.classList.remove('drop-day');
  eatNextClick();
  const cell = dayCellAt(e.clientX, e.clientY) || c.over;
  if (!cell || cell.dataset.day === c.from) return;
  const days = Math.round((Date.parse(cell.dataset.day + 'T12:00') - Date.parse(c.from + 'T12:00')) / 86400000);
  if (days) send({ t: 'event_move', id: c.id, days, minutes: 0, grow: 0 });
});

// ----- the calendar: a to-do dragged onto a time ------------------------------------------
// A to-do that shows in the calendar (in the day's card, or with the all-day
// events of the week) can be taken onto a time of a day: half an hour is put
// there for it. Its tick box is not a handle; a press that does not move is
// the click it always was.
let todoDrag = null;
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.button !== 0 || e.ctrlKey || !e.target.closest || e.target.closest('.todo-box')) return;
    const row = e.target.closest('.calendar [data-todo-block]');
    if (!row) return;
    const words = row.querySelector('.todo-text, .chip-title');
    todoDrag = { row, page: row.dataset.todoPage, block: row.dataset.todoBlock, words: words ? words.textContent : '', x: e.clientX, y: e.clientY, moved: false, ghost: null, slot: null, col: null, at: 0 };
  },
  true,
);
const endTodoDrag = (d) => {
  if (d.ghost) d.ghost.remove();
  if (d.slot) d.slot.remove();
  d.row.classList.remove('lifting');
};
window.addEventListener('mousemove', (e) => {
  const d = todoDrag;
  if (!d) return;
  if (!(e.buttons & 1)) {
    todoDrag = null;
    endTodoDrag(d);
    return;
  }
  if (!d.moved && Math.hypot(e.clientX - d.x, e.clientY - d.y) < 5) return;
  if (!d.moved) {
    d.moved = true;
    d.ghost = document.createElement('div');
    d.ghost.className = 'cal-chip chip-ghost todo-ghost';
    d.ghost.textContent = d.words;
    document.body.appendChild(d.ghost);
    d.row.classList.add('lifting');
  }
  d.ghost.style.left = e.clientX + 10 + 'px';
  d.ghost.style.top = e.clientY + 8 + 'px';
  const under = document.elementFromPoint(e.clientX, e.clientY);
  const col = under && under.closest ? under.closest('.cal-col[data-day]') : null;
  if (col !== d.col && d.slot) {
    d.slot.remove();
    d.slot = null;
  }
  d.col = col;
  if (!col) return;
  if (!d.slot) {
    d.slot = document.createElement('div');
    d.slot.className = 'cal-event draft drawing';
    d.slot.style.height = '24px';
    col.appendChild(d.slot);
  }
  d.at = Math.min(minuteAt(col, e.clientY), 24 * 60 - 30);
  d.slot.style.top = (d.at / 60) * 48 + 'px';
});
window.addEventListener('mouseup', () => {
  const d = todoDrag;
  todoDrag = null;
  if (!d || !d.moved) return;
  endTodoDrag(d);
  eatNextClick();
  if (d.col) send({ t: 'todo_plan', page: d.page, block: d.block, day: d.col.dataset.day, minutes: d.at });
});

// ----- the video player: where its frame is ------------------------------------------
// The player is a view of its own over the frame the page draws for it
// (#player-screen). Rust has to know where the frame is: when it opens, and
// when the window changes size.
window.__n2_player = () => {
  const frame = document.getElementById('player-screen');
  if (!frame) return;
  const r = frame.getBoundingClientRect();
  send({ t: 'player_place', x: r.left, y: r.top, w: r.width, h: r.height });
};
addEventListener('resize', () => requestAnimationFrame(window.__n2_player));

// The player is moved by its bar and gets another size by a corner. While
// that goes on the page moves the dock and the player follows it; at the end
// Rust is told the place (so far from the two nearest edges of the window)
// and the width, keeps them, and draws the dock there.
(() => {
  const EDGE = 8;
  const LEAST = 260;
  let drag = null;
  const follow = () => window.__n2_player && window.__n2_player();
  const tell = (dock, w) => {
    const r = dock.getBoundingClientRect();
    const h = r.left + r.width / 2 < innerWidth / 2 ? 'left' : 'right';
    const v = r.top + r.height / 2 < innerHeight / 2 ? 'top' : 'bottom';
    send({ t: 'player_dock', h, v, x: Math.round(h === 'left' ? r.left : innerWidth - r.right), y: Math.round(v === 'top' ? r.top : innerHeight - r.bottom), w: Math.round(w || r.width) });
  };
  const end = (keep) => {
    const d = drag;
    if (!d) return;
    drag = null;
    d.dock.classList.remove('moving', 'sizing');
    if (!d.moved) return;
    if (keep) {
      tell(d.dock);
      eatNextClick();
    } else {
      d.dock.style.cssText = d.was;
    }
    follow();
  };
  document.addEventListener(
    'mousedown',
    (e) => {
      if (e.button !== 0 || !(e.target instanceof Element)) return;
      const dock = e.target.closest('#player-dock');
      if (!dock) return;
      const grip = e.target.closest('.player-grip');
      if (!grip && (!e.target.closest('.player-bar') || e.target.closest('button'))) return;
      drag = { dock, corner: grip ? grip.dataset.corner : '', x: e.clientX, y: e.clientY, r: dock.getBoundingClientRect(), was: dock.style.cssText, moved: false, raf: 0 };
      e.preventDefault();
    },
    true,
  );
  window.addEventListener('mousemove', (e) => {
    const d = drag;
    if (!d) return;
    // The button came up somewhere the page did not see: it is over.
    if (!(e.buttons & 1)) return end(false);
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    if (!d.moved) {
      if (Math.abs(dx) + Math.abs(dy) < 3) return;
      d.moved = true;
      d.dock.classList.add(d.corner ? 'sizing' : 'moving');
      Object.assign(d.dock.style, { left: d.r.left + 'px', top: d.r.top + 'px', right: 'auto', bottom: 'auto', width: d.r.width + 'px' });
    }
    if (!d.corner) {
      d.dock.style.left = Math.max(EDGE, Math.min(d.r.left + dx, innerWidth - d.r.width - EDGE)) + 'px';
      d.dock.style.top = Math.max(EDGE, Math.min(d.r.top + dy, innerHeight - d.r.height - EDGE)) + 'px';
    } else {
      // The corner across from the one that is pulled stays where it is;
      // the frame keeps its shape (16 to 9), so the height comes with the width.
      const east = d.corner.includes('e');
      const south = d.corner.includes('s');
      const across = east ? dx : -dx;
      const down = (south ? dy : -dy) * (16 / 9);
      const more = Math.abs(across) >= Math.abs(down) ? across : down;
      const rest = d.r.height - (d.r.width * 9) / 16; // the bar and the room round the frame
      const roomAcross = east ? innerWidth - EDGE - d.r.left : d.r.right - EDGE;
      const roomDown = south ? innerHeight - EDGE - d.r.top : d.r.bottom - EDGE;
      const most = Math.max(LEAST, Math.min(roomAcross, ((roomDown - rest) * 16) / 9));
      const w = Math.max(LEAST, Math.min(d.r.width + more, most));
      const high = rest + (w * 9) / 16;
      d.dock.style.width = w + 'px';
      d.dock.style.left = (east ? d.r.left : d.r.right - w) + 'px';
      d.dock.style.top = (south ? d.r.top : d.r.bottom - high) + 'px';
    }
    if (!d.raf) {
      d.raf = requestAnimationFrame(() => {
        d.raf = 0;
        follow();
      });
    }
  });
  window.addEventListener('mouseup', () => end(true));
  // Two clicks on the bar: large, or small again.
  document.addEventListener('dblclick', (e) => {
    if (!(e.target instanceof Element) || e.target.closest('button')) return;
    const bar = e.target.closest('#player-dock .player-bar');
    if (!bar) return;
    const dock = bar.closest('#player-dock');
    const now = dock.getBoundingClientRect().width;
    const large = Math.round(Math.min(860, innerWidth - 80, ((innerHeight - 110) * 16) / 9));
    send({ t: 'player_dock', h: dock.dataset.h, v: dock.dataset.v, x: Number(dock.dataset.x), y: Number(dock.dataset.y), w: now < large - 40 ? large : 440 });
  });
})();

// ----- input methods (Japanese, Chinese, Korean…): Return that confirms a word ----
// While a word is being put together, Return, Escape and the arrows belong to
// the input method. In the one-line fields of the app (a name, the search,
// a quick note, a new event) they must not reach the app, which would save
// or send what is typed too early. (The editor of a page has its own guard.)
window.addEventListener(
  'keydown',
  (e) => {
    if (!(e.isComposing || e.keyCode === 229)) return;
    if (!(e.target instanceof Element) || !e.target.matches('input, textarea')) return;
    if (['Enter', 'Escape', 'ArrowDown', 'ArrowUp', 'Tab'].includes(e.key)) e.stopPropagation();
  },
  true,
);

// ----- the sidebar: the list of the last space slides out as the new one comes in --
// Rust puts a new list in place of the old one when the space changes. A
// picture of the old list (a copy, which takes no pointer) stays for a moment
// and slides away to the side the new one does not come from.
(() => {
  let top = 0; // how far the list was scrolled (a list that is taken out no longer says)
  document.addEventListener(
    'scroll',
    (e) => {
      if (e.target instanceof Element && e.target.classList.contains('side-scroll') && e.target.isConnected && !e.target.closest('.side-ghost')) top = e.target.scrollTop;
    },
    true,
  );
  const slideOut = (side, gone, came) => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    side.querySelectorAll('.side-ghost').forEach((g) => g.remove());
    const box = document.createElement('div');
    box.className = 'side-ghost';
    box.setAttribute('aria-hidden', 'true');
    Object.assign(box.style, { left: came.offsetLeft + 'px', top: came.offsetTop + 'px', width: came.offsetWidth + 'px', height: came.offsetHeight + 'px' });
    const copy = gone.cloneNode(true);
    copy.querySelectorAll('.popover').forEach((m) => m.remove());
    copy.classList.remove('from-next', 'from-prev');
    // The copy starts where the fingers left the list (it may lean already).
    const from = { transform: copy.style.transform || 'translateX(0)', opacity: copy.style.opacity || '1' };
    copy.style.transition = 'none';
    box.appendChild(copy);
    side.insertBefore(box, came);
    copy.scrollTop = top;
    top = 0;
    const away = came.classList.contains('from-prev') ? 46 : -46;
    const move = copy.animate([from, { transform: `translateX(${away}px)`, opacity: '0' }], { duration: 210, easing: 'cubic-bezier(0.4, 0, 0.7, 1)', fill: 'forwards' });
    const done = () => box.remove();
    move.onfinish = done;
    move.oncancel = done;
    setTimeout(done, 400);
  };
  new MutationObserver((records) => {
    let gone = null;
    let side = null;
    for (const rec of records) {
      for (const n of rec.removedNodes) {
        if (n.nodeType === 1 && n.classList.contains('side-scroll') && rec.target instanceof Element && rec.target.classList.contains('sidebar')) {
          gone = n;
          side = rec.target;
        }
      }
    }
    if (!gone || !side || !side.isConnected) return;
    const came = [...side.children].find((c) => c.classList.contains('side-scroll'));
    if (came && came.dataset.space !== gone.dataset.space) slideOut(side, gone, came);
  }).observe(document.documentElement, { childList: true, subtree: true });
})();

// ----- the sidebar: two fingers moved sideways go to another space ----------
// One space for each move of the fingers, and a light move is enough. The
// list leans with the fingers, so the move is felt before the space changes;
// if the fingers stop short, or there is no space on that side, it goes back.
// After a move the trackpad keeps sending steps (the rest of the move, then
// the glide): those are not a new move. A pause is, or a step the other way,
// or steps that were small (the end of the glide, or the fingers down again)
// and then are large again: the next move, with no pause before it. Without
// that last sign a second move soon after the first was lost, and with three
// spaces the sidebar stopped at the middle one. A soft move is seen the same
// way: after the steps came nearly to rest, steps that climb are the fingers.
// (The glide only gets weaker; it never climbs.)
(() => {
  const NEED = 30; // how far the fingers go, in all, before the space changes
  let across = 0; // this move so far, sideways
  let down = 0; // and up or down
  let upDown = false; // this move is for the list itself
  let last = 0;
  let done = 0; // when this move changed the space (0: it has not)
  let dir = 0; // which way that move went
  let quiet = 0; // small steps in a row, after the space changed
  let low = Infinity; // the weakest step since the space changed
  let prev = 0; // the step before this one
  let climb = 0; // how far the steps went since that weakest one
  // For a fault that cannot be seen from here: the steps of the fingers and
  // what was made of them, as numbers, kept in a file on this Mac. It is on
  // only when the app says so (see `swipe_trace` in state.rs).
  const trace = [];
  let traceSoon = 0;
  const note = (e, now, what) => {
    if (document.body.dataset.swipeLog !== '1') return;
    trace.push(`${Math.round(now)} ${e.deltaX} ${e.deltaY} ${what}`);
    clearTimeout(traceSoon);
    traceSoon = setTimeout(() => send({ t: 'swipe_trace', lines: trace.splice(0).join('\n') }), 1500);
  };
  let rx = 0; // the last few steps: sideways,
  let ry = 0; // and up or down
  let rest = 0;
  const list = () => document.querySelector('.sidebar .side-scroll');
  const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  // On the sidebar: by what is under the pointer, or by where the pointer is
  // (the sidebar that floats lies over the page).
  const onSidebar = (e) => {
    if (e.target instanceof Element && e.target.closest('.sidebar')) return true;
    const side = document.querySelector('.sidebar');
    if (!side || (side.classList.contains('floating') && document.body.dataset.peek !== '1')) return false;
    const r = side.getBoundingClientRect();
    return e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
  };
  const lean = () => {
    const el = list();
    if (!el || still()) return;
    const part = Math.max(-1, Math.min(1, across / NEED));
    el.style.transition = 'none';
    el.style.transform = `translateX(${(-part * 24).toFixed(1)}px)`;
    el.style.opacity = (1 - Math.abs(part) * 0.4).toFixed(2);
  };
  const settle = () => {
    const el = list();
    if (!el || (!el.style.transform && !el.style.opacity)) return;
    el.style.transition = 'transform 280ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease-out';
    el.style.transform = '';
    el.style.opacity = '';
    setTimeout(() => {
      if (el.isConnected) el.style.transition = '';
    }, 300);
  };
  const fresh = () => {
    across = 0;
    down = 0;
    upDown = false;
    done = 0;
    quiet = 0;
    low = Infinity;
    prev = 0;
    climb = 0;
  };
  document.addEventListener(
    'wheel',
    (e) => {
      if (e.ctrlKey || !onSidebar(e)) return;
      // (The time the trackpad made the step, not the time the page got to it.)
      const now = e.timeStamp || performance.now();
      const step = Math.abs(e.deltaX);
      const gap = now - last;
      last = now;
      rx = (gap > 250 ? 0 : rx * 0.7) + step;
      ry = (gap > 250 ? 0 : ry * 0.7) + Math.abs(e.deltaY);
      if (done) {
        // One space for one move. The move and its glide come as steps with
        // no pause between them, in one direction. A new move is a pause, or
        // a clear step the other way, or a large step after two or more
        // small ones. (One step that is larger than the last is not a sign:
        // the fingers go faster and slower in the middle of one move.)
        const back = e.deltaX !== 0 && Math.sign(e.deltaX) === -dir && step >= 3;
        let again = false;
        if (step <= 4) {
          quiet += 1;
        } else {
          again = step >= 8 && quiet >= 2;
          quiet = 0;
        }
        // A soft move: the steps came nearly to rest, and now they climb.
        const soft = low <= 2 && prev >= low + 1 && step >= Math.max(low + 3, 4);
        if (step <= low) {
          low = step;
          climb = 0;
        }
        climb += e.deltaX;
        prev = step;
        if (now - done < 120 || (gap <= 60 && !back && !again && !soft)) {
          note(e, now, 'glide');
          return;
        }
        note(e, now, back ? 'new:back' : again ? 'new:strong' : soft ? 'new:soft' : 'new:pause');
        // The steps of the climb are the start of the new move: they count
        // for it (a soft move is short, and would stop short without them).
        const carried = again || soft ? climb - e.deltaX : 0;
        fresh();
        across = carried;
      } else if (gap > 250) {
        fresh(); // a long pause before the space changed: start again
      }
      if (upDown) {
        // The move turned sideways: it is a move for the spaces after all.
        if (!(rx > 14 && rx > ry * 2)) {
          note(e, now, 'list');
          return;
        }
        fresh();
      }
      across += e.deltaX;
      down += e.deltaY;
      // Up and down is the list itself: this move is not for the spaces.
      if (Math.abs(down) >= 24 && Math.abs(down) > Math.abs(across) * 1.5) {
        upDown = true;
        settle();
        return;
      }
      // Not clearly sideways yet.
      if (Math.abs(across) < 3 || Math.abs(across) < Math.abs(down)) return;
      note(e, now, `lean:${Math.round(across)}`);
      lean();
      clearTimeout(rest);
      if (Math.abs(across) < NEED) {
        // The fingers stopped short: the list goes back.
        rest = setTimeout(() => {
          fresh();
          settle();
        }, 170);
        return;
      }
      done = now;
      dir = Math.sign(across);
      note(e, now, across > 0 ? 'CHANGE:next' : 'CHANGE:prev');
      const el = list();
      const space = el && el.dataset.space;
      send({ t: 'shortcut', name: across > 0 ? 'space_next' : 'space_prev' });
      // No space on that side: the same list is still there, and goes back.
      rest = setTimeout(() => {
        const same = list();
        if (same && same.dataset.space === space) settle();
      }, 160);
    },
    { passive: true, capture: true },
  );
})();

// ----- diagrams: code blocks of the language Mermaid ---------------------------
//
// The block shows `.diagram[data-diagram]` with the code; this draws it with
// Mermaid (built into the app, see assets/mermaid), in the colours of the app.
// The library loads the first time a page shows a diagram.

const DIAGRAM_LIB = 'n2draw://app/mermaid.min.js';
let diagramLib = null;
const loadDiagramLib = () => {
  if (!diagramLib) {
    diagramLib = new Promise((ok, no) => {
      const s = document.createElement('script');
      s.src = DIAGRAM_LIB;
      s.onload = () => (window.mermaid ? ok(window.mermaid) : no(new Error('The diagram drawer did not start.')));
      s.onerror = () => {
        diagramLib = null;
        no(new Error('The diagram drawer could not be loaded.'));
      };
      document.head.appendChild(s);
    });
  }
  return diagramLib;
};

// A colour of the app as an opaque "#rrggbb": see-through ones are laid
// over the page's own ground, as they show.
const diagramColors = () => {
  const app = document.querySelector('.app') || document.body;
  const probe = document.createElement('span');
  probe.style.display = 'none';
  app.appendChild(probe);
  const rgba = (token) => {
    probe.style.color = `var(${token})`;
    const m = getComputedStyle(probe).color.match(/[\d.]+/g) || ['0', '0', '0'];
    return m.map(Number).concat(m.length < 4 ? [1] : []);
  };
  const ground = rgba('--bg');
  const hex = (token) => {
    const [r, g, b, a] = rgba(token);
    return '#' + [r, g, b].map((v, i) => Math.round(v * a + ground[i] * (1 - a)).toString(16).padStart(2, '0')).join('');
  };
  const look = {
    text: hex('--text'),
    soft: hex('--text-2'),
    fill: hex('--fill-3'),
    fill2: hex('--fill-4'),
    edge: hex('--sep'),
    accent: hex('--accent'),
    ground: hex('--bg'),
  };
  probe.remove();
  look.dark = parseInt(look.ground.slice(1, 3), 16) < 128;
  return look;
};

let diagramCount = 0;
let diagramLook = '';
const drawDiagram = async (el, look, key) => {
  const mermaid = await loadDiagramLib();
  if (diagramLook !== key) {
    diagramLook = key;
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      theme: 'base',
      fontFamily: `${getComputedStyle(document.body).fontFamily}, -apple-system, BlinkMacSystemFont, "Helvetica Neue", Helvetica, Arial, sans-serif`,
      themeVariables: {
        darkMode: look.dark,
        background: look.ground,
        fontSize: '14px',
        primaryColor: look.fill,
        primaryTextColor: look.text,
        primaryBorderColor: look.edge,
        secondaryColor: look.fill2,
        secondaryTextColor: look.text,
        secondaryBorderColor: look.edge,
        tertiaryColor: look.fill2,
        tertiaryTextColor: look.text,
        tertiaryBorderColor: look.edge,
        lineColor: look.soft,
        textColor: look.text,
        mainBkg: look.fill,
        nodeBorder: look.edge,
        clusterBkg: look.fill2,
        clusterBorder: look.edge,
        edgeLabelBackground: look.ground,
        actorBkg: look.fill,
        actorBorder: look.edge,
        actorTextColor: look.text,
        signalColor: look.soft,
        signalTextColor: look.text,
        noteBkgColor: look.fill2,
        noteTextColor: look.text,
        noteBorderColor: look.edge,
      },
      flowchart: { curve: 'basis', padding: 14, htmlLabels: true },
    });
  }
  const source = el.dataset.diagram;
  // (Its own name: Mermaid works in a part of the page with that name.)
  const name = `n2-diagram-${++diagramCount}`;
  try {
    await mermaid.parse(source);
    const { svg } = await mermaid.render(name, source);
    if (el.dataset.diagram !== source) return; // changed while it drew
    el.innerHTML = svg;
    // Round corners on the plain boxes, as the app's cards have.
    el.querySelectorAll('.node rect.label-container, .node rect.basic').forEach((r) => {
      if (!r.getAttribute('rx') || r.getAttribute('rx') === '0') {
        r.setAttribute('rx', '6');
        r.setAttribute('ry', '6');
      }
    });
    el.classList.remove('wrong');
  } catch (e) {
    if (el.dataset.diagram !== source) return;
    // Mermaid leaves its own error drawing in the page: take it away.
    document.querySelectorAll(`#d${name}, #${name}`).forEach((n) => n.remove());
    el.classList.add('wrong');
    const line = String((e && e.message) || e).split('\n').filter((l) => l.trim()).slice(0, 3).join(' ');
    el.textContent = `This diagram cannot be drawn: ${line}`;
  }
  el.dataset.drawn = `${key}\n${source}`;
};

let diagramsQueued = false;
let diagramQueue = Promise.resolve();
const drawDiagrams = () => {
  if (diagramsQueued) return;
  diagramsQueued = true;
  requestAnimationFrame(() => {
    diagramsQueued = false;
    const all = document.querySelectorAll('.diagram[data-diagram]');
    if (!all.length) return;
    const look = diagramColors();
    const key = JSON.stringify(look);
    all.forEach((el) => {
      if (el.dataset.drawn !== `${key}\n${el.dataset.diagram}`) {
        el.dataset.drawn = `${key}\n${el.dataset.diagram}`;
        // One at a time: Mermaid cannot draw two diagrams at once.
        diagramQueue = diagramQueue
          .then(() => drawDiagram(el, look, key))
          .catch((e) => {
            el.classList.add('wrong');
            el.textContent = String((e && e.message) || e);
          });
      }
    });
  });
};
new MutationObserver((mutations) => {
  for (const m of mutations) {
    if (m.type === 'attributes' ? m.target.classList?.contains('diagram') || m.target.classList?.contains('app') : [...m.addedNodes].some((n) => n.nodeType === 1 && (n.matches('.diagram') || n.querySelector('.diagram')))) {
      drawDiagrams();
      return;
    }
  }
}).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-diagram', 'class'] });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', drawDiagrams);
drawDiagrams();

window.__n2_test = { selIn, setSel, command: (name) => send({ t: 'shortcut', name }) };

// Keep this script alive so dioxus.send() stays connected.
await new Promise(() => {});
