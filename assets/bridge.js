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
  // At the end of a line, the engine keeps an empty last line (after
  // Shift+Return, or Return in code) with one more line end, in the text
  // itself, where the app writes a <br>. That one is no part of the words:
  // kept, each such press at the end stored a line end too many.
  if (trailing && trailing.nodeType === 3 && runs.length) {
    const last = runs[runs.length - 1];
    if (last.text.endsWith('\n')) {
      last.text = last.text.slice(0, -1);
      if (!last.text) runs.pop();
    }
  }
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

// Tell Rust the block's current text, and where the caret is in it: what is
// typed for the "/", "@" and "[[" lists ends at the caret, not at the end
// of the line.
const commit = (el) => {
  if (!el.textContent && el.querySelector('br') && !el.querySelector('br + *')) el.innerHTML = '';
  // (Not in a code block: it has no such lists, and the caret is not free to measure.)
  const s = isCode(el) ? null : selIn(el);
  send({ t: 'input', page: el.dataset.page, id: el.dataset.id, md: mdOf(el), pos: s && s.start === s.end ? s.end : undefined });
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
    // The other formats the caret is in, inside the one that stops (italic
    // inside bold): they go on. (The caret left them all, and ⌘B at the end
    // of bold italic text gave plain text.)
    const others = [];
    for (let k = range.startContainer.nodeType === 1 ? range.startContainer : range.startContainer.parentElement; k && k !== inside; k = k.parentElement) {
      if (['STRONG', 'B', 'EM', 'I', 'S', 'CODE', 'U', 'MARK'].includes(k.tagName)) others.push(k);
    }
    // Step out: split the formatted element at the caret.
    const tail = document.createRange();
    tail.setStart(range.startContainer, range.startOffset);
    tail.setEnd(inside, inside.childNodes.length);
    const rest = tail.extractContents();
    const clone = inside.cloneNode(false);
    clone.appendChild(rest);
    let holder = mark;
    for (const k of others) {
      const wrap = k.cloneNode(false);
      wrap.appendChild(holder);
      holder = wrap;
    }
    inside.after(holder);
    if (clone.textContent) holder.after(clone);
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
  // (No "say what to do" in a locked page: the AI does not read it.)
  b.classList.toggle('no-ai', !!el.closest('.page.no-ai'));
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

// ----- focus mode ----------------------------------------------------------------
// The line with the caret is marked (style.css makes the others pale), and
// while keys are pressed the line stays near the middle of its pane. A press
// of the mouse moves nothing: only typing and the arrows do.

const focusOn = () => !!document.querySelector('.app.focus-mode');
let focusKeyAt = 0;
document.addEventListener('keydown', () => (focusKeyAt = performance.now()), true);

const caretRect = () => {
  const sel = getSelection();
  if (!sel || !sel.rangeCount) return null;
  const range = sel.getRangeAt(0).cloneRange();
  range.collapse(false);
  const rects = range.getClientRects();
  if (rects.length) return rects[rects.length - 1];
  const el = blockOf(sel.focusNode);
  return el ? el.getBoundingClientRect() : null;
};

const markFocusLine = () => {
  const sel = getSelection();
  const blk = sel && sel.rangeCount ? blockOf(sel.focusNode) : null;
  const row = blk && blk.closest('.block-row');
  for (const old of document.querySelectorAll('.block-row.fm-here')) if (old !== row) old.classList.remove('fm-here');
  if (row) row.classList.add('fm-here');
  return row;
};

const centreFocusLine = (always) => {
  const row = markFocusLine();
  // (In typewriter mode the line has its own place: see below.)
  if (typewriterOn()) return;
  if (!row || (!always && performance.now() - focusKeyAt > 400)) return;
  const scroller = row.closest('.scroller');
  const r = caretRect();
  if (!scroller || !r || (!r.height && !r.top)) return;
  const box = scroller.getBoundingClientRect();
  const delta = r.top + r.height / 2 - (box.top + box.height * 0.42);
  if (Math.abs(delta) < 24) return;
  // (The measures are in points of the window, the scroll in the page's own:
  // on a zoomed page the line went past the middle at each key, and at 200%
  // it never came to rest.)
  scroller.scrollTo({ top: scroller.scrollTop + delta / zoomOf(scroller), behavior: Math.abs(delta) > 160 ? 'smooth' : 'instant' });
};

document.addEventListener('selectionchange', () => {
  if (focusOn()) requestAnimationFrame(() => centreFocusLine(false));
});

// ----- typewriter mode -------------------------------------------------------------
// View ▸ Typewriter Mode: as keys are pressed (typing, the arrows, Return),
// the line with the caret is kept at 45 % of the height of its pane, with a
// short glide (a jump when the Mac asks for less motion). The page has room
// at its end for the last line to come up to there (style.css). A press or a
// drag of the mouse moves nothing, nor does the caret in the title or in a
// table (a cell keeps its own scroll). With focus mode too, this places the line.
function typewriterOn() {
  return !!document.querySelector('.app.typewriter');
}
const TYPEWRITER_AT = 0.45;
let glide = 0;
// When a key was pressed last. A press of the mouse ends its time: the
// caret it puts, or a selection it draws, moves nothing.
let typedAt = 0;
document.addEventListener('keydown', () => (typedAt = performance.now()), true);
document.addEventListener('mousedown', () => (typedAt = 0), true);
const glideTo = (scroller, top) => {
  cancelAnimationFrame(glide);
  const from = scroller.scrollTop;
  if (Math.abs(top - from) < 1 || matchMedia('(prefers-reduced-motion: reduce)').matches) {
    scroller.scrollTop = top;
    return;
  }
  const t0 = performance.now();
  const step = (now) => {
    const k = Math.min(1, (now - t0) / 150);
    scroller.scrollTop = from + (top - from) * (1 - Math.pow(1 - k, 3));
    if (k < 1) glide = requestAnimationFrame(step);
  };
  glide = requestAnimationFrame(step);
};
const typewriterLine = (always) => {
  if (!typewriterOn() || (!always && performance.now() - typedAt > 400)) return;
  const sel = getSelection();
  const blk = sel && sel.rangeCount ? blockOf(sel.focusNode) : null;
  if (!blk || blk.closest('.table-block, .row-values')) return;
  const scroller = blk.closest('.scroller');
  const r = caretRect();
  if (!scroller || !r || (!r.height && !r.top)) return;
  const box = scroller.getBoundingClientRect();
  const delta = r.top + r.height / 2 - (box.top + box.height * TYPEWRITER_AT);
  if (Math.abs(delta) < 2) return;
  // (In points of the window; the scroll is in the page's own: see zoomOf.)
  const most = scroller.scrollHeight - scroller.clientHeight;
  glideTo(scroller, Math.max(0, Math.min(most, scroller.scrollTop + delta / zoomOf(scroller))));
};
document.addEventListener('selectionchange', () => {
  if (typewriterOn()) requestAnimationFrame(() => typewriterLine(false));
});

// ----- API used from Rust -----------------------------------------------------

window.__rune = {
  // Typewriter mode was turned on: the line with the caret goes to its place.
  typewriter(on) {
    if (on) requestAnimationFrame(() => requestAnimationFrame(() => typewriterLine(true)));
  },
  focusMode(on) {
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (on) centreFocusLine(true);
        else for (const old of document.querySelectorAll('.block-row.fm-here')) old.classList.remove('fm-here');
      }),
    );
  },
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
        // (Come here with the Up or the Down arrow: the caret keeps its
        // place across the page, as in any text.)
        if (navKeep && performance.now() - navKeep.at < 600) caretAtX(el, navKeep.x, navKeep.dir === 'down');
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
      // (A caret at the end of the line stays at its end: what came in was
      // put where it was, as a link in place of "@name".)
      const atEnd = focused && focused.start === focused.end && focused.end >= textLen(el);
      renderInto(el, html);
      if (focused) {
        const len = textLen(el);
        if (atEnd) setSel(el, len);
        else setSel(el, Math.min(focused.start, len), Math.min(focused.end, len));
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

// The Up and Down arrows that leave a line: how far across the page the
// caret was, so that it comes into the next line at the same place (and
// goes on at that place through short lines, while only those keys are used).
let navKeep = null;
const caretX = () => {
  const sel = getSelection();
  if (!sel.rangeCount) return null;
  const r = sel.getRangeAt(0).cloneRange();
  r.collapse(true);
  const rect = r.getClientRects()[0] || r.getBoundingClientRect();
  return rect && (rect.left || rect.top || rect.bottom) ? rect.left : null;
};
const caretAtX = (el, x, firstLine) => {
  const box = el.getBoundingClientRect();
  if (!box.width || !box.height || !document.caretRangeFromPoint) return;
  const cs = getComputedStyle(el);
  const z = zoomOf(el);
  const line = (parseFloat(cs.lineHeight) || 22) * z;
  const y = firstLine ? box.top + (parseFloat(cs.paddingTop) || 0) * z + line / 2 : box.bottom - (parseFloat(cs.paddingBottom) || 0) * z - line / 2;
  const at = document.caretRangeFromPoint(Math.max(box.left + 1, Math.min(box.right - 1, x)), Math.max(box.top + 1, Math.min(box.bottom - 1, y)));
  if (!at || !el.contains(at.startContainer)) return;
  at.collapse(true);
  const sel = getSelection();
  sel.removeAllRanges();
  sel.addRange(at);
};
const keepNav = (dir) => {
  const went = navKeep && performance.now() - navKeep.at < 2500 ? navKeep.x : null;
  const x = went ?? caretX();
  navKeep = x == null ? null : { x, dir, at: performance.now() };
};
// (Any other key, a press of the mouse: the place is forgotten.)
document.addEventListener('mousedown', () => (navKeep = null), true);

// Ctrl+Tab walks the tabs in the order of use while Control is held (see
// `walk_tabs` in state.rs); when Control is let go, the tab reached is the
// one used last. (A key let go with no Control, or the window put away,
// also ends it: the release of Control itself can be missed.)
// The list of the tabs in that order (`TabWalk` in chrome.rs) shows only when
// Control is still held a moment after the first Tab: a quick Ctrl+Tab
// switches with no list flashing up.
let tabWalking = false;
let tabWalkSoon = 0;
const TAB_WALK_LIST_MS = 150;
const tabWalkStart = () => {
  tabWalking = true;
  clearTimeout(tabWalkSoon);
  tabWalkSoon = setTimeout(() => {
    if (tabWalking) document.body.dataset.tabWalk = '1';
  }, TAB_WALK_LIST_MS);
};
const tabWalkEnd = (name = 'tab_walk_done') => {
  if (!tabWalking) return;
  tabWalking = false;
  clearTimeout(tabWalkSoon);
  delete document.body.dataset.tabWalk;
  send({ t: 'shortcut', name });
};
// (A press on a row of the list ends the walk there: see `walk_tabs_to`.)
document.addEventListener(
  'mousedown',
  (e) => {
    if (tabWalking && e.target instanceof Element && e.target.closest('.tab-walk-row')) {
      tabWalking = false;
      clearTimeout(tabWalkSoon);
      delete document.body.dataset.tabWalk;
    }
  },
  true,
);
document.addEventListener(
  'keyup',
  (e) => {
    if (tabWalking && (e.key === 'Control' || (!e.ctrlKey && e.key !== 'Escape'))) tabWalkEnd();
  },
  true,
);
window.addEventListener('blur', () => tabWalkEnd());

document.addEventListener(
  'keydown',
  (e) => {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') navKeep = null;
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
    // Ctrl+Tab: the tab used before this one; Tab again while Control is
    // held goes further back in the order of use, Shift the other way.
    // (A walk left from before, whose Control was let go unseen, ends first.)
    if (e.ctrlKey && !e.metaKey && e.key === 'Tab') {
      e.preventDefault();
      if (!tabWalking) {
        send({ t: 'shortcut', name: 'tab_walk_done' });
        tabWalkStart();
      }
      send({ t: 'shortcut', name: e.shiftKey ? 'tab_walk_back' : 'tab_walk' });
      return;
    }
    // Escape while walking: the list goes, and the tab of the first Tab is in front again.
    if (tabWalking && e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      tabWalkEnd('tab_walk_cancel');
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
    // In focus mode, Escape with nothing open leaves it (and picks no line).
    // (Escape in the AI box shuts the box, as it does out of focus mode.)
    if (e.key === 'Escape' && focusOn() && !(isBlock(t) && t.dataset.menu === '1') && !document.querySelector('.popover, .modal-backdrop, .slash-menu') && !peeking() && !ctxFor && !findOpen() && !(t && t.closest && t.closest('.ai-dock'))) {
      e.preventDefault();
      send({ t: 'shortcut', name: 'focus_mode' });
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
      // The list of a choice cell (or the month of a date cell) closes, and
      // the caret goes back to its cell. (In a table the list is beside the
      // box of the table, and says its cell.)
      const list = t.closest && t.closest('.choice-picker, .date-picker');
      const hung = list && list.parentElement.closest('.hung');
      const back = hung ? hung.closest('.table-frame').querySelector(`.table-block .cell.pick[data-r="${hung.dataset.r}"][data-c="${hung.dataset.c}"]`) : list && list.parentElement.querySelector('.cell.pick');
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
    if (!(e.key === 'Enter' || e.key === 'Backspace' || e.key === 'Delete' || e.key === 'Tab' || e.key.startsWith('Arrow') || (cmd && (key === 'd' || key === 'p')))) return;
    const s = selIn(t) || { start: 0, end: 0 };

    if (cmd && e.shiftKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault();
      send({ t: 'block', action: e.key === 'ArrowUp' ? 'move_up' : 'move_down', page, id, pos: s.start });
      return;
    }
    // ⌘⇧P: "Move to…" for this line, with what is nested in it.
    if (cmd && e.shiftKey && key === 'p') {
      e.preventDefault();
      send({ t: 'block', action: 'move_to', page, id, pos: 0 });
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
    // Delete (forward) at the very end of a line joins the next line to it.
    // (Each line is a box of its own, so the key did nothing there.)
    if (e.key === 'Delete' && !code && s.start === s.end && s.end >= textLen(t) && !e.metaKey && !e.altKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      send({ t: 'delete', page, id, value: mdOf(t) });
      return;
    }
    if (e.key === 'Delete') return;
    // Math in a line: Tab at the end of a line that shows an answer keeps
    // the answer, as text of the line.
    if (e.key === 'Tab' && t.dataset.calc && !e.shiftKey && !e.metaKey && !e.altKey && !e.ctrlKey && s.start === s.end && s.end >= textLen(t)) {
      e.preventDefault();
      document.execCommand('insertText', false, (/\s$/.test(t.textContent) ? '' : ' ') + t.dataset.calc);
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
        if (e.key === 'ArrowUp') keepNav('up');
        // (Up passes over the other columns of a set, unless a narrow pane
        // shows them one under the other; Left goes on from line to line.)
        send({ t: 'nav', dir: e.key === 'ArrowUp' && !stackedColumns(t) ? 'up' : 'left', page, id });
      }
      return;
    }
    if (plain && (e.key === 'ArrowDown' || e.key === 'ArrowRight')) {
      const len = textLen(t);
      const atEnd = s.start === len && s.end === len;
      if (atEnd || (e.key === 'ArrowDown' && s.start === s.end && caretLine(t).last)) {
        e.preventDefault();
        if (e.key === 'ArrowDown') keepNav('down');
        send({ t: 'nav', dir: e.key === 'ArrowDown' && !stackedColumns(t) ? 'down' : 'right', page, id });
      }
    }
  },
  true,
);

// The copy button of a code block, the arrow that folds a heading: a press
// leaves the caret where it is.
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.target.closest && e.target.closest('.code-copy, .fold-arrow')) e.preventDefault();
  },
  true,
);

