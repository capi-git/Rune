// The page script of the meeting pad (src/ui/pad.rs). It runs once inside a
// long-lived eval of the pad's window (in a test run: of the first window,
// which then draws the pad itself). It owns the notepad (a list that is
// typed in), says how big the pad is so that its window can fit it, and says
// when the pad's head is pressed so that the window can be dragged by it.
//
// To Rust, with dioxus.send():
//   { t: 'lines', lines: [...] }   the notepad's lines, a moment after a key
//   { t: 'size', w, h, r }         the size of the pad and its corner radius
//   { t: 'drag' }                  the head was pressed
//   { t: 'keys' }                  the notepad was pressed: it needs the keyboard

const say = (message) => {
  try {
    dioxus.send(message);
  } catch (err) {
    console.error('pad send failed', err);
  }
};

const notes = () => document.getElementById('pad-notes');
const card = () => document.getElementById('pad-card');
const linesOf = (list) =>
  [...list.querySelectorAll('li')].map((li) => li.textContent.replace(/ /g, ' ').trim()).filter(Boolean);

// ----- the notepad ------------------------------------------------------------

// Always one line to type in; the placeholder shows while that line is empty.
function tidy(list) {
  if (!list.querySelector('li')) {
    list.textContent = '';
    const li = document.createElement('li');
    li.appendChild(document.createElement('br'));
    list.appendChild(li);
  }
  list.classList.toggle('is-empty', list.querySelectorAll('li').length === 1 && list.textContent.trim() === '');
}

let sent = null;
let timer = 0;

// The lines Rust gave the list as it was drawn (they were typed before the
// pad was made small): put them in, once.
function fill(list) {
  let lines = [];
  try {
    lines = JSON.parse(list.dataset.lines || '[]');
  } catch (err) {
    lines = [];
  }
  list.textContent = '';
  for (const line of lines.length ? lines : ['']) {
    const li = document.createElement('li');
    if (line) li.textContent = line;
    else li.appendChild(document.createElement('br'));
    list.appendChild(li);
  }
  list.dataset.ready = '1';
  sent = JSON.stringify(linesOf(list));
  tidy(list);
}

function report(now) {
  clearTimeout(timer);
  const go = () => {
    const list = notes();
    if (!list || !list.dataset.ready) return;
    const lines = linesOf(list);
    const key = JSON.stringify(lines);
    if (key === sent) return;
    sent = key;
    say({ t: 'lines', lines });
  };
  if (now) go();
  else timer = setTimeout(go, 350);
}

const inNotes = (node) => {
  const el = node && (node.nodeType === 1 ? node : node.parentElement);
  return el && el.closest ? el.closest('#pad-notes') : null;
};

document.addEventListener('input', (e) => {
  const list = inNotes(e.target);
  if (!list) return;
  tidy(list);
  report(false);
});

// Only plain words go in, on one line.
document.addEventListener(
  'paste',
  (e) => {
    if (!inNotes(e.target)) return;
    e.preventDefault();
    e.stopPropagation();
    const text = ((e.clipboardData && e.clipboardData.getData('text/plain')) || '').replace(/\s*\n\s*/g, ' ');
    document.execCommand('insertText', false, text);
  },
  true,
);

document.addEventListener(
  'keydown',
  (e) => {
    const list = inNotes(e.target);
    if (!list) return;
    // The notepad's keys are its own (in a test run the first window's
    // editor listens too).
    e.stopPropagation();
    if (e.key !== 'Enter') return;
    const sel = window.getSelection();
    const at = sel && sel.anchorNode ? (sel.anchorNode.nodeType === 1 ? sel.anchorNode : sel.anchorNode.parentElement) : null;
    const li = at && at.closest ? at.closest('li') : null;
    // An empty line stays one empty line.
    if (!li || li.textContent.trim() === '') e.preventDefault();
  },
  true,
);

// The pad's window has the keyboard only for the notepad: a press in the
// notepad asks for it.
document.addEventListener(
  'mousedown',
  (e) => {
    if (inNotes(e.target)) say({ t: 'keys' });
  },
  true,
);

// What is typed is sent at once when the notepad is left or a button is pressed.
document.addEventListener('focusout', (e) => {
  if (inNotes(e.target)) report(true);
});
document.addEventListener(
  'pointerdown',
  (e) => {
    if (!inNotes(e.target)) report(true);
  },
  true,
);

// ----- the window: its size, and a drag by the head -----------------------------

// The head of the pad drags the window at once. The small form is one
// button: a press that moves is a drag, a press that does not is a click.
let pressed = null;
document.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return;
  const el = e.target.nodeType === 1 ? e.target : e.target.parentElement;
  const handle = el && el.closest('[data-drag]');
  if (!handle) return;
  if (handle.dataset.drag === 'move') {
    pressed = { x: e.screenX, y: e.screenY };
    return;
  }
  if (el.closest('button, select, input, [contenteditable]')) return;
  say({ t: 'drag' });
});
document.addEventListener('mousemove', (e) => {
  if (!pressed) return;
  if (!(e.buttons & 1)) pressed = null;
  else if (Math.abs(e.screenX - pressed.x) + Math.abs(e.screenY - pressed.y) > 4) {
    pressed = null;
    say({ t: 'drag' });
  }
});
document.addEventListener('mouseup', () => {
  pressed = null;
});

let measured = null;
let lastSize = '';
const sizes = new ResizeObserver(() => size());

function size() {
  const el = card();
  if (!el) return;
  const box = el.getBoundingClientRect();
  const w = Math.ceil(box.width);
  const h = Math.ceil(box.height);
  if (!w || !h) return;
  const r = parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0;
  const key = w + 'x' + h + 'r' + r;
  if (key === lastSize) return;
  lastSize = key;
  say({ t: 'size', w, h, r });
}

// The pad is drawn again when its form changes: find the new parts then.
function look() {
  const list = notes();
  if (list && !list.dataset.ready) fill(list);
  const el = card();
  if (el !== measured) {
    if (measured) sizes.unobserve(measured);
    measured = el;
    if (el) sizes.observe(el);
  }
  size();
}

new MutationObserver(look).observe(document.documentElement, { childList: true, subtree: true });
look();

// For the self-test: what the notepad holds, and typing into it.
window.__n2pad = {
  lines: () => (notes() ? linesOf(notes()) : []),
  type: (lines) => {
    const list = notes();
    if (!list) return false;
    list.dataset.lines = JSON.stringify(lines);
    delete list.dataset.ready;
    fill(list);
    sent = null;
    report(true);
    return true;
  },
};

// Keep this script alive so dioxus.send() stays connected.
await new Promise(() => {});