// A fold hid the line with the caret (`set_fold` in state.rs): when that
// line is gone from the page and the keyboard is in no other box, the caret
// goes to the end of the heading.
window.__rune.caretLeft = (gone, head) => {
  let tries = 0;
  const look = () => {
    if (document.getElementById(gone)) {
      if (tries++ < 60) requestAnimationFrame(look);
      return;
    }
    const a = document.activeElement;
    if (!a || a === document.body) window.__rune.focus(head, -1);
  };
  requestAnimationFrame(look);
};

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
    if ((dragPick && dragPick.on) || (marquee && marquee.on) || performance.now() - dragPickEnded < 500) {
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
// A file that is no picture (a PDF, a table as CSV or as a spreadsheet, a
// file of words) goes to the app as its bytes: the app reads its words and
// gives them to the AI with the request. A file that is too large is not
// read at all; the app says so.
const MOST_AI_FILE = 20 * 1024 * 1024;
// True while the AI box is asked about the picked blocks or rows: the
// keyboard is in the box, or nowhere while the box shows what it was given.
// (Delete and Backspace are not meant for the picks then.)
function aiHoldsPicks() {
  const dock = document.querySelector('.ai-dock.open');
  if (!dock) return false;
  const on = document.activeElement;
  if (on && dock.contains(on)) return true;
  // (Only what was picked on the page: a file in the box holds nothing.)
  return (!on || on === document.body) && !!dock.querySelector('#ai-given .ai-chip[data-given="part"], #ai-given .ai-chip[data-icon="text"]');
}
const sendAiFile = (file) => {
  if (file.size > MOST_AI_FILE) {
    send({ t: 'ai_file', name: file.name, data: '!large' });
    return;
  }
  const reader = new FileReader();
  reader.onload = () => send({ t: 'ai_file', name: file.name, data: String(reader.result || '') });
  reader.onerror = () => send({ t: 'ai_file', name: file.name, data: '' });
  reader.readAsDataURL(file);
};
const aiFilesIn = (data) => [...((data && data.files) || [])].filter((f) => !f.type.startsWith('image/')).slice(0, 8);
document.addEventListener(
  'paste',
  (e) => {
    if (!aiBoxOf(e.target)) return;
    const pictures = aiPicturesIn(e.clipboardData);
    const files = aiFilesIn(e.clipboardData);
    if (!pictures.length && !files.length) return;
    // (Not a picture for the page under the box.)
    e.preventDefault();
    e.stopPropagation();
    // (A file copied in the Finder comes with a picture of its icon: the
    // file is what is meant.)
    if (!files.length) pictures.forEach(sendAiPicture);
    files.forEach(sendAiFile);
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
    const files = aiFilesIn(e.dataTransfer);
    if (!pictures.length && !files.length) return;
    e.preventDefault();
    e.stopPropagation();
    pictures.forEach(sendAiPicture);
    files.forEach(sendAiFile);
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
    if (!isCode(t) && s && s.start !== s.end && /^(https?:\/\/|mailto:|notion2:\/\/|rune:\/\/)\S+$/i.test(trimmed)) {
      setLink(t, s.start, s.end, trimmed);
      return;
    }
    // The link of a page (Copy link) pasted by itself: the app makes it the
    // page's line (on an empty line) or a link with the page's name.
    if (!isCode(t) && s && /^(rune|notion2):\/\/page\/[A-Za-z0-9-]+\/?$/i.test(trimmed)) {
      send({ t: 'paste', page: t.dataset.page, id: t.dataset.id, pos: s.start, value: mdOf(t), text: trimmed });
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
    // One line that is just what the app keeps of the picked blocks copied
    // last (see `copy_picked`) goes to the app, which puts that line in with
    // its formats. (Typed in as words, its stars and backslashes showed.)
    const kept = !text.includes('\n') && window.__rune && window.__rune.keptCopy === text;
    if (isCode(t) || (!text.includes('\n') && !kept)) {
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
    // (Let go where the page did not see it.)
    if (!(ev.buttons & 1)) return up();
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
    // The width each side would have with the line under the pointer. (In a
    // tab of several pages the line is a gap between two cards: its middle
    // follows the pointer.)
    const half = Math.max(0, b0 - a1) / 2;
    const sides = [
      [index, pointer - half - a0],
      [index + 1, b1 - pointer - half],
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
        const width = Math.min(both - min, Math.max(min, pointer - half - a0));
        a.style.flexGrow = String((grow * width) / both);
        b.style.flexGrow = String(grow - (grow * width) / both);
        changed = true;
      }
    }
  };
  const move = (ev) => {
    // (Let go where the page did not see it.)
    if (!(ev.buttons & 1)) return up();
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
    if (!peeking() && onEdge && !peekShow && !e.buttons) peekShow = setTimeout(() => setPeek(true), 100);
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
// The lines of a page: the rows of its `.blocks`, and the rows in its sets
// of columns (src/columns.rs), in their order. (`within`: from `.blocks`.)
const pageRows = (more = '') => `.blocks > .block-row${more}, .blocks > .columns > .column > .block-row${more}`;
const rowsWithin = (more = '') => `:scope > .block-row${more}, :scope > .columns > .column > .block-row${more}`;
// The columns of the line `el` stand one under the other (a narrow pane).
const stackedColumns = (el) => {
  const set = el.closest('.columns');
  return !!set && getComputedStyle(set).flexDirection === 'column';
};
const zoomOf = (el) => {
  let z = 1;
  for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) {
    const own = parseFloat(getComputedStyle(n).zoom);
    if (own > 0 && own !== 1) z *= own;
  }
  return z;
};

// ----- math in a line: the grey answer at the end of a line ------------------
// The page draws it after the text (`.blk[data-calc]::after`), so it is no
// part of the line. A press on it copies it and leaves the caret where it is.
// It starts where the text ends, on the text's last line, and is as wide as
// its words in the line's letters.
const calcCanvas = document.createElement('canvas');
const onCalc = (el, x, y) => {
  const range = document.createRange();
  range.selectNodeContents(el);
  const rects = range.getClientRects();
  const end = rects[rects.length - 1];
  if (!end) return false;
  const cs = getComputedStyle(el);
  const ctx = calcCanvas.getContext('2d');
  ctx.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  const wide = ctx.measureText(' ' + el.dataset.calc).width * zoomOf(el);
  return x >= end.right - 1 && x <= end.right + wide + 2 && y >= end.top - 2 && y <= end.bottom + 2;
};
document.addEventListener(
  'mousedown',
  (e) => {
    const el = e.button === 0 && e.target.closest && e.target.closest('.blk[data-calc]');
    if (!el || !onCalc(el, e.clientX, e.clientY)) return;
    e.preventDefault();
    e.stopPropagation();
    send({ t: 'copy_answer', text: el.dataset.calc.replace(/^= /, '') });
  },
  true,
);

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

// A menu that hangs from a cell, a row or a heading of a table, or from a
// value at the top of a row's page. It is fixed to the window: under what
// it hangs from, moved up as far as it must to end above the foot of the
// window, and inside the window at both sides. With no room at its right,
// its second list opens at its left.
//
// A menu of a table is not drawn in the box that scrolls the table sideways
// (see `Grid` in ui/tables.rs): a frozen cell is a layer of its own there,
// and WebKit cuts what is in such a layer at the edges of the box, a fixed
// menu too. It is drawn beside the box, and its element says what it hangs
// from: the row `data-r`, the heading `data-c`, or the cell of both.
const SUB_ROOM = 244;
const hangsFrom = (menu) => {
  const hung = menu.parentElement;
  if (!hung) return null;
  const all = { left: 8, top: 8, right: innerWidth - 8, bottom: innerHeight - 8 };
  const frame = hung.classList.contains('hung') && hung.closest('.table-frame');
  const box = frame && frame.querySelector(':scope > .table-block');
  if (!box) {
    const a = hung.getBoundingClientRect();
    // (The values of a row are a layer of their own while a menu is open in
    // them, in the pane that scrolls: the menu stays inside that pane.)
    const pane = hung.closest('.row-values') && hung.closest('.scroller');
    const p = pane && pane.getBoundingClientRect();
    const edge = p ? { left: Math.max(all.left, p.left + 4), top: Math.max(all.top, p.top + 4), right: Math.min(all.right, p.right - 4), bottom: Math.min(all.bottom, p.bottom - 4) } : all;
    return { left: a.left, bottom: a.bottom, edge, el: hung };
  }
  const { hang, r, c } = hung.dataset;
  const b = box.getBoundingClientRect();
  // (The menu of a column hangs from the row of names that stays at the top
  // of the view, while that shows: see `placeSticks`.)
  const tr = ((hang === 'column' && stickOf(box)) || box).querySelector(`tr[data-r="${hang === 'column' ? 0 : r}"]`);
  const el = hang === 'row' ? tr : tr && tr.querySelector(`:scope > td[data-col="${c}"]`);
  if (!el) return { left: b.left, bottom: b.top, edge: all, el: box };
  const a = el.getBoundingClientRect();
  // The menu of a row is at the left edge of the table; that of a cell or
  // a heading under it, at the part of it that shows in the box.
  // (A table with kinds has the strip of its boxes at its left, inside its
  // box: the menu of a row is at the first cell, past that strip.)
  const strip = (parseFloat(getComputedStyle(box).paddingLeft) || 0) * (b.width / box.offsetWidth || 1);
  const left = hang === 'row' ? b.left + strip : Math.max(b.left + strip, Math.min(a.left, b.right - 40));
  return { left, bottom: a.bottom, edge: all, el };
};
const placeTableMenu = (menu) => {
  const from = hangsFrom(menu);
  if (!from) return;
  const cb = containingBlock(menu);
  menu.classList.add('anchored');
  const z = zoomOf(menu);
  const w = menu.offsetWidth * z;
  const h = menu.offsetHeight * z;
  const { edge } = from;
  const left = Math.max(edge.left, Math.min(from.left, edge.right - w));
  const top = Math.max(edge.top, Math.min(from.bottom + 4, edge.bottom - h));
  menu.style.left = (left - cb.left) / z + 'px';
  menu.style.top = (top - cb.top) / z + 'px';
  menu.style.right = 'auto';
  menu.dataset.sub = left + w + SUB_ROOM * z > innerWidth - 8 ? 'left' : 'right';
};
// A second list beside a menu of a table, and a menu of the table itself,
// end above the foot of the window too.
const placeSubMenu = (sub) => {
  sub.style.top = '';
  // (A menu that opens grows with a motion, and is not as tall as it will
  // be yet: it is measured without the motion, which then starts again.)
  const motion = sub.style.animation;
  sub.style.animation = 'none';
  const r = sub.getBoundingClientRect();
  const over = Math.min(r.bottom - (innerHeight - 8), r.top - 8);
  if (over > 0) sub.style.top = `${parseFloat(getComputedStyle(sub).top) - over / zoomOf(sub)}px`;
  sub.style.animation = motion;
};
const RAISED_MENUS = '.table-menu .column-sub, .own-menu, .kinds-menu';
// The "/" menu and the "@" list of a line: under the line when they fit there,
// else over it (at the foot of a page that cannot scroll further), always
// whole in the window. Placed again when their items change and when the
// page scrolls.
const placeSlashMenu = (menu) => {
  const row = menu.closest('.block-row') || menu.parentElement;
  if (!row || !menu.isConnected) return;
  const a = row.getBoundingClientRect();
  const cb = containingBlock(menu);
  menu.classList.add('anchored');
  const z = zoomOf(menu);
  menu.style.maxHeight = '';
  const most = parseFloat(getComputedStyle(menu).maxHeight) * z || 380 * z;
  const h = Math.min(menu.scrollHeight * z, most);
  const w = menu.offsetWidth * z;
  // The line the caret is on. (It was taken to be the first line of the
  // row: at the end of a paragraph of several lines the list opened under
  // the first line, over the line that is typed in.) With the caret in
  // another place, the first line of the row.
  const sel = getSelection();
  const caret = sel && sel.rangeCount && row.contains(sel.focusNode) ? caretRect() : null;
  const first = a.top + Math.min(a.height, 30 * z);
  const foot = caret ? Math.min(a.bottom, Math.max(first, caret.bottom)) : first;
  const head = caret && foot > first ? Math.max(a.top, Math.min(caret.top, foot - 14 * z)) : a.top;
  const below = innerHeight - foot - 10;
  const above = head - 10;
  const left = Math.max(8, Math.min(a.left + 26 * z, innerWidth - w - 8));
  menu.style.left = (left - cb.left) / z + 'px';
  menu.style.right = 'auto';
  if (h <= below || below >= above) {
    menu.style.top = (foot + 2 - cb.top) / z + 'px';
    menu.style.bottom = 'auto';
    menu.style.maxHeight = Math.min(most, below) / z + 'px';
  } else {
    // Over the line, its foot at the line: it shrinks upward as items go.
    menu.style.top = 'auto';
    menu.style.bottom = (innerHeight - head + 2 - (cb.bottom !== undefined ? innerHeight - cb.bottom : 0)) / z + 'px';
    menu.style.maxHeight = Math.min(most, above) / z + 'px';
  }
};
const slashWatch = new MutationObserver((list) => {
  const seen = new Set();
  for (const m of list) {
    const menu = m.target.closest && m.target.closest('.slash-menu');
    if (menu && !seen.has(menu)) {
      seen.add(menu);
      placeSlashMenu(menu);
    }
  }
});
document.addEventListener(
  'scroll',
  () => document.querySelectorAll('.slash-menu.anchored').forEach(placeSlashMenu),
  { capture: true, passive: true },
);

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
const TABLE_MENUS = '.table-hung .popover:not(.column-sub), .row-values .popover:not(.column-sub)';

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
  // Pinned tabs stand first. One of them moves among the pinned ones only;
  // anything else let go on a pinned tab lands after the last of them.
  const row = e.target.closest && e.target.closest('.tabs');
  const pins = row ? [...row.querySelectorAll(':scope > .tab.pinned')] : [];
  const lastPin = pins[pins.length - 1];
  const moved = pins.find((p) => p.dataset.tab === skip);
  if (moved || (tab && tab.classList.contains('pinned'))) {
    if (tab === moved) return null;
    if (moved && pins.includes(tab)) {
      const r = tab.getBoundingClientRect();
      return { el: tab, to: tab.dataset.tab, mode: e.clientX < r.left + r.width / 2 ? 'before' : 'after' };
    }
    return lastPin === moved ? null : { el: lastPin, to: lastPin.dataset.tab, mode: 'after' };
  }
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
  // (A pinned tab is not pulled into a pane.)
  if (tabDrag) return tabDrag === activeTab() || document.querySelector(`.tab.pinned[data-tab="${CSS.escape(tabDrag)}"]`) ? null : p;
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
const OWN_DRAG = 'application/x-rune-drag';
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
    } else if (!paneTarget(e) && !(e.target.closest && e.target.closest('.work-card'))) {
      // (Out into a tab of its own only when it is let go outside the page
      // area: on the line between two panes, a folded pane or the find bar,
      // nothing changes. It was thrown out of its tab from there.)
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

// At the left or the right edge of a line, the dragged line goes beside
// it: the two stand in columns (src/columns.rs). (Not over the handles at
// its left: a line dragged by its handle straight up or down goes there.)
// Beside a line of the top level, or beside a line of a set that has room
// for one more column (three at most). Gives the side, or null.
const SIDE_EDGE = 60;
const sideOf = (e, row) => {
  if (row.classList.contains('free') || row.classList.contains('side')) return null;
  const set = row.closest('.columns');
  if (set) {
    const from = [...document.querySelectorAll('.block-row[data-id]')].find((r) => r.dataset.id === dragId);
    if (set.children.length >= 3 && !(from && set.contains(from))) return null;
  } else if (parseFloat(row.style.paddingLeft) > 0) return null;
  const r = row.getBoundingClientRect();
  const edge = Math.min(SIDE_EDGE * zoomOf(row), r.width * 0.15);
  if (e.clientX >= r.left && e.clientX < r.left + edge) return 'left';
  if (e.clientX > r.right - edge) return 'right';
  return null;
};
const dropTarget = (e) => {
  const row = e.target && e.target.closest && e.target.closest('.block-row');
  if (!row || row.dataset.id === dragId) return null;
  const rect = row.getBoundingClientRect();
  return { row, after: e.clientY >= rect.top + rect.height / 2, side: sideOf(e, row) };
};

document.addEventListener('dragover', (e) => {
  if (!dragId) return;
  const target = dropTarget(e);
  if (!target) return;
  e.preventDefault();
  e.dataTransfer.dropEffect = 'move';
  // A side of a line in a set marks that side of its whole column.
  if (target.side) markRow(target.row.closest('.column') || target.row, `drop-${target.side}`);
  else markRow(target.row, target.after ? 'drop-after' : 'drop-before');
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
    send({ t: 'move', page: target.row.dataset.page, from: dragId, to: target.row.dataset.id, after: target.after, side: target.side || undefined });
  }
  endDrag();
});

document.addEventListener('dragend', endDrag);

// ----- drawings (Excalidraw in a frame of its own) ------------------------------
// The frame says when it's ready and sends its scene as you draw; the page
// keeps the latest scene per drawing, so a frame that opens again (a page
// shown again, the same page in both panes) gets it straight away.

const scenes = Object.assign({}, window.__rune_scenes || {});
delete window.__rune_scenes;
const drawingFrames = (id) =>
  [...document.querySelectorAll('.drawing[data-drawing]')].filter((d) => d.dataset.drawing === id).map((d) => d.querySelector('iframe.drawing-frame')).filter(Boolean);
const loadInto = (frame, json) => frame.contentWindow && frame.contentWindow.postMessage({ runedraw: 'load', json }, '*');

window.addEventListener('message', (e) => {
  const d = e.data;
  if (!d || typeof d !== 'object' || typeof d.runedraw !== 'string') return;
  const frame = [...document.querySelectorAll('iframe.drawing-frame')].find((f) => f.contentWindow === e.source);
  const box = frame && frame.closest('.drawing');
  if (!box) return;
  const id = box.dataset.drawing;
  if (d.runedraw === 'ready') {
    box.dataset.ready = '1';
    if (id in scenes) loadInto(frame, scenes[id]);
    else send({ t: 'drawing_ready', id }); // this window has no scene yet: ask for it
  } else if (d.runedraw === 'save' && typeof d.json === 'string') {
    scenes[id] = d.json;
    box.dataset.saves = String((Number(box.dataset.saves) || 0) + 1);
    send({ t: 'drawing_save', page: box.dataset.page, id, data: d.json });
    // The same drawing open in the other pane follows along.
    drawingFrames(id).forEach((f) => f !== frame && f.closest('.drawing').dataset.ready && loadInto(f, d.json));
  } else if (d.runedraw === 'expand') {
    setExpanded(box, !box.dataset.expanded);
  } else if (d.runedraw === 'probe') {
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
  if (frame && frame.contentWindow) frame.contentWindow.postMessage({ runedraw: 'expanded', on }, '*');
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
      // It saves now what it still holds (a new colour or label is saved a
      // moment after the change). When this press leads to another page,
      // the frame is gone by then: the save it sends as it goes comes from
      // no window the page knows, and was dropped.
      const frame = box.querySelector('iframe.drawing-frame');
      if (frame && frame.contentWindow) frame.contentWindow.postMessage({ runedraw: 'flush' }, '*');
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
  m.querySelectorAll('[data-ctx="paste"]').forEach((i) => (i.style.display = ''));
  m.querySelectorAll('[data-ctx="copy-picture"]').forEach((i) => (i.style.display = 'none'));
  m.classList.add('show');
  const mz = zoomOf(m);
  m.style.left = Math.max(8, Math.min(x, innerWidth - m.offsetWidth * mz - 8)) / mz + 'px';
  m.style.top = Math.max(8, Math.min(y, innerHeight - m.offsetHeight * mz - 8)) / mz + 'px';
};
// A right-click on a picture (in a page, in a cell of a table, a cover, a
// card of a gallery, or shown large): the short menu with "Copy picture"
// alone. The picture is one the
// app keeps: its name is the end of its address.
const keptPicture = (img) => {
  const name = ((img && (img.currentSrc || img.src)) || '').split('/').pop().split('?')[0];
  return /^[A-Za-z0-9]{1,40}\.[a-z0-9]{2,5}$/.test(name) ? name : null;
};
const openPictureCtx = (name, x, y) => {
  const m = ctxMenu();
  if (!m) return;
  ctxFor = { picture: name };
  m.querySelectorAll('[data-ctx]').forEach((i) => (i.style.display = i.dataset.ctx === 'copy-picture' ? '' : 'none'));
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
    const picture = t.closest('.lightbox img, .cell-pic > img, .image-block > img, .page-cover > img, img.gallery-pic');
    const kept = picture && keptPicture(picture);
    if (kept) {
      openPictureCtx(kept, e.clientX, e.clientY);
      return;
    }
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
  if (f.picture) {
    if (kind === 'copy-picture') send({ t: 'copy_picture', name: f.picture });
    return;
  }
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
// Text wins over a picture: a copy from a spreadsheet or a web page carries
// the words and a picture of them. But a picture file copied in the Finder
// comes with its own name as the text: there the file is what is meant
// (its name was typed into the line).
function pictureIn(data) {
  if (!data) return null;
  const files = [...(data.files || [])].filter((f) => f.type.startsWith('image/'));
  const text = data.getData('text/plain');
  if (text) {
    const names = text.split(/[\r\n]+/).map((line) => line.trim());
    return files.find((f) => f.name && names.includes(f.name)) || null;
  }
  const item = [...(data.items || [])].find((i) => i.kind === 'file' && i.type.startsWith('image/'));
  return (item && item.getAsFile()) || files[0] || null;
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

// Sends a picture file to the app; the answer is kept until it is sent,
// and says whether anything was sent.
function sendPicture(file, page, id) {
  return new Promise((done) => {
    if (!file || !page) return done();
    if (file.size > PICTURE_MOST) {
      send({ t: 'image_data', page, id, data: '' });
      return done(true);
    }
    const reader = new FileReader();
    reader.onload = () => {
      send({ t: 'image_data', page, id, data: String(reader.result).split(',')[1] || '' });
      done(true);
    };
    reader.onerror = () => done();
    reader.readAsDataURL(file);
  });
}
// Picture files dropped on a page go in where they are let go: at the line
// under the pointer, or at the line of that page nearest by height (the
// margin beside a line, the title and the cover are no line: a picture let
// go there went to the end of the page, often out of view).
const holdsFiles = (e) => e.dataTransfer && [...e.dataTransfer.types].includes('Files');
const rowAtHeight = (pane, y) => {
  let best = null;
  let gap = Infinity;
  for (const row of pane.querySelectorAll(pageRows('[data-id]'))) {
    const box = row.getBoundingClientRect();
    if (!box.height) continue;
    const away = y < box.top ? box.top - y : y > box.bottom ? y - box.bottom : 0;
    if (away < gap) {
      gap = away;
      best = row;
    }
  }
  return best;
};
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
    // the row at that height (above the first line: the first line).
    const row = under && under.closest(pageRows('[data-id]'));
    const pane = under && under.closest('.pane');
    const at = row || (pane && rowAtHeight(pane, e.clientY));
    if (!at) return;
    page = at.dataset.page;
    id = at.dataset.id;
  }
  // One after another, so they keep their order: the first that is sent
  // names the row, each of the others goes after the picture put just
  // before it ("last"; the app keeps the row when the first cannot be kept).
  let first = true;
  files.reduce(
    (before, file) =>
      before
        .then(() => sendPicture(file, page, first ? id : 'last'))
        .then((sent) => {
          if (sent) first = false;
          return new Promise((done) => setTimeout(done, 60));
        }),
    Promise.resolve(),
  );
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
// A letter typed on a choice cell opens its list with that letter in the box
// (on a date cell, its month: the letter starts the day that is typed).
const seedPicker = (letter) => {
  let tries = 0;
  const tick = () => {
    const box = document.querySelector('.choice-picker .pick-input, .date-picker .pick-input');
    if (box) {
      box.value = letter;
      box.dispatchEvent(new Event('input', { bubbles: true }));
    } else if (tries++ < 30) requestAnimationFrame(tick);
  };
  tick();
};
window.__rune.focusCell = (id, r, c) => {
  const box = document.querySelector(`.table-block[data-table="${id}"]`);
  if (!box) return;
  const tr = box.querySelector(`tr[data-r="${r}"]`);
  // (When the row is gone, with a filter that it no longer passes: no cell.
  // The first cell of the table is a heading, and the next keys would give
  // the column another name.)
  intoCell(cellAt(box, r, c) || (tr && typedCells(tr)[0]) || (Number(r) === 0 ? box.querySelector('.cell') : null));
};
// Where the columns that stay in place are, once the page shows them.
window.__rune.freezeColumns = (id) =>
  requestAnimationFrame(() => {
    const box = document.querySelector(`.table-block[data-table="${id}"]`);
    if (!box) return;
    const heads = box.querySelectorAll('tr.head-row > td');
    let left = 0;
    for (let i = 0; i < (Number(box.dataset.frozen) || 0) && i < heads.length; i += 1) {
      box.style.setProperty(`--f${i}`, `${left}px`);
      left += heads[i].getBoundingClientRect().width / zoomOf(heads[i]);
    }
    // (Drawn again while scrolled sideways, with other columns frozen,
    // hidden or wider: what is under the frozen cells is another part now.)
    cutTable(box);
    // (And the row of names at the top of the view is drawn again with it.)
    const stuck = stickOf(box);
    if (stuck) copyHead(box, stuck);
    const scroller = box.closest('.scroller');
    if (scroller) placeSticks(scroller);
  });
window.addEventListener('resize', () => {
  for (const box of document.querySelectorAll('.table-block[data-frozen]:not([data-frozen="0"])')) window.__rune.freezeColumns(box.dataset.table);
  for (const box of document.querySelectorAll('.table-block')) {
    if (box.scrollLeft > 0) queueCut(box);
    else syncBar(box);
  }
  for (const scroller of document.querySelectorAll('.scroller')) placeSticks(scroller);
});
// A table scrolled sideways. Its frozen cells and the strip of the boxes
// that pick rows are see-through (the page shows under them, as in Notion,
// and the page is glass: no colour would match it), so what scrolls is cut
// where they begin: a column that is all under them does not show, and the
// one column that is partly under them is cut at their right edge (with no
// frozen column: at the left of the table, so nothing shows in the strip).
// The cuts are rules of one style sheet, by table and by place of the
// column, so cells that the app draws again keep them; they hold only while
// the box says it is scrolled (data-slid), so a table that is drawn new, at
// its start, has none. How far the one column is cut is a value on the box
// (--cut): a scroll changes that value, and the rules only when a column
// goes under the edge or comes out.
const tableCuts = new Map(); // table id -> its rules
let cutSheet = null;
// The bar under a wide table (`.table-bar`, drawn by the app beside the box
// that scrolls): as long as the part of the table that shows, at the place
// the table is scrolled to. With no room to scroll there is no bar.
const barOf = (box) => (box.parentElement ? box.parentElement.querySelector(':scope > .table-bar') : null);
const barSizes = new ResizeObserver((entries) => {
  for (const entry of entries) syncBar(entry.target);
});
const barWatched = new WeakSet();
const syncBar = (box) => {
  const bar = barOf(box);
  if (!bar) return;
  if (!box.hasAttribute('data-own-bar')) box.setAttribute('data-own-bar', '');
  if (!barWatched.has(box)) {
    barWatched.add(box);
    barSizes.observe(box);
  }
  const shown = box.clientWidth;
  const all = box.scrollWidth;
  const on = all > shown + 1;
  if (bar.hasAttribute('data-on') !== on) bar.toggleAttribute('data-on', on);
  if (!on) return;
  const thumb = bar.firstElementChild;
  const track = bar.clientWidth || shown;
  const wide = Math.max(28, Math.round((track * shown) / all));
  const at = ((track - wide) * box.scrollLeft) / (all - shown);
  const width = `${wide}px`;
  const place = `translateX(${at.toFixed(1)}px)`;
  if (thumb.style.width !== width) thumb.style.width = width;
  if (thumb.style.transform !== place) thumb.style.transform = place;
};
// A press on the bar moves the table from here, with its cuts, in one step
// (as a swipe does: see `wheelTable`).
let barDrag = null;
const moveBar = (x) => {
  const d = barDrag;
  const track = d.bar.getBoundingClientRect();
  const room = track.width - d.thumb.getBoundingClientRect().width;
  if (room <= 0) return;
  const share = Math.max(0, Math.min(1, (x - d.grab - track.left) / room));
  const to = share * (d.box.scrollWidth - d.box.clientWidth);
  if (Math.abs(to - d.box.scrollLeft) < 0.5) return;
  d.box.scrollLeft = to;
  cutTable(d.box);
};
const endBarDrag = () => {
  if (!barDrag) return;
  barDrag.bar.removeAttribute('data-held');
  barDrag = null;
};
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.button !== 0) return;
    const bar = e.target.closest && e.target.closest('.table-bar');
    const box = bar && bar.parentElement.querySelector(':scope > .table-block');
    if (!box) return;
    e.preventDefault();
    e.stopPropagation();
    const thumb = bar.firstElementChild;
    const t = thumb.getBoundingClientRect();
    // On the thumb: it is held where it was pressed. Beside it: its middle
    // comes to the pointer.
    const grab = e.clientX >= t.left && e.clientX <= t.right ? e.clientX - t.left : t.width / 2;
    barDrag = { box, bar, thumb, grab };
    bar.setAttribute('data-held', '');
    moveBar(e.clientX);
  },
  true,
);
document.addEventListener(
  'mousemove',
  (e) => {
    if (!barDrag) return;
    // (Let go outside the window: no button is down any more.)
    if (!(e.buttons & 1) || !barDrag.box.isConnected) return endBarDrag();
    e.preventDefault();
    moveBar(e.clientX);
  },
  true,
);
document.addEventListener('mouseup', endBarDrag, true);
window.addEventListener('blur', endBarDrag);
const cutTable = (box) => {
  syncBar(box);
  const id = box.dataset.table;
  if (!id) return;
  const heads = box.isConnected ? box.querySelectorAll(':scope > table > tbody > tr.head-row > td') : [];
  let rules = '';
  if (heads.length && box.scrollLeft > 0) {
    const z = zoomOf(heads[0]);
    const cs = getComputedStyle(box);
    let edge = box.getBoundingClientRect().left + (box.clientLeft + (parseFloat(cs.paddingLeft) || 0)) * zoomOf(box) * (parseFloat(cs.zoom) || 1);
    for (const td of heads) if (td.classList.contains('frozen')) edge = Math.max(edge, td.getBoundingClientRect().right);
    const cells = (i) => `.table-block:is([data-table="${id}"], [data-of="${id}"])[data-slid] > table > * > tr:not(.group-row) > td:nth-child(${i + 1}):not(.frozen)`;
    const under = [];
    heads.forEach((td, i) => {
      if (td.classList.contains('frozen')) return;
      const r = td.getBoundingClientRect();
      if (r.right <= edge + 0.25) under.push(cells(i));
      else if (r.left < edge - 0.25) {
        // (Above, below and at the right, what hangs out of a cell still shows.)
        rules += `${cells(i)}{clip-path:inset(-40px -100vw -40px var(--cut))}\n`;
        const cut = `${((edge - r.left) / z).toFixed(1)}px`;
        if (box.style.getPropertyValue('--cut') !== cut) box.style.setProperty('--cut', cut);
      }
    });
    if (under.length) rules += `${under.join(',\n')}{clip-path:inset(50%)}\n`;
    if (!box.hasAttribute('data-slid')) box.setAttribute('data-slid', '');
  } else if (box.hasAttribute('data-slid')) box.removeAttribute('data-slid');
  // (The row of names that stays at the top of the view moves with the table.)
  const stuck = stickOf(box);
  if (stuck) {
    if (stuck.scrollLeft !== box.scrollLeft) stuck.scrollLeft = box.scrollLeft;
    stuck.toggleAttribute('data-slid', box.hasAttribute('data-slid'));
    const cut = box.style.getPropertyValue('--cut');
    if (stuck.style.getPropertyValue('--cut') !== cut) stuck.style.setProperty('--cut', cut);
  }
  if ((tableCuts.get(id) || '') === rules) return;
  if (rules) tableCuts.set(id, rules);
  else tableCuts.delete(id);
  // Tables that are gone lose their cuts.
  for (const other of tableCuts.keys()) if (other !== id && !document.querySelector(`.table-block[data-table="${other}"]`)) tableCuts.delete(other);
  if (!cutSheet) {
    cutSheet = document.createElement('style');
    cutSheet.id = 'table-cuts';
    document.head.appendChild(cutSheet);
  }
  cutSheet.textContent = [...tableCuts.values()].join('');
};
const queueCut = (box) => requestAnimationFrame(() => cutTable(box));
document.addEventListener(
  'scroll',
  (e) => {
    const box = e.target;
    if (!(box instanceof Element) || !box.classList.contains('table-block')) return;
    cutTable(box);
    wheelOwn(box);
  },
  true,
);
// A swipe sideways on a table is moved here, in step with the cuts. (Left
// to WebKit, the table moves at once, apart from the page script, and the
// cuts come a moment later: in a fast swipe the words of the cells showed
// under the frozen column for an instant, again and again.) A swipe up or
// down, and one at the end of the table, stay with WebKit.
const wheelTable = (e) => {
  // (Also on the copy of its row of names at the top of the view: see `placeSticks`.)
  const box = stuckOf.get(e.currentTarget) || e.currentTarget;
  // (Once the engine has a swipe, its steps cannot be stopped: moving the
  // table here too would move it twice.)
  if (e.ctrlKey || e.defaultPrevented || !e.cancelable) return;
  const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? box.clientWidth : 1;
  const dx = e.deltaX * unit;
  if (!dx || Math.abs(dx) <= Math.abs(e.deltaY * unit)) return;
  const z = zoomOf(box) * (parseFloat(getComputedStyle(box).zoom) || 1);
  const to = Math.max(0, Math.min(box.scrollWidth - box.clientWidth, box.scrollLeft + dx / z));
  if (Math.abs(to - box.scrollLeft) < 0.5) return;
  e.preventDefault();
  box.scrollLeft = to;
  cutTable(box);
};
const wheelOwned = new WeakSet();
const wheelOwn = (box) => {
  if (wheelOwned.has(box)) return;
  wheelOwned.add(box);
  box.addEventListener('wheel', wheelTable, { passive: false });
};
// (Each table gets this when the pointer first comes over it: a listener
// on the whole page would make every scroll of the page wait for the script.)
document.addEventListener(
  'mouseover',
  (e) => {
    const box = e.target.closest && e.target.closest('.table-block');
    if (!box || stuckOf.has(box)) return;
    wheelOwn(box);
    // (A table put in another place is at its start again, with no word
    // of that from the engine.)
    if (box.hasAttribute('data-slid') !== box.scrollLeft > 0) cutTable(box);
  },
  true,
);
// The row of the names of a table taller than the page's view stays at the
// top of the view while the rows scroll under it, and goes when the table's
// end passes. (WebKit holds a sticky thing to the nearest box that scrolls,
// and for the row of names that is the box that scrolls the table sideways.)
// What stays is a copy of the row, in a rail over the table beside that box,
// held at the top by the engine (`position: sticky`): in a fast scroll a row
// moved by the script was behind the page in most frames, and the copy is
// steady in all. The script shows it only while the table's own row is above
// the top of the view (so the first row is never under it), keeps it in step
// sideways with its cuts (`cutTable`), and hands its presses to the row of the
// table: a press on a name opens the menu of its column (which hangs from the
// copy, see `hangsFrom`), the box of the names picks all rows, ⌘ or ⇧ and a
// press picks the column. It is drawn on the page's own colour, so the rows
// under it do not show through.
const sticks = new Map(); // the box of a table -> the rail of its copy
const stuckOf = new WeakMap(); // a copy -> the box of its table
const stickOf = (box) => {
  const rail = sticks.get(box);
  return rail && rail.isConnected ? rail.firstElementChild : null;
};
const headOf = (box) => {
  const table = box.firstElementChild;
  const head = table && table.tBodies && table.tBodies[0] && table.tBodies[0].rows[0];
  return head && head.classList.contains('head-row') ? head : null;
};
const copyHead = (box, copy) => {
  const head = headOf(box);
  if (!head) return;
  if (copy.className !== box.className) copy.className = box.className;
  copy.style.cssText = box.style.cssText;
  copy.toggleAttribute('data-slid', box.hasAttribute('data-slid'));
  const table = document.createElement('table');
  table.style.cssText = box.firstElementChild.style.cssText;
  const row = head.cloneNode(true);
  const names = head.querySelectorAll('textarea');
  row.querySelectorAll('textarea').forEach((t, i) => {
    t.value = names[i] ? names[i].value : '';
    t.readOnly = true;
  });
  // (No second place to type in, and no second of what the app and the page
  // script find by its marks.)
  row.removeAttribute('data-dioxus-id');
  for (const el of row.querySelectorAll('[data-dioxus-id], [data-r], [data-c], [data-check], [id]')) {
    for (const name of ['data-dioxus-id', 'data-r', 'data-c', 'data-check', 'id']) el.removeAttribute(name);
  }
  for (const el of row.querySelectorAll('textarea, button')) el.tabIndex = -1;
  table.appendChild(document.createElement('tbody')).appendChild(row);
  copy.replaceChildren(table);
  copy.scrollLeft = box.scrollLeft;
};
const dropStick = (box) => {
  const rail = sticks.get(box);
  if (!rail) return;
  rail.remove();
  sticks.delete(box);
};
// (The page's height changes with no scroll and no new drawing of a table: a
// picture that loads above, a toggle opened by sync. The row is placed again.)
const pageSizes = new ResizeObserver((entries) => {
  for (const entry of entries) {
    const scroller = entry.target.parentElement;
    if (scroller && scroller.isConnected) placeSticks(scroller);
  }
});
const sizedPage = new WeakMap(); // a scroller -> its page that is watched
const placeSticks = (scroller) => {
  const page = scroller.querySelector(':scope > .page');
  if (page && sizedPage.get(scroller) !== page) {
    if (sizedPage.get(scroller)) pageSizes.unobserve(sizedPage.get(scroller));
    sizedPage.set(scroller, page);
    pageSizes.observe(page);
  }
  for (const [box, rail] of sticks) if (!box.isConnected || rail.parentElement !== box.parentElement) dropStick(box);
  const s = scroller.getBoundingClientRect();
  let held = 0;
  for (const box of [...scroller.getElementsByClassName('table-block')]) {
    if (!box.dataset.table) continue;
    const b = box.getBoundingClientRect();
    const head = b.top < s.top && b.bottom > s.top && b.height > s.height ? headOf(box) : null;
    const end = head ? head.parentElement.getBoundingClientRect().bottom : 0;
    if (!head || end <= s.top) {
      dropStick(box);
      continue;
    }
    let rail = sticks.get(box);
    if (!rail) {
      rail = document.createElement('div');
      rail.className = 'table-stick';
      rail.setAttribute('aria-hidden', 'true');
      const copy = rail.appendChild(document.createElement('div'));
      copy.dataset.of = box.dataset.table;
      stuckOf.set(copy, box);
      copy.addEventListener('wheel', wheelTable, { passive: false });
      copyHead(box, copy);
      box.parentElement.appendChild(rail);
      sticks.set(box, rail);
    }
    // (Over the head and the rows of the table, not its totals: the copy
    // leaves with the last row.)
    const z = b.height / box.offsetHeight || 1;
    const top = `${box.offsetTop}px`;
    const high = `${((end - b.top) / z).toFixed(1)}px`;
    if (rail.style.top !== top) rail.style.top = top;
    if (rail.style.height !== high) rail.style.height = high;
    held = Math.max(held, head.getBoundingClientRect().height);
  }
  // (A cell that the keyboard goes to is brought into view under the copy,
  // not behind it.)
  const pad = held ? `${(held / (s.height / scroller.offsetHeight || 1)).toFixed(1)}px` : '';
  if (scroller.style.scrollPaddingTop !== pad) scroller.style.scrollPaddingTop = pad;
};
document.addEventListener(
  'scroll',
  (e) => {
    const scroller = e.target;
    if (scroller instanceof Element && scroller.classList.contains('scroller')) placeSticks(scroller);
  },
  { capture: true, passive: true },
);
// (Before every other listener: a press on the copy is for the row it copies.)
// A press on a name is a press on the real name: the page scrolls so that
// the table's own row is where the copy is (at the top of the view), and the
// real name gets the caret, or the words, that WebKit gave the copy's name
// for that press (a second press of a double press then lands on the real
// one). A right press does the same and opens the menu of a text box there.
// The chevron opens the menu of its column; ⌘ or ⇧ picks the column; the box
// picks all rows. Only a press that began on the copy is the copy's: a
// release over it of a press made elsewhere (a drag) goes on as usual, and
// so do the side buttons of a mouse.
let stickPress = null; // what of the copy the button went down on
const realHead = (copy, el) => {
  const box = stuckOf.get(copy);
  const td = el.closest('td[data-col]');
  return box && td ? box.querySelector(`:scope > table > tbody > tr.head-row > td[data-col="${td.dataset.col}"]`) : null;
};
// The table's own row of names to where the copy is, and the copy away.
const toHead = (copy) => {
  const box = stuckOf.get(copy);
  const s = box && box.closest('.scroller');
  if (!s) return;
  const sr = s.getBoundingClientRect();
  s.scrollTop += (box.getBoundingClientRect().top - sr.top) / (sr.height / s.offsetHeight || 1);
  placeSticks(s);
};
const toRealName = (copy, name) => {
  const real = realHead(copy, name);
  const into = real && real.querySelector('textarea');
  if (!into) return null;
  const [start, end] = [name.selectionStart, name.selectionEnd];
  toHead(copy);
  into.focus({ preventScroll: true });
  into.setSelectionRange(start, end);
  return into;
};
for (const type of ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'click', 'dblclick', 'auxclick', 'contextmenu', 'focusin', 'focusout', 'focus', 'blur']) {
  window.addEventListener(
    type,
    (e) => {
      const t = e.target instanceof Element ? e.target : null;
      const copy = t && t.closest('.table-stick > .table-block');
      // (Its name has the caret for a moment in a press: no one else is told.)
      if (type.startsWith('focus') || type === 'blur') {
        if (copy) e.stopImmediatePropagation();
        return;
      }
      if (e.button === 3 || e.button === 4) return;
      // (A right press is always the copy's when it is on it.)
      if (type === 'pointerdown' || (type === 'contextmenu' && copy)) stickPress = copy ? t : null;
      const from = stickPress;
      if (type === 'mouseup' && from && e.button === 0) {
        const name = from.closest('textarea');
        const of = from.closest('.table-stick > .table-block');
        if (name && of && !e.metaKey && !e.shiftKey && !e.ctrlKey) {
          stickPress = null;
          toRealName(of, name);
        }
      }
      if (!copy || !from) return;
      e.stopImmediatePropagation();
      const name = t.closest('textarea');
      const plain = e.button === 0 && !e.metaKey && !e.shiftKey && !e.ctrlKey && name;
      // (WebKit puts the caret in the copy's name: that place goes to the real one.)
      if (!(type === 'mousedown' && plain) && !type.startsWith('pointer')) e.preventDefault();
      if (type === 'contextmenu') {
        const into = name ? toRealName(copy, name) : null;
        // (After the scroll that brought the row there: a scroll shuts that menu.)
        if (into) requestAnimationFrame(() => into.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: e.clientX, clientY: e.clientY })));
        return;
      }
      if (type !== 'click' || e.button !== 0 || plain) return;
      const real = realHead(copy, t);
      if (!real) return;
      if (t.closest('.head-check')) return real.querySelector('[data-check]')?.click();
      if (e.metaKey || e.shiftKey) {
        const at = real.querySelector('textarea') || real;
        at.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, metaKey: e.metaKey, shiftKey: e.shiftKey, clientX: e.clientX, clientY: e.clientY }));
        return;
      }
      if (t.closest('.th-menu')) real.querySelector('.th-menu')?.click();
      else toHead(copy);
    },
    true,
  );
}
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
// ⌘ and a click on an address in a table (a link, an email, a phone number)
// opens it, as its small button does; a plain click puts the caret in it.
document.addEventListener(
  'click',
  (e) => {
    if (!e.metaKey || e.button !== 0 || !e.target.closest) return;
    const cell = e.target.closest('.cell-link');
    const go = cell && cell.querySelector('[data-go]');
    if (!go || e.target.closest('[data-go]')) return;
    e.preventDefault();
    e.stopPropagation();
    go.click();
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
  // (A choice or a date cell is a button: with the keyboard in it, the
  // table is still the one in use, and its rows stay where they are.)
  const cell = e.target.closest && e.target.closest('textarea.cell, button.cell.pick');
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
// The handle goes to the row at the pointer: right at the left edge of the
// table, on the first line of the row (the middle of a row of one line),
// whatever the height of the row. The pointer is "at" a row when it is over
// one of its cells, or beside it in the strip at the left of the table,
// where the handle shows: the handle is there before the pointer is on it,
// and stays while the pointer goes from the row to it. The row of the
// headings has a handle too, with the plus alone: a new first row. (In a
// table with no line of its name above it, the handle of the table as a
// line of the page is at that height: there it shows, not a second one.)
// The row at the handle shows its box to pick it (in a table with kinds).
const ROW_LINE = 33;
const ROW_STRIP = 106;
let nearHandle = null;
let nearLine = null;
let handRow = null;
const leaveHandle = () => {
  if (nearHandle) nearHandle.classList.remove('near');
  if (nearLine) nearLine.classList.remove('at-strip');
  if (handRow) handRow.classList.remove('hand');
  nearHandle = null;
  nearLine = null;
  handRow = null;
};
const placeRowHandle = (tr) => {
  const frame = tr.closest('.table-frame');
  const handle = frame && frame.querySelector(':scope > .row-handle');
  if (!handle || !frame.offsetHeight) return;
  const f = frame.getBoundingClientRect();
  const t = tr.getBoundingClientRect();
  // The window measures in points of the screen; `top` is in the points of
  // the page, which a zoom of the page draws bigger or smaller.
  const z = f.height / frame.offsetHeight || 1;
  const line = Math.min(t.height / z, ROW_LINE);
  const head = tr.dataset.r === '0';
  if (head) {
    const inner = frame.closest('.block-inner');
    const own = inner && inner.querySelector(':scope > .gutter');
    const g = own && own.getBoundingClientRect();
    if (g && g.height && g.bottom > t.top + 2 && g.top < t.top + line * z - 2) {
      const row = inner.closest('.block-row');
      if (nearLine === row && !nearHandle) return;
      leaveHandle();
      nearLine = row;
      row.classList.add('at-strip');
      return;
    }
  }
  if (nearHandle !== handle || nearLine || handRow !== tr) leaveHandle();
  nearHandle = handle;
  handRow = tr;
  tr.classList.add('hand');
  const top = `${Math.round(((t.top - f.top) / z + (line - (handle.offsetHeight || 24)) / 2) * 2) / 2}px`;
  if (handle.dataset.r === tr.dataset.r && handle.style.top === top && handle.classList.contains('near')) return;
  handle.dataset.r = tr.dataset.r;
  handle.style.top = top;
  handle.classList.toggle('for-head', head);
  handle.classList.add('on', 'near');
};
// The row at a place in the window: the row under it, or the row beside it
// when it is in the strip at the left of a table.
const rowAt = (x, y, el) => {
  if (!el || !el.closest) return null;
  const own = el.closest('.table-block, .popover, .backdrop, .picked-bar, .table-side');
  if (own && !own.classList.contains('table-block')) return null;
  // (In the strip of the boxes that pick rows, at the left inside the box
  // of a table with kinds, the row is found by its height, as beside it.)
  const tr = own && el.closest('tr[data-r]');
  if (tr || (own && !own.classList.contains('typed'))) return tr;
  const pane = el.closest('.pane') || document;
  for (const frame of pane.querySelectorAll('.table-frame')) {
    const box = frame.querySelector(':scope > .table-block');
    if (!box || !frame.offsetHeight) continue;
    const f = frame.getBoundingClientRect();
    const z = f.height / frame.offsetHeight || 1;
    if (x >= f.left || x < f.left - ROW_STRIP * z || y < f.top || y >= f.bottom) continue;
    for (const tr of box.querySelectorAll('tr[data-r]')) {
      const t = tr.getBoundingClientRect();
      if (y >= t.top && y < t.bottom) return tr;
    }
  }
  return null;
};
let tablePointer = null;
const handleToPointer = (x, y, el) => {
  // (On the handle itself, or on that of the table as a line: it stays as it is.)
  if (el && el.closest && (el.closest('.row-handle') || (nearLine && el.closest('.gutter') && nearLine.contains(el)))) return;
  const tr = rowAt(x, y, el);
  if (tr) placeRowHandle(tr);
  else leaveHandle();
};
document.addEventListener(
  'mousemove',
  (e) => {
    tablePointer = { x: e.clientX, y: e.clientY };
    handleToPointer(e.clientX, e.clientY, e.target);
  },
  { passive: true },
);
document.documentElement.addEventListener('mouseleave', leaveHandle);
// The page or the table scrolled under a pointer that did not move: the
// handle goes to the row that is at the pointer now.
let handleQueued = false;
document.addEventListener(
  'scroll',
  () => {
    if (handleQueued || !tablePointer) return;
    handleQueued = true;
    requestAnimationFrame(() => {
      handleQueued = false;
      if (tablePointer) handleToPointer(tablePointer.x, tablePointer.y, document.elementFromPoint(tablePointer.x, tablePointer.y));
    });
  },
  { capture: true, passive: true },
);
const tableOfHandle = (handle) => handle.parentElement.querySelector(':scope > .table-block');
// The table with picked rows or columns. Rows are picked by their handle:
// a click picks one row and opens its menu, ⌘ and a click picks one more
// (or lets it go), ⇧ and a click picks all up to it. Columns are picked by
// ⌘ or ⇧ and a click on their headings. They stay picked when a menu
// closes; a press anywhere else lets them go, and so does Escape. The
// Delete key takes them out.
let pickedTable = null;
const unpick = () => {
  if (pickedTable) tableOp(pickedTable, 'unpick', 0, 0);
  pickedTable = null;
};
// The caret leaves the page when rows or columns are picked: the Delete
// key is theirs then, not a cell's.
const caretOut = () => {
  const a = document.activeElement;
  if (a && a !== document.body && a.blur && !a.closest('.popover')) a.blur();
};
// The heading of a column, when the press picks the column: ⌘ or ⇧ is
// down. (⇧ in the heading that has the caret selects its words.) A locked
// table keeps its columns.
const pickedHead = (e) => {
  if (e.button !== 0 || e.ctrlKey || e.altKey || !(e.metaKey || e.shiftKey) || !e.target.closest) return null;
  const td = e.target.closest('.table-block:not(.locked) tr.head-row > td[data-col]');
  if (!td || (!e.metaKey && e.target === document.activeElement)) return null;
  return td;
};
document.addEventListener(
  'mousedown',
  (e) => {
    // (A box that picks rows takes no caret, and ⇧ on it selects no words.)
    if (e.target.closest && e.target.closest('.table-block [data-check]')) return e.preventDefault();
    // (No caret in the heading, and no words selected by the ⇧.)
    if (pickedHead(e)) return e.preventDefault();
    // (Nor does ⇧ on a handle select the words of the page up to it.)
    if ((e.shiftKey || e.metaKey) && e.target.closest && e.target.closest('.row-handle [data-row-op="row-menu"]')) e.preventDefault();
    if (pickedTable && !(e.target.closest && e.target.closest('.row-handle, .popover, .backdrop, .picked-bar, td.col-picked .th-menu, .ai-dock'))) unpick();
  },
  true,
);
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !document.querySelector('.popover')) unpick();
  // ⌘D: a copy of each picked row (from the menu of the rows too).
  if (e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey && e.key.toLowerCase() === 'd' && pickedTable && pickedTable.isConnected && pickedTable.querySelector('tr.picked')) {
    const t = e.target.closest ? e.target : null;
    if (t && t.closest('input, textarea, select, [contenteditable]') && !t.closest('.row-menu')) return;
    e.preventDefault();
    e.stopPropagation();
    tableOp(pickedTable, 'duplicate-picked', 0, 0);
    return;
  }
  if ((e.key !== 'Backspace' && e.key !== 'Delete') || !pickedTable || e.metaKey || e.ctrlKey || e.altKey || e.isComposing) return;
  // (Not while the AI is asked about the picked rows: its box shows them.)
  if (aiHoldsPicks()) return;
  // (A key typed in a box is for the box.)
  if (e.target.closest && e.target.closest('input, textarea, select, [contenteditable]')) return;
  if (!pickedTable.isConnected || !pickedTable.querySelector('tr.picked, td.col-picked')) return;
  e.preventDefault();
  tableOp(pickedTable, 'delete-picked', 0, 0);
});
document.addEventListener(
  'click',
  (e) => {
    // The box at the left of a row picks it, or lets it go (⇧: all rows
    // from the one picked last); the box of the headings, all or none.
    const check = e.target.closest && e.target.closest('.table-block [data-check]');
    if (check) {
      e.preventDefault();
      e.stopPropagation();
      const box = check.closest('.table-block');
      caretOut();
      pickedTable = box;
      if (check.dataset.check === 'all') return tableOp(box, 'pick-all', 0, 0);
      return tableOp(box, 'pick-row', Number(check.dataset.check) || 0, e.shiftKey ? 1 : 2);
    }
    const head = pickedHead(e);
    if (head) {
      e.preventDefault();
      e.stopPropagation();
      caretOut();
      pickedTable = head.closest('.table-block');
      return tableOp(pickedTable, 'pick-column', e.shiftKey ? 1 : 2, Number(head.dataset.col) || 0);
    }
    const b = e.target.closest && e.target.closest('.row-handle [data-row-op]');
    if (!b) return;
    // (Before the app sees the click: its own click closes an open menu.)
    e.stopPropagation();
    const handle = b.closest('.row-handle');
    const row = Number(handle.dataset.r) || 0;
    const box = tableOfHandle(handle);
    const op = b.dataset.rowOp;
    // (The row of the headings has the plus alone: a new first row.)
    if (op !== 'row-menu') return tableOp(box, op, row, 0);
    if (!row) return;
    caretOut();
    pickedTable = box;
    if (e.shiftKey || e.metaKey) tableOp(box, 'pick-row', row, e.shiftKey ? 1 : 2);
    else tableOp(box, 'row-menu', row, 0);
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
  // (Let go where the page did not see it: the drag ends where it was.)
  if (!(e.buttons & 1)) {
    send({ t: 'cover_y', page: coverDrag.cover.dataset.cover, y: coverDrag.now });
    coverDrag = null;
    return;
  }
  // Dragging the picture down shows more of its top. (The pointer is in
  // points of the window, the height in the page's own: on a zoomed page
  // the picture moved faster or slower than the pointer.)
  const moved = ((e.clientY - coverDrag.startY) / zoomOf(coverDrag.cover) / coverDrag.cover.clientHeight) * 60;
  coverDrag.now = Math.min(100, Math.max(0, coverDrag.y - moved));
  coverDrag.img.style.objectPosition = `50% ${coverDrag.now}%`;
});
document.addEventListener('mouseup', () => {
  if (!coverDrag) return;
  send({ t: 'cover_y', page: coverDrag.cover.dataset.cover, y: coverDrag.now });
  coverDrag = null;
});

// ----- "@" in a text box: the pages with that name ------------------------------------
// In a cell of a table, the notes of an event and the Ask box (each says so
// with `data-at`), "@" and some letters list the pages with that name, as in
// a line of a page. The page that is chosen is written as a link to it
// (`data-at="link"`: the app shows such a link by the name of its page), or
// goes with the question (`data-at="give"`). The app finds the pages (see
// `find_pages_for_at` in state.rs); the list is drawn here, under the box.
let atList = null; // { field, box, rows, on, start, n }
let atAsked = 0;
const atField = (el) => (el && el.matches && el.matches('textarea[data-at], input[data-at]') && el.dataset.at ? el : null);
// The "@" before the caret and what was typed after it, when that can be the
// start of a page's name (not an email address, not a long sentence).
const atTyped = (field) => {
  const end = field.selectionStart;
  if (end == null || end !== field.selectionEnd) return null;
  const before = field.value.slice(0, end);
  const start = before.lastIndexOf('@');
  if (start < 0 || (start > 0 && !/[\s(]/.test(before[start - 1]))) return null;
  const q = before.slice(start + 1);
  if (q.length > 40 || /^\s/.test(q) || q.includes('\n') || q.trim().split(/\s+/).length > 4) return null;
  return { start, q };
};
const closeAt = () => {
  if (!atList) return;
  atList.box.remove();
  atList = null;
};
const placeAt = () => {
  const a = atList;
  if (!a) return;
  const r = a.field.getBoundingClientRect();
  const h = a.box.offsetHeight;
  const w = a.box.offsetWidth;
  // Under the box; above it when there is no room under it.
  const top = r.bottom + 4 + h <= innerHeight - 6 ? r.bottom + 4 : Math.max(6, r.top - 4 - h);
  a.box.style.top = `${Math.round(top)}px`;
  a.box.style.left = `${Math.round(Math.max(6, Math.min(r.left, innerWidth - w - 6)))}px`;
};
const drawAt = () => {
  const a = atList;
  if (!a) return;
  a.box.textContent = '';
  a.box.hidden = a.rows.length === 0;
  a.rows.forEach((row, i) => {
    const line = document.createElement('div');
    line.className = i === a.on ? 'at-row on' : 'at-row';
    line.setAttribute('role', 'option');
    line.dataset.at = String(i);
    if (row.sign) {
      const sign = document.createElement('span');
      sign.className = 'at-sign';
      sign.textContent = row.sign;
      line.appendChild(sign);
    }
    const name = document.createElement('span');
    name.className = 'at-name';
    name.textContent = row.title;
    line.appendChild(name);
    if (row.inside) {
      const inside = document.createElement('span');
      inside.className = 'at-inside';
      inside.textContent = row.inside;
      line.appendChild(inside);
    }
    a.box.appendChild(line);
  });
  placeAt();
};
const chooseAt = (i) => {
  const a = atList;
  const row = a && a.rows[i];
  if (!row) return;
  const field = a.field;
  const end = field.selectionStart;
  let put = '';
  if (field.dataset.at === 'give') send({ t: 'ask_give_page', id: row.id });
  else put = `[${row.title.replace(/[\[\]\n]/g, ' ').trim() || 'Page'}](rune://page/${row.id}) `;
  const value = field.value.slice(0, a.start) + put + field.value.slice(end);
  closeAt();
  // (Through the engine's own setter, so that the app takes the new words as typed.)
  const own = Object.getOwnPropertyDescriptor(field.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, 'value');
  own.set.call(field, value);
  const caret = a.start + put.length;
  field.setSelectionRange(caret, caret);
  field.dispatchEvent(new Event('input', { bubbles: true }));
};
document.addEventListener(
  'input',
  (e) => {
    const field = atField(e.target);
    if (!field) return;
    const typed = atTyped(field);
    if (!typed) return closeAt();
    if (!atList || atList.field !== field) {
      closeAt();
      const box = document.createElement('div');
      box.className = 'at-list';
      box.setAttribute('role', 'listbox');
      box.setAttribute('aria-label', 'Pages');
      box.hidden = true;
      box.addEventListener('mousedown', (press) => {
        // (The keyboard stays in the box that is typed in.)
        press.preventDefault();
        press.stopPropagation();
        const line = press.target.closest && press.target.closest('.at-row');
        if (line) chooseAt(Number(line.dataset.at));
      });
      // In the app's own element: it holds the colours and the letters of
      // the look. (On the page's body the list had neither: black words
      // with no box, over what was behind them.)
      (document.querySelector('.app') || document.body).appendChild(box);
      atList = { field, box, rows: [], on: 0, start: 0, n: 0 };
    }
    atList.start = typed.start;
    atAsked += 1;
    atList.n = atAsked;
    const page = field.closest('[data-page]');
    send({ t: 'pages_find', q: typed.q, n: atAsked, here: (page && page.dataset.page) || '' });
  },
  true,
);
window.__rune.pagesFound = (n, rows) => {
  if (!atList || atList.n !== n) return;
  atList.rows = Array.isArray(rows) ? rows : [];
  atList.on = 0;
  drawAt();
};
// The keys of the list come before those of the box it belongs to (Return
// and Tab in a cell go to the next cell: here they choose the page).
window.addEventListener(
  'keydown',
  (e) => {
    const a = atList;
    if (!a || e.isComposing || e.keyCode === 229) return;
    if (e.key === 'Escape') {
      closeAt();
      e.preventDefault();
      e.stopImmediatePropagation();
      return;
    }
    if (!a.rows.length) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      a.on = (a.on + (e.key === 'ArrowDown' ? 1 : a.rows.length - 1)) % a.rows.length;
      drawAt();
    } else if (e.key === 'Enter' || e.key === 'Tab') chooseAt(a.on);
    else return;
    e.preventDefault();
    e.stopImmediatePropagation();
  },
  true,
);
document.addEventListener('focusout', (e) => {
  if (atList && e.target === atList.field) closeAt();
});
document.addEventListener('mousedown', (e) => {
  if (atList && e.target !== atList.field && !atList.box.contains(e.target)) closeAt();
});
// (A window that changes its size: the list goes with its box, as every
// menu does; see "an open menu stays at its thing".)
document.addEventListener(
  'scroll',
  () => {
    if (atList) placeAt();
  },
  true,
);

// ----- a card over a link to a page ---------------------------------------------------
// When the pointer rests on a link to a page (a link in a line, a line that
// is a sub-page, a link in a cell of a table, a row of "Linked from"), a
// small card says what the page is: its icon and name, where it is, and its
// first lines. The app gives the words (`preview_page` in state.rs) and
// says nothing for a page that is gone; the card is drawn here. It goes
// away when the pointer leaves the link, and at a press, a key, a scroll
// and a menu that opens. A press on the link does what it did. The card
// takes no pointer and no press (`pointer-events: none`): it lies over
// what is under the link, often the next link of a list, and that must
// stay what the pointer is on. (The card took the pointer: a press on the
// next name, through the card, put the card away and opened nothing.)
const LINKS_WITH_CARD = '.blk a[href^="rune://page/"], .blk a[href^="notion2://page/"], .page-link[data-preview], .linked-page[data-preview], .backlink[data-backlink]';
const CARD_REST = 400; // the rest of the pointer before a card shows, in ms
let linkCard = null; // { link, id, n, x, y, timer, box, watch }
let linkCardAsked = 0;
let linkCardShut = 0; // when the last card went away
let linkCardKey = 0; // when a key was last pressed
const linkCardPage = (link) => link.dataset.preview || link.dataset.backlink || ((link.getAttribute('href') || '').match(/^(?:rune|notion2):\/\/page\/([^/?#]+)/) || [])[1] || '';
const closeLinkCard = () => {
  const c = linkCard;
  if (!c) return;
  linkCard = null;
  clearTimeout(c.timer);
  if (c.watch) c.watch.disconnect();
  if (c.box) {
    c.box.remove();
    linkCardShut = performance.now();
  }
};
// No card while something is dragged, while a menu or a box is open over
// the page, or over a link in the line that is being typed in.
const noLinkCard = (link) => {
  if (dragging() || document.body.dataset.dragging || document.querySelector('.popover, .modal-backdrop, .at-list, .marquee')) return true;
  const line = link.closest('.blk');
  return !!line && line === document.activeElement && performance.now() - linkCardKey < 1500;
};
const placeLinkCard = () => {
  const c = linkCard;
  if (!c || !c.box) return;
  // Under the link, from the left of the part of it the pointer is on (a
  // link in a line can go over two lines); above it when there is no room
  // under it. Never over the link, and inside the window. (These places are
  // in points of the window, as the card is: a zoomed page changes nothing.)
  const all = c.link.getBoundingClientRect();
  const part = [...c.link.getClientRects()].find((r) => c.y >= r.top - 1 && c.y <= r.bottom + 1 && c.x >= r.left - 1 && c.x <= r.right + 1) || all;
  const w = c.box.offsetWidth;
  const h = c.box.offsetHeight;
  const gap = 6;
  const under = all.bottom + gap + h <= innerHeight - 6 || all.top - gap - h < 6;
  c.box.style.top = `${Math.round(under ? all.bottom + gap : all.top - gap - h)}px`;
  c.box.style.left = `${Math.round(Math.max(6, Math.min(part.left, innerWidth - w - 6)))}px`;
  c.box.classList.toggle('above', !under);
};
const askLinkCard = () => {
  const c = linkCard;
  if (!c) return;
  if (!c.link.isConnected || noLinkCard(c.link)) return closeLinkCard();
  linkCardAsked += 1;
  c.n = linkCardAsked;
  send({ t: 'page_preview', id: c.id, n: c.n });
};
window.__rune.pagePreview = (n, card) => {
  const c = linkCard;
  // (An answer for another page than the pointer is on now is dropped.)
  if (!c || c.n !== n || c.box) return;
  if (!card || card.id !== c.id || !c.link.isConnected || noLinkCard(c.link)) return closeLinkCard();
  const part = (cls, words, into) => {
    const el = document.createElement('div');
    el.className = cls;
    if (words) el.textContent = words;
    into.appendChild(el);
    return el;
  };
  const box = document.createElement('div');
  box.className = 'link-card';
  box.setAttribute('role', 'tooltip');
  const head = part('link-card-head', '', box);
  // (The icon is drawn by the app, with names it knows; all the words go in as text.)
  part('link-card-icon', '', head).innerHTML = card.icon || '';
  part('link-card-name', card.title, head);
  if (card.place) part('link-card-place', card.place, box);
  if (card.locked) part('link-card-lines locked', 'Locked', box);
  else if (Array.isArray(card.lines) && card.lines.length) part('link-card-lines', card.lines.join('\n'), box);
  // In the app's own element, for the colours and the letters of the look.
  (document.querySelector('.app') || document.body).appendChild(box);
  c.box = box;
  placeLinkCard();
  // A menu or a box that opens by itself (a command of the menu bar), or a
  // page that went from under the pointer: the card goes.
  c.watch = new MutationObserver(() => {
    if (linkCard === c && (!c.link.isConnected || document.querySelector('.popover, .modal-backdrop, .at-list'))) closeLinkCard();
  });
  c.watch.observe(document.querySelector('.app') || document.body, { childList: true, subtree: true });
};
document.addEventListener('mouseover', (e) => {
  const t = e.target;
  if (!t || !t.closest) return;
  const c = linkCard;
  const link = t.closest(LINKS_WITH_CARD);
  // On the link still: the card stays. Off it: the card goes.
  if (c && link === c.link) return;
  closeLinkCard();
  if (!link) return;
  const id = linkCardPage(link);
  // (A button held down: text is being selected, or something is dragged.)
  if (!id || e.buttons || noLinkCard(link)) return;
  // At once when a card was open a moment ago (from one link to the next).
  const rest = performance.now() - linkCardShut < 300 ? 0 : CARD_REST;
  linkCard = { link, id, n: 0, x: e.clientX, y: e.clientY, box: null, watch: null, timer: setTimeout(askLinkCard, rest) };
});
document.addEventListener('mousemove', (e) => {
  if (linkCard && !linkCard.box) [linkCard.x, linkCard.y] = [e.clientX, e.clientY];
});
// The pointer left the window.
document.addEventListener('mouseout', (e) => {
  if (linkCard && !e.relatedTarget) closeLinkCard();
});
for (const type of ['mousedown', 'wheel', 'scroll', 'dragstart', 'contextmenu']) document.addEventListener(type, closeLinkCard, true);
window.addEventListener(
  'keydown',
  () => {
    linkCardKey = performance.now();
    closeLinkCard();
  },
  true,
);
window.addEventListener('blur', closeLinkCard);
window.addEventListener('resize', closeLinkCard);

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
  // A picture in a cell of a table: the pictures of that table, in order.
  const table = img && img.isConnected && img.closest('.cell-pic') && img.closest('.table-block');
  if (table) return [...table.querySelectorAll('.cell-pic > img')];
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
    // (The right button is for its menu, "Copy picture"; a press that puts
    // that menu away does not put the picture away with it.)
    if (e.button !== 0 || e.ctrlKey) return;
    const menu = ctxMenu();
    if (menu && menu.classList.contains('show')) {
      closeCtx();
      e.stopPropagation();
      return;
    }
    e.stopPropagation();
    if (e.detail > 1 || performance.now() - opened < 350) return;
    hideLarge();
  });
  setTimeout(() => shade.classList.add('open'), 0);
}
// A press on a picture in a cell of a table shows it large too (the small
// square in the cell is all that could be seen of a screenshot).
document.addEventListener('click', (e) => {
  if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const img = e.target.closest && e.target.closest('.cell-pic > img');
  if (!img) return;
  e.preventDefault();
  showLarge(img);
});
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
    // Copy is for the picture that is shown. (The view went away and the
    // key copied what was picked on the page before.)
    const kept = e.key.toLowerCase() === 'c' && !e.shiftKey && !e.altKey && keptPicture(large.big);
    if (kept) {
      e.preventDefault();
      e.stopImmediatePropagation();
      send({ t: 'copy_picture', name: kept });
    }
    hideLarge();
    return;
  }
  e.preventDefault();
  e.stopImmediatePropagation();
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') stepLarge(1);
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') stepLarge(-1);
  else if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') hideLarge();
  else if (e.key === 'Backspace' || e.key === 'Delete') {
    // Delete takes the picture that is shown out of its page (Undo brings
    // it back), or out of its cell of a table, by the cell's own button
    // (there the view went away and nothing was taken out).
    const box = large.from.isConnected && large.from.closest('.image-block[data-image]');
    const inCell = large.from.isConnected && large.from.closest('.cell-pic');
    hideLarge();
    if (box) send({ t: 'image_delete', page: box.dataset.page, id: box.dataset.image });
    else if (inCell && inCell.querySelector('.pic-x')) inCell.querySelector('.pic-x').click();
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
  // (Let go where the page did not see it: the picture stays as it was,
  // and the next click is not the end of this drag.)
  if (!(e.buttons & 1)) {
    picDrag = null;
    if (d.size && d.moved) {
      d.sizeEl.style.width = d.was;
      d.box.classList.toggle('sized', d.wasSized);
    }
    endPicDrag(d);
    return;
  }
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
    if (line.bottom > view.bottom - room) scroller.scrollTop += (line.bottom - (view.bottom - room)) / zoomOf(scroller);
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
        const row = pane && pane.querySelector(pageRows('[data-page]'));
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
      const all = [...pickedRows()[0].closest('.blocks').querySelectorAll(rowsWithin('[data-id]'))];
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
    // ⌘⇧P: "Move to…" for them. Its list hangs on the first of them that
    // is in sight (one out of sight would close it at once).
    if (cmd && e.shiftKey && key === 'p') {
      e.preventDefault();
      e.stopImmediatePropagation();
      const rows = [...pickedRows()];
      const seen = rows.find((r) => inSight(r, true)) || rows[0];
      if (!inSight(seen, true)) seen.scrollIntoView({ block: 'nearest' });
      send({ t: 'block', action: 'move_to', page: seen.dataset.page, id: seen.dataset.id, pos: 0 });
      return;
    }
    if (cmd && key === 'c') act('picked_copy');
    else if (cmd && key === 'x') act('picked_cut');
    // (Not while the AI is asked about them: the keyboard may be on the page
    // for a moment then, and the key is not meant for the blocks.)
    else if ((e.key === 'Backspace' || e.key === 'Delete') && !aiHoldsPicks()) act('picked_delete');
    else if (e.key === 'Escape') act('pick_clear');
  },
  true,
);
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.button !== 0 || e.ctrlKey || !e.target.closest) return;
    const t = e.target;
    const row = t.closest(pageRows());
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
    // (A press on the AI box keeps what is picked: the request is about it.
    // So does one on "Move to…" and on its list of pages.)
    if (some && !t.closest('.ai-dock, .move-menu, #move-lines')) send({ t: 'pick_clear' });
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

// The words of what is selected: while words of a line are selected, or lines
// are picked as a whole, a quiet count shows ("12 words"), in the foot of the
// page's menu when that is open, else at the foot of the pane, at its right.
// Words as the page's own count counts them: what is between white space
// (Rust's white space, `Block::words`); the app counts the words of picked
// lines itself and puts them on the page's lines (`data-picked-words`).
const WHITE = /[\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/;
const wordsIn = (text) => text.replace(/[\u200b\ufeff]/g, '').split(WHITE).filter(Boolean).length;
const selWords = () => {
  const lines = [...document.querySelectorAll('.blocks[data-picked-words]')].find((b) => Number(b.dataset.pickedWords) > 0 && b.querySelector(rowsWithin('.picked')));
  if (lines) return { n: Number(lines.dataset.pickedWords), pane: lines.closest('.pane') };
  const sel = getSelection();
  if (!sel || !sel.rangeCount || sel.isCollapsed) return null;
  const line = blockOf(sel.anchorNode);
  if (!line || line !== blockOf(sel.focusNode)) return null;
  const n = wordsIn(sel.toString());
  return n ? { n, pane: line.closest('.pane') } : null;
};
// (The foot of the page's menu shows the count by its style, whenever the menu
// is drawn: `--sel-words` on the pane.)
let selWordsQueued = false;
const showSelWords = () => {
  selWordsQueued = false;
  const got = selWords();
  for (const pane of document.querySelectorAll('.pane')) {
    const here = !!got && got.pane === pane;
    let label = pane.querySelector(':scope > .sel-words');
    if (here && !label) {
      label = document.createElement('div');
      label.className = 'sel-words';
      label.setAttribute('aria-hidden', 'true');
      pane.appendChild(label);
    }
    if (here) {
      const words = `${got.n} ${got.n === 1 ? 'word' : 'words'}`;
      if (label.textContent !== words) label.textContent = words;
      const foot = JSON.stringify(` · ${got.n} selected`);
      if (pane.style.getPropertyValue('--sel-words') !== foot) pane.style.setProperty('--sel-words', foot);
    } else if (pane.style.getPropertyValue('--sel-words')) pane.style.removeProperty('--sel-words');
    if (label && label.classList.contains('on') !== here) label.classList.toggle('on', here);
  }
};
const queueSelWords = () => {
  if (selWordsQueued) return;
  selWordsQueued = true;
  requestAnimationFrame(showSelWords);
};
document.addEventListener('selectionchange', queueSelWords);
// (Lines picked or let go.)
new MutationObserver(queueSelWords).observe(document.documentElement, { subtree: true, attributes: true, attributeFilter: ['data-picked-words'] });

// ----- a box drawn over the page picks the blocks it touches ----------------------
// As in Notion: a press on the empty part of a page (its margins, the space
// under its last line, the part of a line that is not text or a control) and
// a drag draws a box, and every block the box touches is picked, a table or
// a picture too: then ⌘C copies them, ⌘X cuts them, Delete takes them out.
// The box begins at a place of the page, not of the window: near the top or
// the foot of the page, the page scrolls and the box grows with it.
const MARQUEE_OWN = '.blk, input, textarea, select, button, a, .g-btn, .marker, .image-block, .table-wrap, .row-values, .drawing, .video-embed, .popover, .ai-panel, .page-link, .code-wrap, .callout-icon, .check, .toggle-arrow';
let marquee = null;
let marqueeEnded = -Infinity;
// (While the box is drawn, the page script marks the blocks itself, at once
// (data-mq): the app hears what is picked when the button is let go, so
// nothing is drawn again in the middle of the drag.)
const marqueeRows = (m) => {
  // Where each block is in the page (not in the window: the page scrolls).
  const sx = m.scroller.scrollLeft * m.z;
  const sy = m.scroller.scrollTop * m.z;
  m.rows = [...m.blocks.querySelectorAll(rowsWithin('[data-id]'))].map((row) => {
    const r = row.getBoundingClientRect();
    return { row, left: r.left + sx, right: r.right + sx, top: r.top + sy, bottom: r.bottom + sy, depth: Math.round((parseFloat(row.style.paddingLeft) || 0) / 26) };
  });
};
const marqueeDraw = () => {
  const m = marquee;
  if (!m || !m.on) return;
  const sx = m.scroller.scrollLeft * m.z;
  const sy = m.scroller.scrollTop * m.z;
  // Where the press was, now that the page may have scrolled.
  const ax = m.x0 - (sx - m.sx0);
  const ay = m.y0 - (sy - m.sy0);
  const left = Math.min(ax, m.x);
  const top = Math.min(ay, m.y);
  const right = Math.max(ax, m.x);
  const bottom = Math.max(ay, m.y);
  // (Only the part that is in the page shows.)
  const view = m.view;
  const st = m.box.style;
  const x = Math.max(left, view.left);
  const y = Math.max(top, view.top);
  st.transform = `translate(${x}px, ${y}px)`;
  st.width = `${Math.max(0, Math.min(right, view.right) - x)}px`;
  st.height = `${Math.max(0, Math.min(bottom, view.bottom) - y)}px`;
  let first = -1;
  let last = -1;
  m.rows.forEach((r, i) => {
    if (r.bottom - sy <= top || r.top - sy >= bottom || r.right - sx <= left || r.left - sx >= right) return;
    if (first < 0) first = i;
    last = i;
  });
  // (With a block, what is nested in it. As the app picks: to the end of
  // what is nested in the first block or in the last, the further of the two.)
  if (last >= 0) {
    const under = (i) => {
      let j = i + 1;
      while (j < m.rows.length && m.rows[j].depth > m.rows[i].depth) j += 1;
      return j - 1;
    };
    last = Math.max(under(first), under(last));
  }
  if (first === m.first && last === m.last) return;
  m.first = first;
  m.last = last;
  m.rows.forEach((r, i) => {
    const on = first >= 0 && i >= first && i <= last;
    if (on !== r.row.hasAttribute('data-mq')) r.row.toggleAttribute('data-mq', on);
  });
};
// Near the top or the foot of the page, the page scrolls under the pointer.
// (A box that begins beside an edge does not scroll the page at once: that
// edge pulls only after the pointer was away from it, or went on toward it.
// The speed is by the time that went by, not by the frames of the screen.)
const marqueeGlide = (now) => {
  const m = marquee;
  if (!m || !m.on) return;
  const edge = 36;
  const dt = m.at ? Math.min(50, now - m.at) : 0;
  m.at = now;
  if (m.y > m.view.top + edge || m.y < m.y0 - 12) m.pullTop = true;
  if (m.y < m.view.bottom - edge || m.y > m.y0 + 12) m.pullFoot = true;
  const over = m.pullTop && m.y < m.view.top + edge ? m.y - (m.view.top + edge) : m.pullFoot && m.y > m.view.bottom - edge ? m.y - (m.view.bottom - edge) : 0;
  if (over && dt) {
    m.carry = (m.carry || 0) + Math.max(-24, Math.min(24, over * 0.4)) * (dt / 16.7);
    const step = Math.trunc(m.carry);
    if (step) {
      m.carry -= step;
      const before = m.scroller.scrollTop;
      m.scroller.scrollTop += step;
      if (m.scroller.scrollTop !== before) marqueeDraw();
    }
  } else m.carry = 0;
  m.frame = requestAnimationFrame(marqueeGlide);
};
const marqueeEnd = () => {
  const m = marquee;
  marquee = null;
  if (!m || !m.on) return;
  cancelAnimationFrame(m.frame);
  m.box.remove();
  document.body.classList.remove('drag-picking', 'box-picking');
  const marked = m.rows.filter((r) => r.row.hasAttribute('data-mq'));
  // (A press that hardly moved and touched no block is a click: under the
  // last line it makes a new line, as before.)
  if (marked.length || Math.hypot(m.x - m.x0, m.y - m.y0) >= 10 || m.scroller.scrollTop * m.z !== m.sy0) {
    marqueeEnded = performance.now();
    // (A line under the pointer does not take the caret: see focusin.)
    dragPickEnded = marqueeEnded;
  }
  // Now the app hears what is picked. The marks of the drag stay until its
  // own marks are drawn, so nothing blinks between the two.
  const done = () => {
    for (const r of m.rows) r.row.removeAttribute('data-mq');
    m.blocks.removeAttribute('data-mq-on');
  };
  if (!marked.length || !m.blocks.isConnected) {
    send({ t: 'pick_clear' });
    done();
    return;
  }
  send({ t: 'pick', page: m.page, id: m.rows[m.first].row.dataset.id, extend: false });
  if (m.last !== m.first) send({ t: 'pick', page: m.page, id: m.rows[m.last].row.dataset.id, extend: true });
  let tries = 0;
  const wait = () => {
    // (Both ends: the block under the press can be picked from before.)
    if ((m.rows[m.first].row.classList.contains('picked') && m.rows[m.last].row.classList.contains('picked')) || (tries += 1) > 40) done();
    else requestAnimationFrame(wait);
  };
  requestAnimationFrame(wait);
};
document.addEventListener(
  'mousedown',
  (e) => {
    marqueeEnd();
    if (e.button !== 0 || e.shiftKey || e.metaKey || e.ctrlKey || e.altKey || !e.target.closest) return;
    const t = e.target;
    const scroller = t.closest('.scroller');
    const blocks = scroller && scroller.querySelector(':scope > .page > .blocks');
    const row = blocks && blocks.querySelector(rowsWithin('[data-page]'));
    if (!row) return;
    // The empty part of the page only: not its heading, not what has a use of
    // its own for a press.
    // (The frame of a table itself, with no cell or control under the press,
    // is its empty edge: the strip at its left, the space under its rows.)
    const empty = t === scroller || t.matches('.page, .blocks, .columns, .column, .page-tail') || (t.closest(pageRows()) && (!t.closest(MARQUEE_OWN) || t.matches('.table-block')));
    if (!empty) return;
    // (A zoomed page scrolls in its own units: places in the window are those
    // times the zoom.)
    const z = zoomOf(scroller);
    marquee = { scroller, blocks, page: row.dataset.page, x0: e.clientX, y0: e.clientY, z, sx0: scroller.scrollLeft * z, sy0: scroller.scrollTop * z, x: e.clientX, y: e.clientY, on: false, box: null, rows: [], view: null, first: -2, last: -2, frame: 0 };
    // No selection of text begins with this press: the engine would carry
    // it from line to line under the pointer, the caret with it, and scroll
    // the page by itself. What the press did before stays: the line that
    // had the caret lets it go (a line, a cell of a table, the title).
    if (!e.defaultPrevented) {
      e.preventDefault();
      const a = document.activeElement;
      if (a && a !== document.body && a.blur) a.blur();
      const sel = getSelection();
      if (sel && sel.rangeCount) sel.removeAllRanges();
    }
  },
  true,
);
document.addEventListener(
  'mousemove',
  (e) => {
    const m = marquee;
    if (!m) return;
    if (!(e.buttons & 1) || !m.blocks.isConnected) {
      marqueeEnd();
      return;
    }
    m.x = e.clientX;
    m.y = e.clientY;
    if (!m.on) {
      if (Math.hypot(m.x - m.x0, m.y - m.y0) < 5) return;
      m.on = true;
      m.view = m.scroller.getBoundingClientRect();
      marqueeRows(m);
      m.blocks.setAttribute('data-mq-on', '');
      m.box = document.createElement('div');
      m.box.className = 'marquee';
      // (In the app's own frame: the colours of the look are set there.)
      (m.scroller.closest('.app') || document.body).appendChild(m.box);
      // (box-picking: nothing under the pointer answers to it in the drag,
      // so no handle, row box or other pointer look comes and goes.)
      document.body.classList.add('drag-picking', 'box-picking');
      const a = document.activeElement;
      if (a && isBlock(a)) a.blur();
      hideBar();
      m.frame = requestAnimationFrame(marqueeGlide);
    }
    const sel = getSelection();
    if (sel && sel.rangeCount) sel.removeAllRanges();
    e.preventDefault();
    marqueeDraw();
  },
  true,
);
window.addEventListener('mouseup', marqueeEnd, true);
window.addEventListener('blur', marqueeEnd);
// The click that ends the drag is not a click on the page.
document.addEventListener(
  'click',
  (e) => {
    if (performance.now() - marqueeEnded > 80) return;
    e.preventDefault();
    e.stopPropagation();
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
window.__rune.hearStart = () => {
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
window.__rune.hear = (text) => {
  if (!hearBox) return;
  if (!hearBox.isConnected || document.activeElement !== hearBox) return hearLost();
  hearBox.value = hearBase + (hearBase && !/\s$/.test(hearBase) ? ' ' : '') + text;
  hearBox.dispatchEvent(new Event('input', { bubbles: true }));
  hearBox.scrollTop = hearBox.scrollHeight;
};
window.__rune.hearEnd = hearEnd;
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
  // (Not in a locked page: the AI does not read it.)
  const row = isBlock(el) && !el.closest('.page.no-ai') ? el.closest('.block-row.kind-text') : null;
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
  // A folded heading hides lines (they are not in the page): words found in
  // them count as one match, at the end of the heading, and the heading
  // opens when the search comes to it (`revealMatch`).
  for (const row of root.querySelectorAll('.block-row.folded[data-fold-text]')) {
    const blk = row.querySelector('.blk');
    if (!blk || !row.dataset.foldText.toLowerCase().includes(needle)) continue;
    const r = document.createRange();
    r.selectNodeContents(blk);
    r.collapse(false);
    r._fold = { page: row.dataset.page, id: row.dataset.id };
    const at = out.findIndex((m) => m.compareBoundaryPoints(Range.START_TO_START, r) > 0);
    out.splice(at < 0 ? out.length : at, 0, r);
  }
  return out;
};

const paintFind = () => {
  const count = document.getElementById('find-count');
  if (count) {
    count.textContent = !findInput() || !findInput().value ? '' : findMatches.length ? `${findAt + 1} of ${findMatches.length}` : 'No matches';
  }
  if (!canHighlight) return;
  if (findMatches.length) CSS.highlights.set('rune-find', new Highlight(...findMatches));
  else CSS.highlights.delete('rune-find');
  const current = findMatches[findAt];
  if (current) CSS.highlights.set('rune-find-current', new Highlight(current));
  else CSS.highlights.delete('rune-find-current');
};

// (Set while a folded heading opens for the search: the match it hid is shown next.)
let findOpens = false;
const revealMatch = () => {
  const r = findMatches[findAt];
  if (r && r._fold) {
    findOpens = true;
    send({ t: 'block', page: r._fold.page, id: r._fold.id, action: 'unfold' });
    return;
  }
  const el = r && r.startContainer.parentElement;
  if (el) el.scrollIntoView({ block: 'center', behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });
};

// `keepPlace`: the page changed under an open search; stay on the same match.
const runFind = (keepPlace) => {
  findMatches = collectMatches(findRoot(), findInput() ? findInput().value : '');
  if (!keepPlace || findAt >= findMatches.length || findAt < 0) findAt = findMatches.length ? 0 : -1;
  paintFind();
  const opened = findOpens;
  findOpens = false;
  if (!keepPlace || opened) revealMatch();
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

// ----- an open menu stays at its thing ---------------------------------------------
// One rule for every menu that hangs on a thing of the page: the menu of a
// line, the menus of a table (a cell, a row, a heading, the date picker, a
// list of choices), the day of a to-do, the menu of a page in the sidebar
// and of a tab, the lists of the event panel, the "/" menu and the "@" lists.
//   - The person scrolls the page with the wheel: a menu with a shade
//     closes (see "the wheel over an open menu's shade" below). A scroll
//     inside a menu's own list does nothing to the menu.
//   - The page moves under an open menu in any other way (a scroll made by
//     a key or by the app, the window, the sidebar or the zoom changing the
//     size of the page): the menu is placed again by the function that
//     placed it first, so it stays at its thing. When its thing is out of
//     sight, the menu closes.
// (They were placed once, and stayed where they were when their thing went.)
const FOLLOWED = [
  ['.block-menu.anchored', placeBlockMenu, (m) => m.parentElement, 28],
  ['.sidebar-menu.anchored, .tab-menu.anchored, .due-menu.anchored, .ed-menu.anchored', anchorMenu, (m) => m.parentElement, 0],
  [TABLE_MENUS, placeTableMenu, (m) => (hangsFrom(m) || {}).el, 0],
  ['.slash-menu.anchored', placeSlashMenu, (m) => m.closest('.block-row') || m.parentElement, 0],
];
const OWN_SCROLL = '.popover, .block-menu, .slash-menu, .ed-menu, .at-list, .ai-menu';
// Whether a thing shows in the part of the page that scrolls it (`first`:
// only its first line counts, in points of the page).
const inSight = (el, first) => {
  const r = el.getBoundingClientRect();
  // (A thing with no box of its own has no place to be out of.)
  if (!r.width && !r.height) return true;
  const bottom = first ? Math.min(r.bottom, r.top + first * zoomOf(el)) : r.bottom;
  for (let n = el.parentElement; n && n !== document.documentElement; n = n.parentElement) {
    const o = getComputedStyle(n).overflowY;
    if (o !== 'auto' && o !== 'scroll' && o !== 'hidden') continue;
    const b = n.getBoundingClientRect();
    if (bottom <= b.top + 1 || r.top >= b.bottom - 1) return false;
  }
  return bottom > 0 && r.top < innerHeight;
};
// A menu placed while what it is in still moves into its place (a panel
// that opens with a motion is, for that moment, what a fixed menu is placed
// against) is placed once more when the motion has ended.
const awaited = new WeakSet();
const restThenFollow = (menu) => {
  for (let n = menu.parentElement; n && n !== document.body; n = n.parentElement) {
    for (const motion of n.getAnimations ? n.getAnimations() : []) {
      if (motion.playState !== 'running' || awaited.has(motion)) continue;
      awaited.add(motion);
      motion.finished.then(queueFollow, () => {});
    }
  }
};
// (Every such menu is a `.popover` or a list of the event panel. These two
// lists are kept by the engine, so that a scroll with no menu open, which
// is nearly every scroll, looks through the page for none.)
const openPopovers = document.getElementsByClassName('popover');
const openEdMenus = document.getElementsByClassName('ed-menu');
const followMenus = () => {
  if (!atList && !openPopovers.length && !openEdMenus.length) return;
  for (const [which, place, thingOf, first] of FOLLOWED) {
    for (const menu of document.querySelectorAll(which)) {
      if (menu.dataset.gone) continue;
      const thing = thingOf(menu);
      // (The lists of the event panel have a shade of their own, with no
      // page that scrolls under it: they are placed again and stay open.)
      if (thing && !inSight(thing, first) && !menu.classList.contains('ed-menu')) {
        menu.dataset.gone = '1';
        menu.style.visibility = 'hidden';
        send({ t: 'shortcut', name: 'close_menu' });
      } else {
        place(menu);
        restThenFollow(menu);
      }
    }
  }
  if (atList) {
    if (inSight(atList.field, 0)) placeAt();
    else closeAt();
  }
};
let followQueued = false;
const queueFollow = () => {
  if (followQueued) return;
  followQueued = true;
  requestAnimationFrame(() => {
    followQueued = false;
    followMenus();
  });
};
document.addEventListener(
  'scroll',
  (e) => {
    if (e.target.closest && e.target.closest(OWN_SCROLL)) return;
    queueFollow();
  },
  { capture: true, passive: true },
);
window.addEventListener('resize', queueFollow);

// ----- the message at the foot --------------------------------------------------
// A press on the "Undo" of a message leaves the keyboard where it is: the
// caret stays in the line, and what is typed next goes there.
document.addEventListener(
  'mousedown',
  (e) => {
    if (e.target.closest && e.target.closest('.toast')) e.preventDefault();
  },
  true,
);
// The message sits clear of what else is at the foot of a pane: the AI's
// button, its box, its answer with its buttons, what it did a moment ago;
// and of the small video player and the meeting pad drawn in the page (in
// a window that is not wide, or with the player moved to the foot).
// It was drawn over them. Now each of them that it would lie over lifts
// it: it sits above the highest. (From the foot up; with no room above
// them all, it stays at the foot.) Seen again when its words change and
// while it shows, since the AI's box can open under it.
const TOAST_FOOT = 28;
const seatToast = () => {
  const t = document.querySelector('.toast');
  if (!t) return;
  const h = t.offsetHeight;
  const left = innerWidth / 2 - t.offsetWidth / 2 - 8;
  const right = innerWidth / 2 + t.offsetWidth / 2 + 8;
  const boxes = [...document.querySelectorAll('.ai-dock, .ai-done, .player-dock, .pad-inline')]
    .map((el) => el.getBoundingClientRect())
    .filter((b) => b.width > 0 && b.right > left && b.left < right)
    .sort((a, b) => b.bottom - a.bottom);
  let foot = TOAST_FOOT;
  for (const b of boxes) {
    if (b.bottom > innerHeight - foot - h - 8 && b.top < innerHeight - foot + 8) foot = innerHeight - b.top + 10;
  }
  if (foot + h > innerHeight - 8) foot = TOAST_FOOT;
  const want = foot === TOAST_FOOT ? '' : `${Math.round(foot)}px`;
  if (t.style.bottom !== want) t.style.bottom = want;
};
const watchToast = (t) => {
  seatToast();
  new MutationObserver(seatToast).observe(t, { childList: true, characterData: true, subtree: true });
  const again = setInterval(() => (t.isConnected ? seatToast() : clearInterval(again)), 250);
};
window.addEventListener('resize', seatToast);

// ----- the room a scroll bar takes ------------------------------------------------
// A page that grows past the window gets a scroll bar, which takes 10
// points from the width; a column in the middle (the page, Home, Settings)
// then moved 5 points to the left. What scrolls says how much room its bar
// takes at this moment (--bar, in its own points: 0 with no bar), and the
// styles keep the column where it is with no bar (style.css, `--bar`).
// Seen before the next picture is drawn, so nothing moves on the screen.
const BAR_BOXES = '.scroller, .settings-scroll';
const barRoom =
  typeof ResizeObserver !== 'undefined'
    ? new ResizeObserver((entries) => {
        for (const { target } of entries) {
          const room = `${target.offsetWidth - target.clientWidth}px`;
          if (target.style.getPropertyValue('--bar') !== room) target.style.setProperty('--bar', room);
        }
        // (The page has another size: an open menu goes with its thing.)
        queueFollow();
      })
    : null;
const watchBars = (n) => {
  if (!barRoom || !n.querySelectorAll) return;
  if (n.matches(BAR_BOXES)) barRoom.observe(n);
  n.querySelectorAll(BAR_BOXES).forEach((box) => barRoom.observe(box));
};
watchBars(document.body);

// ----- new blocks get their text from data-html ----------------------------------

new MutationObserver((mutations) => {
  for (const m of mutations) {
    if (isBlock(m.target) || blockOf(m.target)) continue; // edits inside a block
    m.addedNodes.forEach((n) => {
      initWithin(n);
      if (n.nodeType !== 1) return;
      watchBars(n);
      if (n.classList.contains('toast')) watchToast(n);
      if (n.tagName === 'TEXTAREA' || (n.querySelector && n.querySelector('textarea.title-input, textarea.ed-title'))) queueSizeAll();
      const menus = n.matches && n.matches('.sidebar-menu, .tab-menu, .due-menu, .ed-menu') ? [n] : n.querySelectorAll ? n.querySelectorAll('.sidebar-menu, .tab-menu, .due-menu, .ed-menu') : [];
      (n.matches && n.matches('.block-menu') ? [n] : n.querySelectorAll ? [...n.querySelectorAll('.block-menu')] : []).forEach(placeBlockMenu);
      (n.matches && n.matches('.slash-menu') ? [n] : n.querySelectorAll ? [...n.querySelectorAll('.slash-menu')] : []).forEach((menu) => {
        placeSlashMenu(menu);
        slashWatch.observe(menu, { childList: true, subtree: true });
      });
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
  const go = (head) => {
    const scroller = head.closest('.scroller');
    if (!scroller) return;
    // (On a zoomed page the places are in points of the window and the scroll
    // is in the page's own units.)
    const top = (head.getBoundingClientRect().top - scroller.getBoundingClientRect().top) / zoomOf(scroller) + scroller.scrollTop - 72;
    const calm = matchMedia('(prefers-reduced-motion: reduce)').matches;
    scroller.scrollTo({ top: Math.max(0, top), behavior: calm ? 'auto' : 'smooth' });
  };
  const head = document.getElementById('blk-' + item.dataset.outline);
  if (head) return go(head);
  // A heading under a folded heading is not in the page: that one opens
  // first, and the page goes to the heading when it is drawn.
  const nav = item.closest('.page-outline');
  if (!nav || !nav.dataset.page) return;
  send({ t: 'block', page: nav.dataset.page, id: item.dataset.outline, action: 'show' });
  let tries = 0;
  const drawn = () => {
    const now = document.getElementById('blk-' + item.dataset.outline);
    if (now) go(now);
    else if (tries++ < 90) requestAnimationFrame(drawn);
  };
  requestAnimationFrame(drawn);
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

// ----- a page keeps its scroll place -------------------------------------------------
// The box that scrolls a pane is used again for the next tab, and made anew
// for a split: the page in it began at its top each time. The place of each
// page in each pane is kept here (in this window's memory only) and put back
// when the pane shows that page again, before the window is drawn. A page
// that a pane goes to anew starts at its top (`placeFresh`, from Rust).
// (`scrollTop` is in the box's own units, so a zoomed page needs no sum.)
const PLACE_BOXES = '.pane > .scroller[data-place], .pane > .scroller > .calendar .cal-scroll';
const places = new Map(); // "pane|what it shows" -> scrollTop
// The hours of the calendar: when their place was last set (by a scroll).
// That place holds for a short time only: the calendar that is opened again
// later, or on another day, opens at now (`calOpen`), not at the hours of
// this morning. A page keeps its place with no such limit.
const HOURS_PLACE_MS = 30 * 60 * 1000;
const hoursSet = new Map(); // key -> [time, day]
const hoursKey = (key) => key.endsWith('#hours');
const hoursFresh = (key) => {
  const set = hoursSet.get(key);
  return !!set && Date.now() - set[0] < HOURS_PLACE_MS && set[1] === new Date().toDateString();
};
const placeWaits = new Set(); // boxes whose page is not yet as long as its place needs
const placeKeyOf = (el) => {
  const pane = el.closest('.pane');
  const what = el.classList.contains('cal-scroll') ? 'calendar#hours' : el.dataset.place;
  return pane && pane.dataset.pane && what ? pane.dataset.pane + '|' + what : '';
};
const placePut = (el) => {
  // (Hours whose place is old are left for `calOpen`.)
  if (hoursKey(el._place) && !hoursFresh(el._place)) {
    places.delete(el._place);
    hoursSet.delete(el._place);
    return;
  }
  const want = places.get(el._place) || 0;
  el.scrollTop = want;
  placeWaits.delete(el);
  el._placeWant = null;
  if (Math.abs(el.scrollTop - want) <= 1) return;
  // (A picture, a table or a diagram gets its height a little later.)
  el._placeWant = want;
  placeWaits.add(el);
  let frames = 0;
  const again = () => {
    if (el._placeWant !== want) return;
    if (el.isConnected) el.scrollTop = want;
    if (!el.isConnected || Math.abs(el.scrollTop - want) <= 1 || ++frames > 90) {
      el._placeWant = null;
      placeWaits.delete(el);
    } else requestAnimationFrame(again);
  };
  requestAnimationFrame(again);
};
// `shown`: also look for a box that was not drawn for a time (the other
// panes in focus mode): it lost its place.
const placesSync = (shown) => {
  for (const el of document.querySelectorAll(PLACE_BOXES)) {
    const key = placeKeyOf(el);
    if (key !== el._place) {
      el._place = key;
      el._placeHid = false;
      if (key) {
        initWithin(el);
        placePut(el);
      }
    } else if (shown && key) {
      const hid = !el.clientHeight;
      if (!hid && el._placeHid) placePut(el);
      el._placeHid = hid;
    }
  }
};
let placeApp = null;
const placeAppWatch = new MutationObserver(() => placesSync(true));
new MutationObserver(() => {
  const app = document.querySelector('.app');
  if (app && app !== placeApp) {
    placeApp = app;
    placeAppWatch.observe(app, { attributes: true, attributeFilter: ['class'] });
  }
  placesSync(false);
}).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-pane', 'data-place'] });
document.addEventListener(
  'scroll',
  (e) => {
    const el = e.target;
    if (!(el instanceof Element && el._place && el._placeWant == null && el.clientHeight)) return;
    places.set(el._place, el.scrollTop);
    if (hoursKey(el._place)) hoursSet.set(el._place, [Date.now(), new Date().toDateString()]);
  },
  true,
);
// (The person scrolls: a place that still waits for its page is let go.)
for (const type of ['wheel', 'mousedown', 'keydown', 'touchstart']) {
  window.addEventListener(
    type,
    () => {
      for (const el of placeWaits) el._placeWant = null;
      placeWaits.clear();
    },
    { capture: true, passive: true },
  );
}
window.__rune.placeFresh = (pane, what) => {
  for (const key of [pane + '|' + what, pane + '|' + what + '#hours']) {
    places.delete(key);
    hoursSet.delete(key);
    for (const el of document.querySelectorAll(PLACE_BOXES)) {
      if (el._place !== key || el.classList.contains('cal-scroll')) continue;
      el._placeWant = null;
      el.scrollTop = 0;
    }
  }
};
// A copy of a tab ("Duplicate Tab"): each new pane starts at the places of
// the pane it is a copy of (and the hours of its calendar, as they are).
window.__rune.placeCopy = (from, to) => {
  for (const [key, top] of [...places]) {
    if (!key.startsWith(from + '|')) continue;
    const mine = to + key.slice(from.length);
    places.set(mine, top);
    if (hoursSet.has(key)) hoursSet.set(mine, hoursSet.get(key));
  }
  // (The new pane may be drawn already, at its top.)
  for (const el of document.querySelectorAll(PLACE_BOXES)) if (el._place && el._place.startsWith(to + '|')) placePut(el);
};
window.__rune.placeKept = (el) => {
  const key = placeKeyOf(el);
  return places.has(key) && (!hoursKey(key) || hoursFresh(key));
};
placesSync(false);

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
  const open = (n) => send({ t: 'open_url', url: 'rune://page/' + n.id });

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
      // (Let go where the page did not see it: the drag ends where it is,
      // and the picture does not follow the pointer until the next press.)
      if (!(e.buttons & 1)) {
        g.drag = null;
        canvas.style.cursor = g.hover >= 0 ? 'pointer' : 'grab';
        return;
      }
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

// ----- the calendar: the hours open at now ---------------------------------------------
// The week and the day open with the line of now in sight, a third of the
// way down, so that the hours before it show too. Hours that do not hold
// today open at `morning` (8 AM). Early and late in the day the hours end
// at midnight, and the view with them: the line is then nearer an edge.
// This is done once for each time the hours come to the screen, and again
// at "Today" (`day` is today: the hours that show it are waited for). It
// is never done while the person looks at the hours: the line moves each
// minute, the view does not. (All in the scroller's own points, so that a
// zoomed page opens at the same hour.)
const calPlace = (s, morning) => {
  const now = s.querySelector('.cal-now');
  s.scrollTop = now ? Math.max(0, (parseFloat(now.style.top) || 0) - s.clientHeight / 3) : morning;
};
window.__rune.calOpen = (morning, day) => {
  const all = () => [...document.querySelectorAll('.cal-scroll')];
  if (!day) {
    all().filter((s) => !s._opened).forEach((s) => {
      s._opened = true;
      calPlace(s, morning);
    });
    return;
  }
  const place = () => {
    const shown = all().filter((s) => s.querySelector(`.cal-col[data-day="${day}"]`));
    shown.forEach((s) => calPlace(s, morning));
    return shown.length > 0;
  };
  if (place()) return;
  // (The hours of today are drawn a moment after the press: as soon as
  // they are in the page. In the month there are none, and nothing is done.)
  const seen = new MutationObserver(() => place() && seen.disconnect());
  seen.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-day'] });
  setTimeout(() => seen.disconnect(), 1000);
};

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
    evDrag = { el, id: el.dataset.event, x: e.clientX, y: e.clientY, grow, moved: false, at: cols.indexOf(col), count: cols.length, z: zoomOf(el), wide: col.getBoundingClientRect().width, high: box.height, was: el.style.height, days: 0, mins: 0 };
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
  // (The pointer, and the sizes read from the page, are in points of the
  // window; what is set on the event is in the page's own units: on a
  // zoomed page the two differ by the zoom.)
  d.mins = Math.round(dy / (QUARTER * d.z)) * 15;
  if (d.grow) {
    d.mins = Math.max(d.mins, 15 - Math.round(d.high / d.z / QUARTER) * 15);
    d.el.style.height = Math.max(QUARTER, d.high / d.z + (d.mins / 15) * QUARTER) + 'px';
  } else {
    d.days = Math.max(-d.at, Math.min(d.count - 1 - d.at, Math.round(dx / d.wide)));
    d.el.style.transform = 'translate(' + (d.days * d.wide) / d.z + 'px, ' + (d.mins / 15) * QUARTER + 'px)';
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

// ----- the calendar: ⌘C on an event, ⌘V at a time -----------------------------------------
// ⌘C copies the event under the pointer (else the one picked); ⌘V puts a
// copy at the time under the pointer, or on the day under it at its own
// time (the month, the row of all-day events). Not while typing.
let calPointer = { x: -1, y: -1 };
window.addEventListener('mousemove', (e) => {
  calPointer = { x: e.clientX, y: e.clientY };
}, { passive: true, capture: true });
window.addEventListener(
  'keydown',
  (e) => {
    if (!e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || e.isComposing) return;
    const key = e.key.toLowerCase();
    if (key !== 'c' && key !== 'v') return;
    const t = e.target;
    if (isBlock(t) || (t && t.matches && t.matches('input, textarea, select, [contenteditable="true"]'))) return;
    const cal = document.querySelector('.pane.focused .calendar') || document.querySelector('.calendar');
    if (!cal) return;
    const under = calPointer.x >= 0 ? document.elementFromPoint(calPointer.x, calPointer.y) : null;
    // (Only for the calendar itself: the pointer is over its days, or the
    // keyboard is in it. In a page beside a calendar, or in the notes of
    // the day, these keys are a copy and a paste there: a picture pasted in
    // an empty picture box was taken as "Copy an event first".)
    const inCal = (el) => !!(el && el.closest && el.closest('.calendar') === cal && !el.closest('.day-notes, .page'));
    if (!inCal(under) && !inCal(t)) return;
    if (key === 'c') {
      const ev = (under && under.closest && under.closest('.cal-event[data-event]:not(.draft), .cal-chip[data-event]')) || cal.querySelector('.cal-event.on[data-event], .cal-chip.on[data-event]');
      if (!ev || getSelection().toString()) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      send({ t: 'event_copy', id: ev.dataset.event });
      return;
    }
    const col = under && under.closest && under.closest('.cal-col[data-day]');
    const cell = !col && under && under.closest && under.closest('.cal-day[data-day], .cal-allday-cell[data-day]');
    e.preventDefault();
    e.stopImmediatePropagation();
    if (col) send({ t: 'event_paste', day: col.dataset.day, minute: minuteAt(col, calPointer.y) });
    else send({ t: 'event_paste', day: cell ? cell.dataset.day : '', minute: -1 });
  },
  true,
);

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
    if (!col) return;
    // Where the press is in the day, for a click that makes an event there:
    // measured from the column itself, in its own units. (The app took the
    // place inside the element under the pointer, which is not always the
    // column, and on a zoomed page not in the column's units: the event
    // came at another time than the one that was pressed.)
    // Said for a press on an event too: let go a little under a small
    // event, its click is the column's, and took the minute of a press
    // before it (a drag, a press that closed a panel).
    const quarter = Math.floor(((e.clientY - col.getBoundingClientRect().top) / (48 * zoomOf(col))) * 4) * 15;
    send({ t: 'cal_press', minute: Math.max(0, Math.min(24 * 60 - 15, quarter)) });
    if (e.target.closest('.cal-event')) return;
    evDraw = { col, y: e.clientY, moved: false, ghost: null, from: 0, to: 0 };
  },
  true,
);
// The minute of the day under the pointer in a column of hours, to the quarter hour.
// (On a zoomed page an hour is 48 of the page's own units: the pointer and
// the place of the column are in points of the window.)
const minuteAt = (col, clientY) => Math.max(0, Math.min(24 * 60, Math.round(((clientY - col.getBoundingClientRect().top) / (48 * zoomOf(col))) * 4) * 15));
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
window.__rune_player = () => {
  const frame = document.getElementById('player-screen');
  if (!frame) return;
  const r = frame.getBoundingClientRect();
  send({ t: 'player_place', x: r.left, y: r.top, w: r.width, h: r.height });
};
addEventListener('resize', () => requestAnimationFrame(window.__rune_player));

// The player is moved by its bar and gets another size by a corner. While
// that goes on the page moves the dock and the player follows it; at the end
// Rust is told the place (so far from the two nearest edges of the window)
// and the width, keeps them, and draws the dock there.
(() => {
  const EDGE = 8;
  const LEAST = 260;
  let drag = null;
  const follow = () => window.__rune_player && window.__rune_player();
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
    // (Under Settings, a sheet or a menu the sidebar is not in use: its
    // place alone is no reason to go to another space, which would also
    // put the sheet away.)
    if (e.target instanceof Element && e.target.closest('.settings-view, .modal-backdrop, .backdrop, .popover')) return false;
    const side = document.querySelector('.sidebar');
    if (!side || getComputedStyle(side).visibility === 'hidden' || (side.classList.contains('floating') && document.body.dataset.peek !== '1')) return false;
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

// ----- Back and Forward: the side buttons of a mouse, two fingers sideways ------
// Both step in the pane under the pointer, and else in the pane in front, as
// ⌘[ and ⌘] do (over the sidebar or the row of tabs). Only pages go back:
// under Settings, a sheet or an open menu nothing happens (the step would
// change the page under it, out of sight). `pane_step` in state.rs.
const stepTarget = (target) => {
  if (!(target instanceof Element) || target.closest('.settings-view, .modal-backdrop, .backdrop, .popover')) return false;
  return !document.querySelector('.settings-view, .modal-backdrop');
};
// The side buttons are 3 (Back) and 4 (Forward) in `MouseEvent.button`, as
// WebKit gives them. Their presses go nowhere else: not to a tab (the
// middle button's press closes it), not to a page, a menu or a drag.
(() => {
  const SIDE = { 3: true, 4: false };
  for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'auxclick', 'click', 'contextmenu']) {
    window.addEventListener(
      type,
      (e) => {
        if (!(e.button in SIDE)) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        // (On the release. The press's own `preventDefault` keeps WebKit
        // from sending `mousedown` and `mouseup` at all.)
        if (type !== 'pointerup' || !stepTarget(e.target)) return;
        const pane = e.target.closest('.pane[data-pane]');
        send({ t: 'pane_step', pane: pane ? pane.dataset.pane : null, back: SIDE[e.button] });
      },
      true,
    );
  }
})();
// Two fingers moved sideways over a page: right (the page moves right) goes
// Back, left goes Forward, as in Safari. While the move builds, the page
// leans with the fingers and a small arrow comes in at the edge; a move that
// stops short goes back and does nothing. One step for one move: the rest of
// the move and the glide after it are not a new move (as for the spaces of
// the sidebar, above). Never over the sidebar (its move changes the space),
// over something that scrolls sideways itself (a wide table, a code block,
// a diagram), a drawing, the calendar or the graph; never while a button is
// held (a selection, a drag); a move mostly up or down is a scroll.
(() => {
  const NEED = 90; // how far the fingers go sideways, in all, for the step
  const LEAN = 36; // how far the page leans, at most
  let mode = ''; // '' a new move, 'wait' not clear yet, 'side', 'scroll', 'off', 'done'
  let across = 0;
  let down = 0;
  let last = 0;
  let quiet = 0;
  let rest = 0;
  let prevStep = 0; // the last sideways step of this move
  let falls = 0; // how many steps in a row were smaller than the one before
  let pane = null;
  let sign = null;
  let held = false;
  window.addEventListener('pointerdown', () => (held = true), true);
  window.addEventListener('dragstart', () => (held = true), true);
  for (const type of ['pointerup', 'pointercancel', 'dragend', 'drop', 'blur']) window.addEventListener(type, () => (held = false), true);
  const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  // The sidebar: by what is under the pointer, or by where it is (the
  // sidebar that floats lies over the page while it shows).
  const overSidebar = (e) => {
    if (e.target.closest('.sidebar')) return true;
    const side = document.querySelector('.sidebar');
    if (!side || getComputedStyle(side).visibility === 'hidden' || (side.classList.contains('floating') && document.body.dataset.peek !== '1')) return false;
    const r = side.getBoundingClientRect();
    return e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
  };
  // Something between the pointer and its pane that takes a sideways move.
  const ownMove = (el, stop) => {
    for (; el && el !== stop; el = el.parentElement) {
      if (el.matches('.graph, .calendar, .drawing, iframe, select')) return true;
      // (The row of names that stays at the top of a long table is the table's: see `placeSticks`.)
      const of = stuckOf.get(el);
      if (of && of.scrollWidth > of.clientWidth + 1) return true;
      if (el.scrollWidth > el.clientWidth + 1 && (el.matches('input, textarea') || /auto|scroll/.test(getComputedStyle(el).overflowX))) return true;
    }
    return false;
  };
  let leaning = null; // the pane whose page leans (it is cut at its edges meanwhile)
  const box = (p) => p && p.querySelector(':scope > .scroller');
  const show = (part, back) => {
    // (The arrow is at the window's edge of the pane, not in it: the pane is
    // drawn by the app. It is in `.app` for the colours of the theme.)
    if (!sign || !sign.isConnected) {
      sign = document.createElement('div');
      sign.className = 'swipe-sign';
      sign.innerHTML = '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m14.5 6-6 6 6 6"/></svg>';
      (document.querySelector('.app') || document.body).appendChild(sign);
    }
    const r = pane.getBoundingClientRect();
    sign.style.top = `${Math.round(r.top + r.height / 2 - 16)}px`;
    sign.style.left = `${Math.round(back ? r.left + 14 : r.right - 14 - 32)}px`;
    sign.classList.toggle('forward', !back);
    sign.classList.toggle('ready', part >= 1);
    sign.style.transition = 'none';
    sign.style.opacity = Math.min(1, part * 1.4).toFixed(2);
    if (still()) return;
    sign.style.transform = `translateX(${((back ? -1 : 1) * (1 - part) * 12).toFixed(1)}px)`;
    const el = box(pane);
    if (el) {
      leaning = pane;
      pane.style.clipPath = 'inset(0)';
      el.style.transition = 'none';
      el.style.transform = `translateX(${((back ? 1 : -1) * part * LEAN).toFixed(1)}px)`;
    }
  };
  // Back to rest: the page and the arrow (after a step, the new page comes
  // in from where the old one leaned).
  const settle = () => {
    const p = leaning;
    leaning = null;
    const el = box(p);
    if (el && el.style.transform) {
      el.style.transition = 'transform 180ms cubic-bezier(0.23, 1, 0.32, 1)';
      el.style.transform = '';
      setTimeout(() => {
        if (leaning === p) return;
        el.style.transition = '';
        p.style.clipPath = '';
      }, 200);
    } else if (p) p.style.clipPath = '';
    const s = sign;
    sign = null;
    if (s) {
      s.style.transition = 'opacity 150ms ease-out';
      s.style.opacity = '0';
      setTimeout(() => s.remove(), 160);
    }
  };
  document.addEventListener(
    'wheel',
    (e) => {
      // (The time the trackpad made the step, not the time the page got to it.)
      const now = e.timeStamp || performance.now();
      const gap = now - last;
      last = now;
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? innerWidth : 1;
      const dx = e.deltaX * unit;
      const dy = e.deltaY * unit;
      const step = Math.abs(dx);
      if (gap > 250) {
        // A pause: the next move.
        if (mode === 'side') settle();
        mode = '';
      } else if (mode === 'done') {
        // The rest of the move that stepped, and its glide. A large step
        // after two or more small ones is the next move (the glide only
        // gets weaker).
        const again = step >= 8 && quiet >= 2;
        quiet = step <= 4 ? quiet + 1 : 0;
        if (!again) return;
        mode = '';
      }
      if (mode === 'off' || mode === 'scroll') return;
      if (mode === '') {
        const t = e.target instanceof Element ? e.target : null;
        pane = t && t.closest('.pane[data-pane]');
        const ok = pane && !e.ctrlKey && !e.shiftKey && !e.metaKey && !e.altKey && !held && !e.buttons && stepTarget(t) && !overSidebar(e) && !ownMove(t, pane);
        mode = ok ? 'wait' : 'off';
        across = 0;
        down = 0;
        prevStep = 0;
        falls = 0;
        if (!ok) return;
      }
      across += dx;
      down += dy;
      falls = step < prevStep ? falls + 1 : 0;
      prevStep = step;
      if (mode === 'wait') {
        if (Math.abs(across) + Math.abs(down) < 6) return;
        // Mostly up or down: a scroll, to its end.
        if (Math.abs(across) <= Math.abs(down) * 2) {
          mode = 'scroll';
          return;
        }
        mode = 'side';
      }
      if (!pane.isConnected) {
        mode = 'off';
        return;
      }
      // The glide after the fingers left goes on with no pause, in steps
      // that only get smaller, and the page cannot tell it from the fingers
      // otherwise (WebKit gives a wheel event no phase). So two smaller steps
      // in a row end the move: what it made by then is all it makes, and a
      // quick flick that its glide would carry far does not step (a missed
      // swipe is better than a Back that was not meant). The rest of the
      // glide is let go as after a step.
      if (falls >= 2) {
        clearTimeout(rest);
        mode = 'done';
        quiet = 0;
        settle();
        return;
      }
      const back = across < 0;
      clearTimeout(rest);
      // Nothing that way: nothing shows.
      if (pane.dataset[back ? 'back' : 'forward'] !== 'true') {
        settle();
        return;
      }
      const part = Math.min(1, Math.abs(across) / NEED);
      show(part, back);
      if (part < 1) {
        // The fingers stopped short: the page goes back.
        rest = setTimeout(() => {
          if (mode === 'side') mode = '';
          settle();
        }, 170);
        return;
      }
      mode = 'done';
      quiet = 0;
      send({ t: 'pane_step', pane: pane.dataset.pane, back });
      requestAnimationFrame(() => requestAnimationFrame(settle));
    },
    { passive: true, capture: true },
  );
})();

// ----- diagrams: code blocks of the language Mermaid ---------------------------
//
// The block shows `.diagram[data-diagram]` with the code; this draws it with
// Mermaid (built into the app, see assets/mermaid), in the colours of the app.
// The library loads the first time a page shows a diagram.

const DIAGRAM_LIB = 'runedraw://app/mermaid.min.js';
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
  const name = `rune-diagram-${++diagramCount}`;
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

// ----- the wheel over an open menu's shade ---------------------------------------------
// While a menu is open, a see-through shade over the window takes the clicks
// outside it (one closes the menu). The wheel goes through it: the menu closes
// and what is under the pointer scrolls, as with no menu.
const scrollerAt = (x, y, shade) => {
  shade.style.pointerEvents = 'none';
  let el = document.elementFromPoint(x, y);
  shade.style.pointerEvents = '';
  for (; el && el !== document.documentElement; el = el.parentElement) {
    const cs = getComputedStyle(el);
    const can = (o, room) => (o === 'auto' || o === 'scroll') && room > 1;
    if (can(cs.overflowY, el.scrollHeight - el.clientHeight) || can(cs.overflowX, el.scrollWidth - el.clientWidth)) return el;
  }
  return null;
};
document.addEventListener(
  'wheel',
  (e) => {
    const shade = e.target && e.target.classList && e.target.classList.contains('backdrop') ? e.target : null;
    if (!shade) return;
    const box = scrollerAt(e.clientX, e.clientY, shade);
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? innerHeight : 1;
    if (box) box.scrollBy({ left: e.deltaX * unit, top: e.deltaY * unit });
    // (Once: the first turn of the wheel closes the menu.)
    if (!shade.dataset.closing) {
      shade.dataset.closing = '1';
      send({ t: 'shortcut', name: 'escape' });
    }
  },
  { capture: true, passive: true },
);

window.__rune_test = { selIn, setSel, command: (name) => send({ t: 'shortcut', name }) };
// (For the check of the calendar's kept hours: make them this much older.)
window.__rune_test.ageHours = (ms) => hoursSet.forEach((set) => (set[0] -= ms));

// Keep this script alive so dioxus.send() stays connected.
await new Promise(() => {});
