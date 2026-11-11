// Developer self-test, run inside the real app window when NOTION2_SELFTEST is
// set (always with a throwaway workspace via NOTION2_DB). It clicks and types
// through the interface like a person would and reports what worked.

const results = [];
// Saved copies of the screen at a few moments, for checking the look.
const snaps = {};
const snap = (name) => {
  // What is typed in a box is not in the text of the page: a copy of the
  // page gets it written in, so that a picture of the copy shows it.
  const copy = document.documentElement.cloneNode(true);
  const live = document.querySelectorAll('textarea, input');
  copy.querySelectorAll('textarea, input').forEach((box, i) => {
    const from = live[i];
    if (!from) return;
    if (box.tagName === 'TEXTAREA') box.textContent = from.value;
    else if (from.type === 'checkbox' || from.type === 'radio') box.toggleAttribute('checked', from.checked);
    else box.setAttribute('value', from.value);
  });
  // Each picture goes to the report by itself: all of them with the results
  // is too much for one message, and the results would be lost with it.
  // (Half a pair of code units, which a report cannot carry, becomes a mark.)
  const html = copy.outerHTML;
  snaps[name] = html.length;
  dioxus.send({ snap: name, html: html.toWellFormed ? html.toWellFormed() : html });
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const until = async (fn, ms = 2500, step = 30) => {
  const t0 = performance.now();
  while (performance.now() - t0 < ms) {
    const v = fn();
    if (v) return v;
    await wait(step);
  }
  return null;
};
const check = (name, ok, detail = '') => results.push({ name, ok: !!ok, detail: String(detail ?? '') });
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const T = () => window.__n2_test;
// New Page from Speech (⇧⌘D, in the File menu).
const speakPage = () => T().command('voice');
const key = (el, k, opts = {}) => el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...opts }));
// Is the element really the thing under the pointer at its centre (not covered)?
const onTop = (el) => {
  const r = el.getBoundingClientRect();
  const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
  return !!hit && (hit === el || el.contains(hit));
};
const menuItem = (menu, text) => $$('.menu-item', menu).find((m) => m.textContent.includes(text));
// The menu of the bar that asks where to keep a page: Rune's own, with every space.
const placeNow = (within) => $('#meeting-place .pad-pick-name', within)?.textContent.trim();
const placesOpen = async (within) => {
  if (!$('.place-menu', within)) $('#meeting-place', within).click();
  return until(() => $('.place-menu', within));
};
const placesShut = async (within) => {
  $('.place-shade', within)?.click();
  await until(() => !$('.place-menu', within), 1500);
};
const placeRowName = (row) => row.querySelector('.place-name').textContent.trim();
const placeNames = async (within) => {
  const menu = await placesOpen(within);
  const names = menu ? $$('.place-row', menu).map(placeRowName) : [];
  await placesShut(within);
  return names;
};
const choosePlace = async (within, name) => {
  const menu = await placesOpen(within);
  $$('.place-row', menu).find((r) => placeRowName(r) === name).click();
  await until(() => !$('.place-menu', within), 1500);
};
// (Not the picture of the last space's list, which slides out for a moment when the space changes.)
const treeItem = (title) => $$('.sidebar .tree-item').find((t) => !t.closest('.side-ghost') && t.querySelector('.tree-title')?.textContent === title);
const typeIn = (el, text) => {
  el.focus();
  for (const ch of text) document.execCommand('insertText', false, ch);
};
// Whether the window was in view at the start and end of each check (macOS
// pauses drawing in a window out of view, which stalls the checks).
const views = {};
// The real pointer is no part of a test: resting over the test window, it
// sends hover events of its own when the page changes under it (found when
// one closed the floating sidebar in the middle of its check).
for (const type of ['mouseover', 'mouseout', 'mousemove']) window.addEventListener(type, (e) => e.isTrusted && e.stopImmediatePropagation(), true);
// NOTION2_SELFTEST_ONLY: run only the checks whose name holds one of these.
const ONLY = String(window.__n2_only || '').split(',').map((o) => o.trim()).filter(Boolean);
const run = async (name, fn) => {
  if (ONLY.length && !ONLY.some((o) => name.includes(o))) return;
  dioxus.send({ progress: `${name} [${document.visibilityState}${document.hasFocus() ? ', focused' : ''}]` });
  views[name] = document.visibilityState;
  try {
    await fn();
    if (document.visibilityState !== views[name]) views[name] += ' → ' + document.visibilityState;
  } catch (e) {
    check(name + ' (threw)', false, `${(e && e.message) || e} | active: ${document.activeElement?.className || document.activeElement?.tagName} | crumb: ${document.querySelector('.pagebar .crumb.current')?.textContent}`);
  }
};

// Settings lives in the space menu at the top of the sidebar.
const openSettings = async () => {
  $('#side-settings').click();
  await until(() => $('.settings-view'));
};
// The AI: the small round button at the foot of the pane in front.
const askAi = () => ($('.pane.focused .ai-dock-btn') || $$('.ai-dock-btn').find((b) => b.offsetParent)).click();
// Open one section of Settings by its name in the list on the left.
const settingsSection = async (name) => {
  $(`.settings-nav-item[data-section="${name}"]`).click();
  return until(() => $('.settings-page-header h1')?.textContent === name);
};

// Drag one tab onto another: 'group' (middle) or 'before'/'after' (edges).
const dragTab = async (from, to, where) => {
  const r = to.getBoundingClientRect();
  const x = where === 'before' ? r.left + 3 : where === 'after' ? r.right - 3 : r.left + r.width / 2;
  const y = r.top + r.height / 2;
  const dt = new DataTransfer();
  from.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
  to.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
  to.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
  from.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
  await wait(150);
};

// Pick something up with the mouse and let go somewhere else. `over` is a
// list of [element, x, y] points the pointer passes over; it is let go at the last.
const dragAlong = async (from, over) => {
  const dt = new DataTransfer();
  from.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
  for (const [el, x, y] of over) el.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
  const [el, x, y] = over[over.length - 1];
  el.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
  from.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
  await wait(150);
};
const centre = (el, fx = 0.5) => {
  const r = el.getBoundingClientRect();
  return [el, r.left + r.width * fx, r.top + r.height / 2];
};
// Down to a single tab showing Home.
const oneTab = async () => {
  while ($$('.tab').length > 1) {
    const n = $$('.tab').length;
    $$('.tab')[n - 1].querySelector('.tab-close').click();
    await until(() => $$('.tab').length < n);
  }
  $$('.tile').find((t) => t.title === 'Home').click();
  await until(() => $('.home-hero'));
};

const newPage = async (title) => {
  const before = document.activeElement;
  $('.ws-row .row-btn').click();
  const fresh = () => $$('.title-input').find((x) => x !== before && x.value === '' && x.offsetParent);
  let t = await until(() => {
    const a = document.activeElement;
    return a?.classList.contains('title-input') && a !== before && a.value === '' && a.offsetParent && a;
  }, 1500);
  if (!t) {
    t = await until(fresh, 1500);
    if (t) t.focus();
  }
  if (!t) throw new Error(`new page title did not appear (${title}; tabs ${$$('.tab').length})`);
  t.value = title;
  t.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => treeItem(title));
  return t;
};

// A table: one of the commands of the menu of a column, and of a row (the
// handle of a row comes to the row under the pointer).
const columnDo = async (table, c, item) => {
  $$('.th-menu', table)[c].click();
  const row = await until(() => $(`.column-menu [data-item="${item}"]`, table));
  if (!row) throw new Error(`the menu of column ${c} has no "${item}"`);
  row.click();
};
const rowHandle = async (table, r) => {
  $(`tr[data-r="${r}"]`, table).dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
  const handle = await until(() => $(`.row-handle.on[data-r="${r}"]`, table.parentElement));
  if (!handle) throw new Error(`no handle came to row ${r}`);
  return handle;
};
const rowDo = async (table, r, item) => {
  $('[data-row-op="row-menu"]', await rowHandle(table, r)).click();
  const row = await until(() => $(`.row-menu [data-item="${item}"]`, table));
  if (!row) throw new Error(`the menu of row ${r} has no "${item}"`);
  row.click();
};
// Type in a cell and leave it, as a person does.
const fillCell = (cell, text) => {
  cell.focus();
  cell.value = text;
  cell.dispatchEvent(new Event('input', { bubbles: true }));
  cell.dispatchEvent(new Event('change', { bubbles: true }));
};

await wait(600);

await run('Text encoding is UTF-8', async () => {
  check('Text encoding is UTF-8', document.characterSet === 'UTF-8', document.characterSet);
});

await run('Welcome page shows formatting', async () => {
  const strong = await until(() => $('.blk strong'));
  check('Welcome page shows formatting', strong && $('.blk em') && $('.blk code') && $('.blk a[href]'));
});

await run('Glass colours come from settings', async () => {
  const style = $('.app').getAttribute('style') || '';
  check('Glass colours come from settings', style.includes('--frame-d') && style.includes('--card-l'), style.slice(0, 160));
});

await run('Sidebar menu: Move to Trash', async () => {
  await newPage('Trash me');
  const row = treeItem('Trash me');
  row.querySelector('.tree-actions .row-btn').click();
  const menu = await until(() => $('.sidebar-menu'));
  check('Sidebar menu opens', menu);
  check('Sidebar menu is lifted out of the scroll area', menu?.classList.contains('anchored'));
  const item = menuItem(menu, 'Move to Trash');
  check('Sidebar menu item is clickable (not covered)', item && onTop(item), item && document.elementFromPoint(item.getBoundingClientRect().left + 20, item.getBoundingClientRect().top + 8)?.className);
  item.click();
  const gone = await until(() => !treeItem('Trash me'));
  check('Sidebar "Move to Trash" trashes the page', gone);
  check('Toast says it moved', (await until(() => $('.toast')))?.textContent.includes('Trash'), $('.toast')?.textContent);
  check('The "Moved to Trash" message goes away on its own', await until(() => !$('.toast'), 2500));
});

await run('Page menu: Move to Trash', async () => {
  await newPage('Trash me too');
  $('.pagebar .icon-btn[title="Page options"]').click();
  const menu = await until(() => $('.page-menu'));
  const item = menu && menuItem(menu, 'Move to Trash');
  check('Page menu item is clickable (not covered)', item && onTop(item));
  item.click();
  check('Page menu "Move to Trash" trashes the page', await until(() => !treeItem('Trash me too')));
});

let blk = null;
await run('Typing, formatting, Enter, shortcuts', async () => {
  const title = await newPage('Typing test');
  key(title, 'Enter');
  blk = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  check('Enter in the title moves to the first block', blk);
  typeIn(blk, 'hello world');
  await wait(150);
  T().setSel(blk, 6, 11);
  key(blk, 'b', { metaKey: true });
  await wait(100);
  check('⌘B makes the selection bold', blk.innerHTML.includes('<strong>world</strong>'), blk.innerHTML);
  T().setSel(blk, 5);
  typeIn(blk, ' **big**');
  await wait(120);
  check('Typing **big** formats it', blk.querySelectorAll('strong').length === 2, blk.innerHTML);
  T().setSel(blk, T().selIn(blk) ? blk.textContent.replace(/​/g, '').length : 0);
  key(blk, 'Enter');
  const second = await until(() => {
    const a = document.activeElement;
    return a?.classList.contains('blk') && a !== blk && a;
  });
  check('Enter makes a new block and moves the caret', second);
  typeIn(second, '- ');
  const bullet = await until(() => $('.block-row.kind-bulleted'));
  check('"- " turns the block into a bullet', bullet);
  const li = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(li, 'item');
  await wait(150);
  key(li, 'z', { metaKey: true });
  check('⌘Z undoes the bullet and its text', await until(() => !$('.block-row.kind-bulleted')));
  key(document.activeElement || document.body, 'z', { metaKey: true, shiftKey: true });
  check('⌘⇧Z redoes it', await until(() => $('.block-row.kind-bulleted')?.textContent.includes('item')));
});

await run('Slash menu', async () => {
  const last = $$('.blk').pop();
  last.focus();
  T().setSel(last, last.textContent.length);
  key(last, 'Enter');
  const fresh = await until(() => {
    const a = document.activeElement;
    return a?.classList.contains('blk') && a !== last && a;
  });
  typeIn(fresh, '/');
  check('Typing / opens the menu', await until(() => $('.slash-menu')));
  key(fresh, 'Escape');
  check('Escape closes it', await until(() => !$('.slash-menu')));
});

await run('Block menu', async () => {
  const row = $('.block-row');
  row.querySelector('.handle').click();
  const menu = await until(() => $('.block-menu'));
  const item = menu && menuItem(menu, 'Duplicate');
  check('Block menu item is clickable (not covered)', item && onTop(item));
  key(document.body, 'Escape');
  await until(() => !$('.block-menu'));
  // The menu of a line at the foot of the window opens where all of it is seen.
  const rows = $$('.pane .blocks > .block-row').filter((r) => r.querySelector('.handle'));
  const low = rows[rows.length - 1];
  if (low) {
    low.scrollIntoView({ block: 'end' });
    await wait(200);
    low.querySelector('.handle').click();
    const m = await until(() => $('.block-menu.anchored'), 3000);
    await wait(200);
    const r = m && m.getBoundingClientRect();
    const items = m ? $$('.menu-item', m) : [];
    check('The menu of a line at the foot of the window is all in the window, with no scroll', r && r.top >= 0 && r.bottom <= innerHeight && items.length && onTop(items[items.length - 1]), r ? `${Math.round(r.top)}..${Math.round(r.bottom)} of ${innerHeight}, line at ${Math.round(low.getBoundingClientRect().top)}, over the last item: ${(() => { const b = items[items.length - 1].getBoundingClientRect(); const e = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2); const chain = []; for (let n = e; n && n !== document.body && chain.length < 6; n = n.parentElement) chain.push(n.className && n.className.baseVal === undefined ? n.className : n.tagName); return chain.join(' < '); })()}` : 'no menu');
    key(document.body, 'Escape');
    await until(() => !$('.block-menu'));
  }
});

await run('Icon picker', async () => {
  const add = $$('.page-controls .ghost-btn').find((b) => b.textContent.includes('Add icon'));
  add.click();
  const picker = await until(() => $('.icon-picker'));
  const cell = picker && $$('.ip-cell', picker)[3];
  check('Icon cell is clickable (not covered)', cell && onTop(cell));
  cell.click();
  check('Choosing an icon shows it on the page', await until(() => $('.page-icon .page-ico')));
  const swatch = $$('.ip-swatch', picker)[5];
  swatch?.click();
  await wait(100);
  key(document.body, 'Escape');
});

await run('Tabs and the sliding highlight', async () => {
  await oneTab();
  // One Home tab at most: a new tab from Home brings that tab forward.
  $('.new-tab').click();
  await wait(300);
  check('With a Home tab open, a new tab opens no second Home tab', $$('.tab').length === 1 && $$('.tab .tab-title').filter((t) => t.textContent === 'Home').length === 1, $$('.tab .tab-title').map((t) => t.textContent).join(' | '));
  treeItem('Getting Started').click();
  await until(() => $$('.tab').length === 2);
  $('.new-tab').click();
  check('From another tab, a new tab goes to the Home tab there is', await until(() => $$('.tab').length === 2 && $('.tab.active .tab-title')?.textContent === 'Home'), $$('.tab .tab-title').map((t) => t.textContent).join(' | '));
  $$('.tab')[1].click();
  await until(() => $('.tab.active .tab-title')?.textContent === 'Getting Started');
  $$('.tile').find((t) => t.title === 'Home').click();
  check('Home in the sidebar goes to the Home tab, and the page keeps its tab', await until(() => $('.tab.active .tab-title')?.textContent === 'Home' && $$('.tab').length === 2 && $$('.tab .tab-title').some((t) => t.textContent === 'Getting Started')), $$('.tab .tab-title').map((t) => t.textContent).join(' | '));
  await wait(300);
  const g = $('.tab-glider');
  const active = $('.tab.active');
  const x = parseFloat((g.style.transform.match(/translateX\(([-\d.]+)px/) || [])[1]);
  check('Highlight sits under the active tab', Math.abs(x - active.offsetLeft) < 1.5 && Math.abs(g.offsetWidth - active.offsetWidth) < 1.5, `${x} vs ${active.offsetLeft}`);
  $$('.tab')[0].click();
  await wait(300);
  const x2 = parseFloat((g.style.transform.match(/translateX\(([-\d.]+)px/) || [])[1]);
  check('Highlight slides to the clicked tab', Math.abs(x2 - $$('.tab')[0].offsetLeft) < 1.5, x2);
  $$('.tab')[1].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  const menu = await until(() => $('.tab-menu'));
  const item = menu && menuItem(menu, 'Close Tab');
  check('Tab menu is lifted out and clickable', menu?.classList.contains('anchored') && item && onTop(item));
  item.click();
  check('Close Tab from the menu works', await until(() => $$('.tab').length === 1));
});

await run('Pages open in their own tab', async () => {
  await oneTab();
  treeItem('Getting Started').click();
  check('Clicking a page opens it in a tab of its own', await until(() => $$('.tab').length === 2 && $('.tab.active')?.textContent.includes('Getting Started')));
  check('The new tab glows for a moment', await until(() => $('.tab.landed'), 1000));
  $$('.tab')[0].click();
  await until(() => $('.home-hero'));
  treeItem('Getting Started').click();
  check('Clicking an open page brings its tab forward', (await until(() => $$('.tab')[1]?.classList.contains('active'))) && $$('.tab').length === 2);
  $$('.tab')[0].querySelector('.tab-close').click();
  await until(() => $$('.tab').length === 1);
});

await run('Pages side by side', async () => {
  treeItem('Reading List').click();
  await until(() => $$('.tab').length === 2 && $$('.tab')[1]?.textContent.includes('Reading List'));
  let tabs = $$('.tab');
  await dragTab(tabs[1], tabs[0], 'middle');
  check('Dropping a tab on another shows both pages side by side', await until(() => $$('.tab').length === 1 && $('.panes.split') && $$('.pane .page').length === 2));
  check('The tab names both pages', $$('.tab.split .tab-seg').length === 2 && $('.tab.split').textContent.includes('Getting Started') && $('.tab.split').textContent.includes('Reading List'));
  await wait(250);
  snap('side-by-side');
  $$('.tab.split .tab-seg')[0].click();
  check('Clicking a part of the tab focuses that page', await until(() => $$('.pane')[0]?.classList.contains('focused')));
  $('.tab').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  const tm = await until(() => $('.tab-menu'));
  check('Tab menu is short', tm && $$('.menu-item', tm).length <= 4, tm && $$('.menu-item', tm).map((m) => m.textContent).join(' | '));
  menuItem(tm, 'Separate Pages').click();
  check('Separate Pages makes two tabs again', await until(() => $$('.tab').length === 2 && !$('.panes.split')));
  tabs = $$('.tab');
  const firstId = tabs[0].dataset.tab;
  await dragTab(tabs[1], tabs[0], 'before');
  check('Dropping a tab on an edge reorders it', await until(() => $$('.tab')[1]?.dataset.tab === firstId));
  $$('.tab')[1].click();
  await until(() => $('.pane .title-input')?.value === 'Getting Started');
  // Drag a page from the sidebar onto the right half of the page area.
  const card = $('.work-card').getBoundingClientRect();
  const x = card.left + card.width * 0.75;
  const y = card.top + card.height / 2;
  const target = document.elementFromPoint(x, y);
  const dt = new DataTransfer();
  const source = treeItem('Reading List');
  source.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
  target.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
  check('Dragging over a pane shows the half it will take', $('.pane-drop.show'));
  target.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
  source.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
  check('Dropping a sidebar page there opens it beside', await until(() => $('.panes.split') && $$('.pane')[1]?.querySelector('.title-input')?.value === 'Reading List'));
  $$('.pane')[1].querySelector('.pagebar .icon-btn[title="Close pane"]').click();
  await until(() => !$('.panes.split'));
});

await run('Pull a page out of a combined tab', async () => {
  await oneTab();
  treeItem('Getting Started').click();
  await until(() => $$('.tab').length === 2);
  $$('.tab')[0].querySelector('.tab-close').click();
  await until(() => $$('.tab').length === 1);
  treeItem('Reading List').click();
  await until(() => $$('.tab').length === 2);
  await dragTab($$('.tab')[1], $$('.tab')[0], 'middle');
  await until(() => $('.tab.split'));
  const merge = async () => {
    await dragTab($$('.tab')[1], $$('.tab')[0], 'middle');
    await until(() => $('.tab.split'));
  };
  // Right part dragged sideways along the bar to its end: a tab of its own there.
  let seg = $$('.tab.split .tab-seg')[1];
  const dt = new DataTransfer();
  seg.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
  const [own, ox, oy] = centre($('.tab.split'), 0.6);
  own.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: ox, clientY: oy, dataTransfer: dt }));
  check('Over its own tab, a dragged page shows no drop marker', document.body.dataset.dragging === 'seg' && !$('.tab-drop-before, .tab-drop-after, .tab-drop-split, .tabs.tab-drop-end'));
  const [nt, nx, ny] = centre($('.new-tab'));
  nt.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: nx, clientY: ny, dataTransfer: dt }));
  check('Further along the bar it shows where it will land', $('.tabs.tab-drop-end'));
  nt.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, clientX: nx, clientY: ny, dataTransfer: dt }));
  seg.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
  check('Dragging a part sideways in the top bar gives that page its own tab', await until(() => $$('.tab').length === 2 && !$('.tab.split') && !$('.panes.split') && $$('.tab')[1].textContent.includes('Reading List')));
  check('The tab that moved glows', await until(() => $$('.tab')[1]?.classList.contains('landed'), 1000));
  // Let go on its own tab: nothing changes.
  await merge();
  seg = $$('.tab.split .tab-seg')[1];
  await dragAlong(seg, [centre($('.tab.split'), 0.5)]);
  check('Letting go on its own tab changes nothing', $$('.tab').length === 1 && $$('.tab.split .tab-seg').length === 2);
  // Left part dragged left, past the tabs: a tab of its own before them.
  seg = $$('.tab.split .tab-seg')[0];
  await dragAlong(seg, [centre($('.toprow .nav-group'))]);
  check('Dragged left past the tabs, it gets a tab first in line', await until(() => $$('.tab').length === 2 && !$('.tab.split') && $$('.tab')[0].textContent.includes('Getting Started') && $$('.tab')[1].textContent.includes('Reading List')));
  // Onto the middle of another tab: it joins that tab.
  await merge();
  $('.new-tab').click();
  await until(() => $$('.tab').length === 2);
  seg = $$('.tab.split .tab-seg')[1];
  await dragAlong(seg, [centre($$('.tab')[1], 0.5)]);
  check('Dropped on the middle of another tab, it joins that tab', await until(() => $$('.tab').length === 2 && !$$('.tab')[0].classList.contains('split') && $$('.tab')[1].classList.contains('split') && $$('.tab')[1].textContent.includes('Reading List')));
  // Let go off the bar and the pages (on the sidebar): a tab of its own next to it.
  seg = $$('.tab.split .tab-seg').find((x) => x.textContent.includes('Reading List'));
  await dragAlong(seg, [centre($('.sidebar .tiles'))]);
  check('Let go on the sidebar, it gets a tab of its own', await until(() => $$('.tab').length === 3 && !$('.tab.split') && $$('.tab')[2].textContent.includes('Reading List')));
  $$('.tab')[2].querySelector('.tab-close').click();
  await until(() => $$('.tab').length === 2);
  // Sidebar page onto the bar: at the end it's a new tab; on the middle of a tab it pairs.
  await dragAlong(treeItem('Typing test'), [centre($('.new-tab'))]);
  check('A sidebar page dropped on the bar opens a tab there', await until(() => $$('.tab').length === 3 && $$('.tab')[2].textContent.includes('Typing test')));
  await dragAlong(treeItem('Getting Started'), [centre($$('.tab')[2])]);
  check('A sidebar page dropped on a tab pairs with it', await until(() => $$('.tab').length === 3 && $$('.tab')[2].classList.contains('split') && $('.panes.split')));
  await wait(250);
  snap('pulled-apart');
  await oneTab();
});

await run('Sidebar unpin and peek', async () => {
  $('.nav-group .bar-btn').click();
  const edge = await until(() => $('.edge-strip'));
  check('Unpinning shows the edge strip', edge && !$('.app').classList.contains('pinned'));
  // The card tucks away if the window loses focus (someone clicking
  // elsewhere while the test runs), so record that it appeared at all.
  let appeared = false;
  const watch = new MutationObserver(() => {
    if (document.body.dataset.peek === '1') appeared = true;
  });
  watch.observe(document.body, { attributes: true, attributeFilter: ['data-peek'] });
  edge.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  const shown = await until(() => appeared, 1500);
  watch.disconnect();
  check('Hovering the edge slides the sidebar out', shown);
  $('.work-card').dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  check('Leaving tucks it away', await until(() => document.body.dataset.peek !== '1', 1500));
  $('.edge-strip').click();
  check('Clicking the edge pins it again', await until(() => $('.app').classList.contains('pinned')));
});

await run('Settings: transparency', async () => {
  await openSettings();
  const slider = await until(() => $('.settings-view .slider'));
  const before = $('.app').getAttribute('style');
  slider.value = '80';
  slider.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(120);
  check('Moving the slider changes the glass live', $('.app').getAttribute('style') !== before);
  slider.dispatchEvent(new Event('change', { bubbles: true }));
  const tint = $$('.settings-view .tint')[2];
  tint.click();
  await wait(100);
  check('Picking a tint marks it chosen', tint.classList.contains('on') || $$('.settings-view .tint.on').length === 1);
  const alpha = (name) => Number(($('.app').getAttribute('style').match(new RegExp(`--${name}: rgba\\([^)]*, ([0-9.]+)\\)`)) || [])[1]);
  const blur = $('#glass-blur');
  const label = () => $('#glass-blur')?.parentElement.querySelector('.set-value')?.textContent;
  blur.value = '0';
  blur.dispatchEvent(new Event('input', { bubbles: true }));
  check('Blur is a slider: at the left it says None', await until(() => label() === 'None'), label());
  blur.value = '32';
  blur.dispatchEvent(new Event('input', { bubbles: true }));
  check('Half-way along it says 50%', await until(() => label() === '50%'), label());
  slider.value = '0';
  slider.dispatchEvent(new Event('input', { bubbles: true }));
  check('A solid window has nothing to blur, so Blur is greyed out', await until(() => $('#glass-blur')?.disabled) && alpha('frame-d') === 1 && alpha('card-l') === 1, `${alpha('frame-d')} ${alpha('card-l')}`);
  slider.value = '48';
  slider.dispatchEvent(new Event('input', { bubbles: true }));
  slider.dispatchEvent(new Event('change', { bubbles: true }));
  blur.value = '16';
  blur.dispatchEvent(new Event('input', { bubbles: true }));
  blur.dispatchEvent(new Event('change', { bubbles: true }));
  await until(() => !$('#glass-blur')?.disabled && label() === '25%');
  check('At the usual amount the frame and the page card are as clear as in Aven', alpha('frame-d') === 0.635 && alpha('card-d') === 0.476 && alpha('frame-l') === 0.749 && alpha('card-l') === 0.863, `dark ${alpha('frame-d')} / ${alpha('card-d')}, light ${alpha('frame-l')} / ${alpha('card-l')}`);
  check('No API key option is left', !$('.settings-view input[type=password]') && !document.body.textContent.includes('abliteration'));
  snap('settings');
  check('Settings lists its sections down the left', $$('.settings-nav-item').map((b) => b.textContent).join(', ') === 'Appearance, Space, Calendar, AI, Notifications, Data', $$('.settings-nav-item').map((b) => b.textContent).join(', '));
  let opened = 0;
  for (const name of ['Space', 'AI', 'Notifications', 'Data', 'Appearance']) {
    if (await settingsSection(name)) opened++;
  }
  check('Each section opens with its own page', opened === 5, String(opened));
  check('Settings rows have a name and a control', $$('.settings-view .set-row').length >= 4 && $$('.settings-view .set-row .set-label').length >= 4);
  check('The sidebar and page are hidden behind Settings', getComputedStyle($('.work-card')).visibility === 'hidden');
  $('.settings-back').click();
  check('Back to notes closes Settings', await until(() => !$('.settings-view') && getComputedStyle($('.work-card')).visibility === 'visible'));
});

await run('Search', async () => {
  $('.search-field').click();
  const input = await until(() => $('.qf-input'));
  input.value = 'Getting';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  check('Search finds the welcome page', await until(() => $('.qf-item')?.textContent.includes('Getting Started')));
  // Several words, in another order than in the name.
  input.value = 'started getting';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  check('Search takes several words in any order', await until(() => $$('.qf-item').some((i) => i.textContent.includes('Getting Started')), 2000), $$('.qf-item').map((i) => i.textContent.slice(0, 30)).join(' | '));
  input.value = 'getting zzzznothing';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  check('…and every word must be in the page', await until(() => !$$('.qf-item').some((i) => i.textContent.includes('Getting Started')), 2000));
  key(input, 'Escape');
  await until(() => !$('.modal'));
});

await run('Toggle opens with a reveal', async () => {
  $$('.tree-item').find((t) => t.textContent.includes('Getting Started')).click();
  const toggle = await until(() => $('.block-row.kind-toggle .toggle-arrow.closed') || $('.block-row.kind-toggle .toggle-arrow'));
  if (toggle.classList.contains('closed')) {
    toggle.click();
  } else {
    toggle.click();
    await wait(200);
    $('.block-row.kind-toggle .toggle-arrow').click();
  }
  check('Opened toggle children ease in', await until(() => $('.block-row.appear'), 1500));
});

await run('To-do tick pops', async () => {
  const box = $('.check');
  box.click();
  check('Ticking a to-do animates it', box.dataset.pop === '1');
});

await run('AI panel opens without an API option', async () => {
  // How far the last line of the page is from the first: the box must not change it.
  const spread = () => {
    const rows = $$('.pane .block-row').filter((r) => r.offsetParent);
    return rows.length > 1 ? rows[rows.length - 1].getBoundingClientRect().top - rows[0].getBoundingClientRect().top : 0;
  };
  const before = spread();
  // At rest the AI is one small round button at the foot of the pane, with no words on it.
  const dock = $('.pane .ai-dock');
  const at = (el) => el.getBoundingClientRect();
  const rest = at(dock);
  const paneBox = at(dock.closest('.pane'));
  check('The AI is a small round button at the foot of the page, in the middle, with no words', dock && Math.round(rest.width) === 32 && Math.round(rest.height) === 32 && dock.textContent.trim() === '' && Math.abs(rest.left + rest.width / 2 - (paneBox.left + paneBox.width / 2)) < 1.5 && paneBox.bottom - rest.bottom > 8 && paneBox.bottom - rest.bottom < 24, `${Math.round(rest.width)}x${Math.round(rest.height)}, ${Math.round(paneBox.bottom - rest.bottom)} from the foot, "${dock?.textContent.trim()}"`);
  check('No other Ask AI button: not in the bar of the page and not in the sidebar', !$$('.pagebar button').some((b) => /Ask AI/.test(b.textContent + b.title)) && !$$('.sidebar .tile').some((t) => /Ask AI/.test(t.title)));
  askAi();
  const panel = await until(() => $('.ai-dock .ai-panel'));
  check('Ask AI opens the panel', panel);
  // It opens with a motion of its width (not a jump), and what is in it comes in after.
  const moves = dock.getAnimations().filter((a) => a.transitionProperty === 'width');
  check('The button opens into the bar with a motion of its width', moves.length === 1 && moves[0].effect.getTiming().duration >= 250 && moves[0].effect.getTiming().duration <= 450, `${moves.length} motion, ${moves[0]?.effect.getTiming().duration} ms`);
  moves.forEach((a) => a.finish());
  dock.getAnimations().forEach((a) => a.finish());
  await wait(60);
  const open = at(dock);
  check('The bar is small: one line high and not as wide as the page', open.height >= 38 && open.height <= 44 && open.width > 300 && open.width <= 481 && open.width < paneBox.width && Math.abs(open.left + open.width / 2 - (paneBox.left + paneBox.width / 2)) < 1.5, `${Math.round(open.width)} x ${Math.round(open.height)} in ${Math.round(paneBox.width)}`);
  check('The box floats over the page: the lines after it stay where they are', panel && getComputedStyle(dock).position === 'absolute' && !panel.closest('.scroller') && before > 20 && Math.abs(spread() - before) < 1, `${Math.round(before)} then ${Math.round(spread())}, ${getComputedStyle(dock).position}`);
  check('…and all of it is in view', open.bottom <= paneBox.bottom && open.top >= paneBox.top && open.left >= paneBox.left && open.right <= paneBox.right);
  {
    const cs = getComputedStyle(dock);
    const alpha = Number((cs.backgroundColor.match(/[\d.]+/g) || [])[3] ?? 1);
    const blur = cs.backdropFilter || cs.webkitBackdropFilter || '';
    check('The bar is glass: a clear tint over the page, a blur of its own, one hairline', alpha > 0.3 && alpha < 0.75 && /blur\(/.test(blur) && parseFloat(cs.borderTopWidth) === 1 && Math.round(open.height) === 44, `${cs.backgroundColor} ${blur} ${Math.round(open.height)}`);
  }
  // What one step of the motion costs: the width is set to 40 sizes in turn, with the layout done each time.
  {
    const t0 = performance.now();
    for (let i = 0; i < 40; i++) {
      dock.style.width = `${40 + i * 11}px`;
      void dock.offsetHeight;
      void panel.getBoundingClientRect();
    }
    const each = (performance.now() - t0) / 40;
    dock.style.width = '';
    void dock.offsetHeight;
    dock.getAnimations().forEach((a) => a.finish());
    check('One step of the motion costs little (so no frame is lost)', each < 2, `${each.toFixed(2)} ms for one step`);
  }
  check('The AI panel offers no API key', panel && !/abliteration|api key/i.test(panel.textContent) && !panel.querySelector('input[type=password]'), panel?.textContent.slice(0, 120));
  // One quiet field, after Aven's message box: no title, no chips, no close
  // button, and a send button only once there is something to send.
  const field = $('#ai-input');
  check('The AI box is one quiet field: no title, no chips, no close button', field && !$('.ai-head', panel) && !$('.chip', panel) && !$('.ai-spark', panel) && !$('.icon-btn', panel) && !$('#ai-send'), panel?.innerHTML.slice(0, 160));
  check('Its text has no box of its own and no coloured ring', field && getComputedStyle(field).borderTopWidth === '0px' && getComputedStyle(field).boxShadow === 'none' && getComputedStyle(field).backgroundColor === 'rgba(0, 0, 0, 0)');
  field.value = 'What is on this page?';
  field.dispatchEvent(new Event('input', { bubbles: true }));
  const sendBtn = await until(() => $('#ai-send'));
  const ink = getComputedStyle(document.body).color;
  check('Something typed brings a round send button in the colour of the text, not blue', sendBtn && getComputedStyle(sendBtn).borderRadius === '50%' && getComputedStyle(sendBtn).backgroundColor === getComputedStyle(field).color && sendBtn.title === 'Ask', sendBtn ? `${getComputedStyle(sendBtn).backgroundColor} vs ${ink}` : 'no button');
  field.value = '';
  field.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => !$('#ai-send'));
  // A press outside puts the empty box away; with words in it, it stays.
  field.value = 'half a question';
  field.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  $('.pagebar').dispatchEvent(new MouseEvent('click', { bubbles: true }));
  await wait(200);
  check('A press outside leaves the box when something is typed in it', $('#ai-input')?.value === 'half a question');
  field.value = '';
  field.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => !$('#ai-send'));
  $('.pagebar').dispatchEvent(new MouseEvent('click', { bubbles: true }));
  check('A press outside puts the empty box away', await until(() => !$('.ai-panel')));
});

await run('Move a tab to a new window', async () => {
  await oneTab();
  treeItem('Getting Started').click();
  await until(() => $$('.tab').length === 2);
  $$('.tab')[1].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  const menu = await until(() => $('.tab-menu'));
  menuItem(menu, 'Move to New Window').click();
  check('The tab leaves this window', await until(() => $$('.tab').length === 1));
  await wait(1500);
});

await run('Spaces', async () => {
  $('.ws-name').click();
  const menu = await until(() => $('.space-menu'));
  check('Space menu opens', menu);
  await wait(200);
  snap('spaces');
  menuItem(menu, 'New space').click();
  const input = await until(() => $('.space-new input'));
  input.value = 'Second space';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(50);
  key(input, 'Enter');
  check('New space is created and shown', await until(() => $('.ws-label')?.textContent === 'Second space'));
  check('A new space starts empty', !treeItem('Getting Started'));
  await newPage('Only in space two');
  $('.ws-name').click();
  const menu2 = await until(() => $('.space-menu'));
  menuItem(menu2, 'My Space').click();
  check('Switching back shows the first space', await until(() => treeItem('Getting Started') && !treeItem('Only in space two')));

  // The order of the spaces: a space dragged in the menu takes its new place.
  {
    $('.ws-name').click();
    await until(() => $('.space-menu .space-row'));
    // The name only: a row also holds the space's letter badge.
    const nameOf = (r) => (r.querySelector('.grow')?.textContent || '').trim();
    const names = () => $$('.space-menu .space-row').map(nameOf);
    const drag = async (from, to, after) => {
      const row = (name) => $$('.space-menu .space-row').find((r) => nameOf(r) === name);
      const a = row(from);
      const b = row(to);
      const box = b.getBoundingClientRect();
      const at = { bubbles: true, cancelable: true, clientX: box.left + 60, clientY: after ? box.bottom - 5 : box.top + 5 };
      const dt = new DataTransfer();
      a.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
      await wait(60);
      b.dispatchEvent(new DragEvent('dragover', { ...at, dataTransfer: dt }));
      await wait(80);
      const shown = b.classList.contains(after ? 'drop-after' : 'drop-before');
      b.dispatchEvent(new DragEvent('drop', { ...at, dataTransfer: dt }));
      a.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
      return shown;
    };
    check('Each space in the menu can be dragged, with a grip', $$('.space-menu .space-row[draggable="true"]').length === 2 && $('.space-menu .space-grip'));
    const marked = await drag('Second space', 'My Space', false);
    check('Dragged over the upper half of a space, a line shows above it', marked);
    check('Let go there, the space takes that place in the menu', await until(() => names().join('|') === 'Second space|My Space'), names().join('|'));
    check('…and in the dots at the foot of the sidebar', await until(() => $$('.side-foot .space-dot')[0]?.title === 'Second space'), $$('.side-foot .space-dot').map((d) => d.title).join('|'));
    await drag('Second space', 'My Space', true);
    check('Dragged to the lower half of the last space, it goes to the end', await until(() => names().join('|') === 'My Space|Second space'), names().join('|'));
    key(document.activeElement || document.body, 'Escape');
    await until(() => !$('.space-menu'), 1500);
  }

  // Each space has its own tabs.
  const tabTitles = () => $$('.tab .tab-title').map((t) => t.textContent);
  check('The tabs of a space are not in the bar of another space', await until(() => tabTitles().length && !tabTitles().includes('Only in space two')), tabTitles().join(' | '));
  treeItem('Getting Started').click();
  check('A page opens in a tab of its space', await until(() => $('.tab.active .tab-title')?.textContent === 'Getting Started'), tabTitles().join(' | '));
  const firstTabs = tabTitles().join(' | ');

  // The foot of the sidebar: Settings, and a dot for each space.
  const dots = () => $$('.side-foot .space-dot');
  const current = () => dots().findIndex((d) => d.classList.contains('current'));
  const foot = $('.side-foot').getBoundingClientRect();
  const side = $('.sidebar').getBoundingClientRect();
  check('Settings is at the foot of the sidebar, not in the menu of the spaces', $('#side-settings') && foot.bottom <= side.bottom + 1 && foot.top >= $('.side-scroll').getBoundingClientRect().bottom - 1 && !menuItem(menu2, 'Settings'));
  // (The dot of the space gets longer in a short move: wait for it.)
  const longer = await until(() => dots().length === 2 && dots()[0].getBoundingClientRect().width > dots()[1].getBoundingClientRect().width + 6, 1500);
  check('The foot has a dot for each space, with the one that shows marked', dots().length === 2 && current() === 0 && dots()[1].title === 'Second space' && longer, `${dots().length} dots, current ${current()}, widths ${dots().map((d) => d.getBoundingClientRect().width).join(' and ')}`);
  await wait(450);
  snap('side-foot');
  let ghost = null;
  const seen = new MutationObserver(() => {
    ghost = ghost || $('.sidebar > .side-ghost .side-scroll');
  });
  seen.observe($('.sidebar'), { childList: true });
  dots()[1].click();
  const list = await until(() => $('.ws-label')?.textContent === 'Second space' && $('.sidebar > .side-scroll'));
  check('The list of the last space slides out while the new one comes in', await until(() => ghost, 1000) && ghost.dataset.space !== list.dataset.space && ghost.textContent.includes('Getting Started'), ghost ? ghost.dataset.space : 'no picture of the old list');
  seen.disconnect();
  check('…and is gone when the move is over', await until(() => !$('.side-ghost'), 1500));
  check('A click on a dot goes to its space, and its list comes in from that side', list && current() === 1 && treeItem('Only in space two') && list.classList.contains('from-next'), list?.className);
  check('…and the mark of the move goes away when it is over', await until(() => $('.side-scroll').className === 'side-scroll', 2000), $('.side-scroll').className);
  check('Another space brings back its own tabs, with the one you left in front', await until(() => $('.tab.active .tab-title')?.textContent === 'Only in space two') && !tabTitles().includes('Getting Started'), tabTitles().join(' | '));
  check('The highlight is under the tab in front after a change of space', await until(() => Math.abs($('.tabs .tab-glider').getBoundingClientRect().left - $('.tab.active').getBoundingClientRect().left) < 2, 800), `${$('.tabs .tab-glider').getBoundingClientRect().left} and ${$('.tab.active').getBoundingClientRect().left}`);
  $('.tabs .new-tab').click();
  check('A new tab belongs to the space in front', await until(() => $('.tab.active .tab-title')?.textContent === 'Home') && tabTitles().includes('Only in space two'), tabTitles().join(' | '));
  $('.tab.active .tab-close').click();
  await until(() => $('.tab.active .tab-title')?.textContent === 'Only in space two');

  // Two fingers moved sideways on the sidebar: one space for each move.
  const swipe = async (dx) => {
    for (let i = 0; i < 4; i++) {
      $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: dx, deltaY: 0, bubbles: true, cancelable: true }));
      await wait(16);
    }
    // The glide after the move: smaller and smaller steps.
    for (const part of [0.6, 0.4, 0.2, 0.1]) {
      $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: dx * part, deltaY: 0, bubbles: true, cancelable: true }));
      await wait(16);
    }
  };
  await swipe(-30);
  check('Two fingers moved to the right go to the space before', await until(() => $('.ws-label')?.textContent === 'My Space' && current() === 0, 2000), $('.ws-label')?.textContent);
  check('The first space has the same tabs as before, and the same one in front', await until(() => tabTitles().join(' | ') === firstTabs && $('.tab.active .tab-title')?.textContent === 'Getting Started'), `${tabTitles().join(' | ')} (was ${firstTabs})`);
  await wait(250);
  await swipe(-30);
  await wait(200);
  check('At the first space, the same move does nothing', $('.ws-label')?.textContent === 'My Space' && treeItem('Getting Started'));
  await wait(250);
  // Up and down is the list, not a move to another space.
  for (let i = 0; i < 6; i++) $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: 8, deltaY: 60, bubbles: true, cancelable: true }));
  await wait(200);
  check('Two fingers up or down do not change the space', $('.ws-label')?.textContent === 'My Space');
  await wait(250);
  await swipe(30);
  check('Two fingers moved to the left go to the space after, once for one move', await until(() => $('.ws-label')?.textContent === 'Second space' && current() === 1, 2000), $('.ws-label')?.textContent);
  await wait(250);
  dots()[0].click();
  await until(() => $('.ws-label')?.textContent === 'My Space' && treeItem('Getting Started'));
  await wait(450);

  // The fingers stop short: the list leans, and goes back; the space stays.
  $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: 14, deltaY: 0, bubbles: true, cancelable: true }));
  await wait(30);
  check('The list leans with the fingers before the space changes', $('.side-scroll').style.transform.includes('translateX(-') && Number($('.side-scroll').style.opacity) < 1, $('.side-scroll').style.transform);
  check('…and goes back when the fingers stop short', await until(() => !$('.side-scroll').style.transform && $('.ws-label')?.textContent === 'My Space', 1500), $('.side-scroll').style.transform);
  await wait(320);

  // A light move is enough: small steps, with a little up and down in them.
  const light = async (dx) => {
    const steps = [[1, 1], [2, -1], [4, 1], [5, 0], [6, 2], [6, -1], [5, 0], [4, 1]];
    for (const [x, y] of steps) {
      $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: x * dx, deltaY: y, bubbles: true, cancelable: true }));
      await wait(16);
    }
  };
  const glide = async (dx, ms) => {
    // The glide after the move: weaker and weaker, with no pause.
    const from = performance.now();
    for (let step = 4; performance.now() - from < ms; step = Math.max(1, step - 1)) {
      $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: step * dx, deltaY: 0, bubbles: true, cancelable: true }));
      await wait(30);
    }
  };
  await light(1);
  check('A light move of the fingers is enough to change the space', await until(() => $('.ws-label')?.textContent === 'Second space' && current() === 1, 2000), $('.ws-label')?.textContent);
  // The fingers go on after the space changed, faster and slower, and then
  // the glide: it is all one move. Nothing more changes, and the new list
  // does not lean or jump.
  let leaned = false;
  const watchLean = new MutationObserver(() => {
    if ($('.sidebar > .side-scroll')?.style.transform) leaned = true;
  });
  watchLean.observe($('.sidebar'), { attributes: true, attributeFilter: ['style'], subtree: true });
  for (const x of [3, 9, 4, 11, 3, 8, 12, 5]) {
    $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: x, deltaY: 0, bubbles: true, cancelable: true }));
    await wait(40);
  }
  await glide(1, 320);
  watchLean.disconnect();
  check('The rest of the same move, and its glide, do not change the space again', $('.ws-label')?.textContent === 'Second space' && current() === 1);
  check('…and the new list does not lean or jump from them', !leaned && !$('.sidebar > .side-scroll').style.transform);
  // A move the other way is a new move, with no pause before it.
  await light(-1);
  check('A move the other way is seen with no pause after the last one', await until(() => $('.ws-label')?.textContent === 'My Space' && current() === 0, 2000), $('.ws-label')?.textContent);
  await wait(450);

  // Three spaces: two moves, one soon after the other, go from the first to
  // the third. (The second move was lost in the glide of the first, and the
  // sidebar stopped at the middle space.)
  $('.ws-name').click();
  menuItem(await until(() => $('.space-menu')), 'New space').click();
  const third = await until(() => $('.space-new input'));
  third.value = 'Third space';
  third.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(50);
  key(third, 'Enter');
  await until(() => $('.ws-label')?.textContent === 'Third space');
  await wait(450);
  dots()[0].click();
  await until(() => $('.ws-label')?.textContent === 'My Space' && treeItem('Getting Started'));
  await wait(450);
  const wheel = (x, y = 0) => $('.side-scroll').dispatchEvent(new WheelEvent('wheel', { deltaX: x, deltaY: y, bubbles: true, cancelable: true }));
  // One move of the fingers, then its glide: weaker and weaker, with no pause.
  const flick = async () => {
    for (const x of [3, 8, 14, 18, 16, 12]) {
      wheel(x);
      await wait(12);
    }
  };
  const tail = async (steps) => {
    for (const x of steps) {
      wheel(x);
      await wait(12);
    }
  };
  await flick();
  await tail([10, 8, 7, 6, 5, 5, 4, 4, 3, 3, 2, 2, 2, 1, 1]);
  check('With three spaces, the first move goes to the middle space', await until(() => $('.ws-label')?.textContent === 'Second space' && current() === 1, 2000), $('.ws-label')?.textContent);
  // The second move starts in the glide of the first: no pause at all.
  await flick();
  await tail([10, 8, 6, 5, 4, 3, 2, 2, 1, 1]);
  check('A second move soon after it, with no pause, goes on to the third space', await until(() => $('.ws-label')?.textContent === 'Third space' && current() === 2, 2000), $('.ws-label')?.textContent);
  await wait(500);
  // One long move with its long glide is still one space, not two.
  dots()[0].click();
  await until(() => $('.ws-label')?.textContent === 'My Space' && treeItem('Getting Started'));
  await wait(450);
  await tail([3, 8, 14, 18, 20, 20, 19, 20, 18, 19, 20, 18, 17, 18, 19, 20, 18, 16, 15, 14, 13, 12, 11, 10, 9, 8, 8, 7, 6, 6, 5, 5, 4, 4, 3, 3, 3, 2, 2, 2, 1, 1, 1, 1]);
  await wait(200);
  check('One long move and its glide go one space only', $('.ws-label')?.textContent === 'Second space' && current() === 1, $('.ws-label')?.textContent);
  await wait(450);
  // A move that starts a little downward and then goes sideways is a move for the spaces.
  for (const [x, y] of [[2, 9], [3, 10], [4, 9], [9, 4], [13, 2], [15, 1], [15, 0], [14, 1], [12, 0], [9, 0], [6, 0], [4, 0], [2, 0]]) {
    wheel(x, y);
    await wait(12);
  }
  check('A move that starts a little downward and then goes sideways changes the space', await until(() => $('.ws-label')?.textContent === 'Third space' && current() === 2, 2000), $('.ws-label')?.textContent);
  await wait(500);
  // Up and down with a little drift sideways is still the list.
  for (let i = 0; i < 12; i++) {
    wheel(-3, 14);
    await wait(12);
  }
  await wait(200);
  check('Up and down with a little drift does not change the space', $('.ws-label')?.textContent === 'Third space' && current() === 2);
  await wait(500);
  // Soft moves, one right after the other, from the third space back to the
  // first. (A soft second move was lost: only a strong one went through.)
  const softMove = async (way) => {
    for (const x of [1, 2, 3, 4, 5, 5, 4, 4, 3, 3, 2, 2, 2, 1, 1]) {
      wheel(x * way);
      await wait(12);
    }
  };
  await softMove(-1);
  check('A soft move from the third space goes to the middle space', await until(() => $('.ws-label')?.textContent === 'Second space' && current() === 1, 2000), $('.ws-label')?.textContent);
  await softMove(-1);
  check('A soft move right after it, with no pause, goes on to the first space', await until(() => $('.ws-label')?.textContent === 'My Space' && current() === 0, 2000), $('.ws-label')?.textContent);
  await wait(500);
  await softMove(1);
  await softMove(1);
  check('…and two soft moves the other way go to the third space', await until(() => $('.ws-label')?.textContent === 'Third space' && current() === 2, 2000), $('.ws-label')?.textContent);
  await wait(500);
  // A very soft move that goes on after the change (steps of 1 to 3) is one move.
  dots()[0].click();
  await until(() => $('.ws-label')?.textContent === 'My Space' && treeItem('Getting Started'));
  await wait(450);
  for (const x of [1, 2, 3, 2, 3, 3, 2, 3, 3, 2, 3, 2, 3, 3, 2, 3, 2, 1, 2, 3, 2, 3, 2, 2, 3, 2, 1, 2, 1, 1, 2, 1, 1]) {
    wheel(x);
    await wait(12);
  }
  await wait(200);
  check('One very soft move that goes on is still one space', $('.ws-label')?.textContent === 'Second space' && current() === 1, $('.ws-label')?.textContent);
  await wait(300);
  dots()[0].click();
  await until(() => $('.ws-label')?.textContent === 'My Space' && treeItem('Getting Started'));
  await wait(450);

  // The sidebar that floats (not pinned): the same move works there.
  T().command('toggle_sidebar');
  const edge = await until(() => $('.edge-strip') && !$('.app').classList.contains('pinned') && $('.edge-strip'));
  edge.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  const card = await until(() => document.body.dataset.peek === '1' && $('.sidebar.floating'), 1500);
  if (card) {
    await wait(420);
    const box = card.getBoundingClientRect();
    // As the trackpad sends it: to what is under the pointer (here the page behind is named, to be sure the place of the pointer is enough).
    const over = { clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, bubbles: true, cancelable: true };
    for (let i = 0; i < 4; i++) {
      $('.work-card').dispatchEvent(new WheelEvent('wheel', { ...over, deltaX: 30, deltaY: 0 }));
      await wait(16);
    }
    check('On the sidebar that floats, two fingers moved sideways change the space too', await until(() => $('.ws-label')?.textContent === 'Second space' && current() === 1, 2000), $('.ws-label')?.textContent);
    check('…and the sidebar stays out', document.body.dataset.peek === '1' && $('.sidebar.floating'));
    await wait(300);
    for (let i = 0; i < 4; i++) {
      $('.sidebar.floating .side-scroll').dispatchEvent(new WheelEvent('wheel', { ...over, deltaX: -30, deltaY: 0 }));
      await wait(16);
    }
    check('…and back again', await until(() => $('.ws-label')?.textContent === 'My Space' && current() === 0, 2000), $('.ws-label')?.textContent);
  } else {
    check('The sidebar that floats came out for the check (the window must be in view)', false, 'it did not come out');
  }
  $('.edge-strip')?.click();
  await until(() => $('.app').classList.contains('pinned'));
  await wait(450);

  // The two buttons of a row lie over its end: they take no room from its name.
  const row = () => $$('.side-scroll .tree-item[data-fav="0"]').find((t) => t.querySelector('.tree-title')?.textContent === 'Getting Started');
  const nameWidth = () => row().querySelector('.tree-title').getBoundingClientRect().width;
  const acts = () => getComputedStyle(row().querySelector('.tree-actions'));
  const before = nameWidth();
  const hiddenFirst = acts().opacity === '0';
  row().querySelector('.tree-actions .row-btn').click();
  await until(() => $('.sidebar-menu') && row().classList.contains('menu-open'));
  check('The buttons of a row in the sidebar show without making its name shorter', hiddenFirst && acts().opacity === '1' && acts().position === 'absolute' && Math.abs(nameWidth() - before) < 0.5 && before > 60, `${before} then ${nameWidth()}, opacity ${acts().opacity}`);
  row().querySelector('.tree-actions .row-btn').click();
  await until(() => !$('.sidebar-menu'));
});

await run('Model pickers', async () => {
  await openSettings();
  await settingsSection('AI');
  const sel = await until(() => $$('.settings-view select.select').find((x) => [...x.options].some((o) => o.textContent.includes('Opus'))));
  check('Settings has a model picker', sel && sel.options.length > 1, sel && [...sel.options].map((o) => o.textContent).join(', '));
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
  askAi();
  const pick = await until(() => $('#ai-model'));
  check('Ask AI has a picker that shows the strength as four dots and the model’s name', pick && $$('.ai-pips i', pick).length === 4 && $('.ai-model-name', pick)?.textContent.length > 2 && !$('.ai-panel select'), pick?.textContent);
  pick.click();
  const list = await until(() => $('.ai-menu'));
  const models = () => $$('.ai-menu-row[data-model]');
  const efforts = () => $$('.ai-menu-row[data-effort]');
  check('It opens a menu of the models, grouped by tool, and of five strengths', list && models().length > 1 && $$('.ai-menu-label').length >= 2 && efforts().length === 5, `${models().length} models, ${efforts().length} strengths`);
  {
    list.getAnimations().forEach((a) => a.finish());
    const m = list.getBoundingClientRect();
    const bar = $('.ai-dock').getBoundingClientRect();
    check('The menu opens upward from the bar, all of it in the window', m.bottom <= bar.top && m.top >= 0 && m.right <= innerWidth && m.left >= 0, `menu ${Math.round(m.top)}..${Math.round(m.bottom)}, bar at ${Math.round(bar.top)}`);
  }
  check('One model and one strength are marked as chosen', $$('.ai-menu-row.active[data-model]').length === 1 && $$('.ai-menu-row.active[data-effort]').length === 1);
  check('Each strength has its dots and a few words on it', efforts().every((r, i) => $$('.ai-pips i[data-on="1"]', r).length === i && r.querySelector('.ai-menu-aside')?.textContent), efforts().map((r) => $$('.ai-pips i[data-on="1"]', r).length).join(','));
  await wait(300);
  snap('ai-menu');
  $('.ai-menu-row[data-effort="high"]').click();
  check('Choosing a strength marks it, and the picker shows its dots', await until(() => $('.ai-menu-row.active[data-effort="high"]') && $$('.ai-pips i[data-on="1"]', $('#ai-model')).length === 3), String($$('.ai-pips i[data-on="1"]', $('#ai-model')).length));
  const other = models().find((r) => !r.classList.contains('active'));
  const name = other.querySelector('.ai-menu-value').textContent;
  other.click();
  check('Choosing a model marks it, and the picker shows its name', await until(() => other.classList.contains('active') && $('.ai-model-name', $('#ai-model')).textContent === name), `${$('.ai-model-name', $('#ai-model'))?.textContent} / ${name}`);
  $('.ai-menu-shade').click();
  check('A press outside the menu puts it away, and the AI box stays', await until(() => !$('.ai-menu')) && $('#ai-input'));
  $('#ai-model').click();
  await until(() => $('.ai-menu'));
  $('.ai-menu-row[data-effort=""]').click();
  models()[0].click();
  $('.ai-menu-shade').click();
  await until(() => !$('.ai-menu'));
  await wait(250);
  snap('ai');
  const typed = $('#ai-input');
  typed.value = 'What did we decide about the budget?';
  typed.setAttribute('value', typed.value);
  typed.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  await wait(100);
  snap('ai-typed');
  typed.value = '';
  typed.removeAttribute('value');
  typed.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => !$('#ai-send'));
  key($('#ai-input'), 'Escape');
  await until(() => !$('.ai-panel'));
});

await run('Model names carry version numbers', async () => {
  await openSettings();
  await settingsSection('AI');
  const sel = await until(() => $$('.settings-view select.select').find((x) => [...x.options].some((o) => o.textContent.includes('Opus'))));
  const labels = sel ? [...sel.options].map((o) => o.textContent) : [];
  check('Model names carry version numbers', labels.includes('Opus 5.5') && labels.some((l) => l.startsWith('Default')), labels.join(', '));
  await settingsSection('AI');
  snap('settings-ai');
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
});

await run('Calendar', async () => {
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  check('Calendar opens in week view', await until(() => $('.calendar .cal-week')));
  $('.cal-toolbar .primary-btn').click();
  key(await until(() => $('#cal-quick')), 'Enter');
  const title = await until(() => $('.cal-editor .ed-title'));
  check('New event opens the editor', title);
  const remind = $('#ev-remind');
  check('New events get a 10-minute reminder', remind && remind.dataset.value === '10' && remind.textContent.includes('10 min before'), remind?.textContent);
  title.value = 'Team sync';
  title.dispatchEvent(new Event('input', { bubbles: true }));
  check('The event shows in the week', await until(() => $$('.cal-event .ev-title').some((t) => t.textContent === 'Team sync')));
  await wait(250);
  snap('week');
  // A click on an empty time: a 15-minute slot to type the name in.
  const col = () => $$('.cal-col')[3];
  const clickAt = (hours) => {
    const r = col().getBoundingClientRect();
    const at = { bubbles: true, clientX: r.left + 10, clientY: r.top + hours * 48 + 3 };
    col().dispatchEvent(new MouseEvent('mousedown', at));
    col().dispatchEvent(new MouseEvent('click', at));
  };
  const before = $$('.cal-event').length;
  clickAt(6.3);
  check('With an event open, a click on an empty time only closes it', await until(() => !$('.cal-editor')) && !$('#ev-draft') && $$('.cal-event').length === before);
  clickAt(6.3);
  const slot = await until(() => $('#ev-draft'));
  check('A click on an empty time puts a slot there to type in', slot && await until(() => document.activeElement?.id === 'ev-draft'), document.activeElement?.id || document.activeElement?.tagName);
  check('The slot starts on the quarter hour that was clicked', slot?.parentElement.style.top === '300px' && slot.placeholder.includes('6:15'), `${slot?.parentElement.style.top} ${slot?.placeholder}`);
  snap('week-slot');
  slot.value = 'Gym';
  slot.dispatchEvent(new Event('input', { bubbles: true }));
  key(slot, 'Enter');
  const made = await until(() => $$('.cal-event:not(.draft)').find((e) => e.querySelector('.ev-title')?.textContent === 'Gym'));
  check('Return keeps it as a 15-minute event, with no panel in the way', made && made.style.top === '300px' && !$('#ev-draft') && !$('.cal-editor') && $$('.cal-event').length === before + 1, `${made?.style.top} ${$$('.cal-event').length - before}`);
  clickAt(10);
  const dropped = await until(() => $('#ev-draft'));
  dropped.value = 'Not this';
  dropped.dispatchEvent(new Event('input', { bubbles: true }));
  key(dropped, 'Escape');
  check('Escape drops the slot', await until(() => !$('#ev-draft')) && $$('.cal-event').length === before + 1, String($$('.cal-event').length - before));
  clickAt(11);
  const empty = await until(() => $('#ev-draft'));
  empty.dispatchEvent(new FocusEvent('blur'));
  empty.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
  check('A slot left with no name goes away', await until(() => !$('#ev-draft')) && $$('.cal-event').length === before + 1, String($$('.cal-event').length - before));
  made.click();
  await until(() => $('.cal-editor .ed-title')?.value === 'Gym');
  const times = [$('#ev-start'), $('#ev-end')].map((b) => b?.dataset.value).join(' to ');
  check('The panel shows the 15 minutes', /6:15.*6:30|06:15.*06:30/.test(times), times);
  $('#ev-delete').click();
  check('Delete removes it', await until(() => $$('.cal-event').length === before));
  check('It says "Event deleted"', await until(() => $('.toast')?.textContent === 'Event deleted'));
  check('The message goes away on its own', await until(() => !$('.toast'), 2500));
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Month').click();
  check('Month view shows the event', await until(() => $('.cal-month') && $$('.cal-chip .chip-title').some((t) => t.textContent === 'Team sync')));
  await wait(200);
  snap('month');
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
});

await run('Repeating events', async () => {
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar .cal-week'));
  $('.cal-toolbar .primary-btn').click();
  key(await until(() => $('#cal-quick')), 'Enter');
  const title = await until(() => $('.cal-editor .ed-title'));
  title.value = 'Weekly review';
  title.dispatchEvent(new Event('input', { bubbles: true }));
  const rule = $('#ev-repeat');
  check('Events start out not repeating', rule && rule.dataset.value === 'none', rule && rule.dataset.value);
  rule.click();
  (await until(() => $('.ed-menu [data-value="week"]'))).click();
  check('Choosing a repeat offers an end', await until(() => $('#ev-until')));
  const shown = () => $$('.cal-event').find((e) => e.querySelector('.ev-title')?.textContent.includes('Weekly review'));
  const next = () => $('.cal-nav .icon-btn[title="Next"]').click();
  next();
  await wait(120);
  const ev = await until(shown);
  check('A weekly event shows the next week too, marked as repeating', ev && ev.querySelector('.repeat-mark'));
  // The panel still shows this week's repeat; wait for the one clicked.
  const shownDay = $('#ev-day')?.dataset.value;
  ev.click();
  await until(() => $('#ev-day') && $('#ev-day').dataset.value !== shownDay);
  check('A repeat can be changed on its own', await until(() => $$('.cal-editor .link-btn').some((b) => b.textContent === 'Change only this one')));
  const del = $('#ev-delete');
  del.click();
  const only = await until(() => $('.ed-menu [data-value="one"]'));
  check('Deleting a repeat asks: only this one, or all', only && $('.ed-menu [data-value="all"]'), `${del.getAttribute('aria-label')} | ${$('.cal-editor')?.outerHTML.slice(-900)}`);
  if (!only) return;
  only.click();
  check('Deleting only that repeat removes it from its week', await until(() => !shown()));
  next();
  await wait(120);
  const later = await until(shown);
  check('The other repeats stay', later);
  later.click();
  (await until(() => $('#ev-delete'))).click();
  (await until(() => $('.ed-menu [data-value="all"]'))).click();
  check('Deleting all repeats removes the event', await until(() => !shown()));
  $('.cal-nav .secondary-btn').click();
});

await run('Event panel', async () => {
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar .cal-week'));
  if ($('.cal-editor')) {
    $('.cal-editor .icon-btn[title="Close"]').click();
    await until(() => !$('.cal-editor'));
  }
  $('.cal-toolbar .primary-btn').click();
  key(await until(() => $('#cal-quick')), 'Enter');
  const title = await until(() => $('.cal-editor .ed-title'));
  title.value = 'Panel check';
  title.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(250);
  check('The panel has no system date, time or list fields', !$('.cal-editor input[type="date"], .cal-editor input[type="time"], .cal-editor select'));
  const tops = $$('.cal-editor .ed-tints .tint').map((t) => Math.round(t.getBoundingClientRect().top));
  check('The nine colours sit on one line', tops.length === 9 && new Set(tops).size === 1, tops.join(','));
  const body = $('.cal-editor .ed-body');
  check('Nothing in the panel is wider than the panel', body.scrollWidth <= body.clientWidth + 1, `${body.scrollWidth} > ${body.clientWidth}`);
  const named = ['#ev-day', '#ev-start', '#ev-end', '#ev-repeat', '#ev-remind', '#ev-delete', '#ev-link', '#ev-notes'].filter((sel) => !($(sel)?.getAttribute('aria-label') || '').trim());
  check('Each control of the panel says what it is', named.length === 0, named.join(', '));
  check('A row says its value too', $('#ev-repeat').getAttribute('aria-label') === 'Repeat: Does not repeat' && /^Starts at /.test($('#ev-start').getAttribute('aria-label')), `${$('#ev-repeat').getAttribute('aria-label')} | ${$('#ev-start').getAttribute('aria-label')}`);

  // A menu: Rune's own, the chosen row grey and in focus, the arrows move, Escape puts it away.
  $('#ev-remind').click();
  const menu = await until(() => $('.ed-menu'));
  check("The reminder opens Rune's own menu, its chosen row in focus", menu && menu.getAttribute('role') === 'menu' && (await until(() => document.activeElement?.dataset.value === '10', 3000)) && document.activeElement.classList.contains('on'), document.activeElement?.outerHTML.slice(0, 160));
  const mr = menu.getBoundingClientRect();
  check('The menu is lifted out of the panel and is all on screen', getComputedStyle(menu).position === 'fixed' && mr.right <= innerWidth && mr.bottom <= innerHeight && mr.top >= 0 && mr.left >= 0, `${getComputedStyle(menu).position} ${Math.round(mr.left)},${Math.round(mr.top)},${Math.round(mr.right)},${Math.round(mr.bottom)}`);
  snap('event-panel-menu');
  key(document.activeElement, 'ArrowDown');
  check('Down moves to the next choice', document.activeElement?.dataset.value === '15', document.activeElement?.dataset.value);
  key(document.activeElement, 'Escape');
  check('Escape puts the menu away and keeps the panel', (await until(() => !$('.ed-menu'))) && $('.cal-editor'));
  check('The keyboard goes back to the reminder', await until(() => document.activeElement?.id === 'ev-remind', 3000), document.activeElement?.id);
  $('#ev-remind').click();
  (await until(() => $('.ed-menu [data-value="30"]'))).click();
  check('A choice changes the reminder', await until(() => $('#ev-remind')?.dataset.value === '30'), $('#ev-remind')?.dataset.value);

  // The times: a list a quarter of an hour apart, and a box to type any time in.
  $('#ev-start').click();
  const field = await until(() => $('.ed-menu .ed-menu-field'));
  check('The start opens a list of times a quarter of an hour apart', $$('.ed-menu .ed-menu-row').length === 96, String($$('.ed-menu .ed-menu-row').length));
  field.value = '4:10 pm';
  field.dispatchEvent(new Event('input', { bubbles: true }));
  key(field, 'Enter');
  check('A time typed in the box is the new start', await until(() => $('#ev-start')?.dataset.value === '16:10'), $('#ev-start')?.dataset.value);
  const [a, b] = [$('#ev-start').textContent, $('#ev-end').textContent];
  check('The times are short: PM is said once', /^\d{1,2}(:\d\d)?$/.test(a) && / PM$/.test(b), `${a} – ${b}`);
  $('#ev-end').click();
  await until(() => $('.ed-menu .ed-menu-row'));
  check('The end list says how long the event then is', $('.ed-menu .ed-menu-row .ed-menu-aside')?.textContent === '15 min', $('.ed-menu .ed-menu-row .ed-menu-aside')?.textContent);
  key(document.activeElement, 'Escape');
  await until(() => !$('.ed-menu'));

  // The day: Rune's own picker; the arrows move a day.
  const was = $('#ev-day').dataset.value;
  $('#ev-day').click();
  const days = await until(() => $('.ed-days'));
  check("The day opens Rune's own day picker, the day in focus", days && (await until(() => document.activeElement?.dataset.day === was, 3000)), `${document.activeElement?.dataset.day} (wanted ${was})`);
  snap('event-panel-days');
  key(document.activeElement, 'ArrowRight');
  const next = await until(() => document.activeElement?.dataset.day !== was && document.activeElement?.dataset.day, 3000);
  document.activeElement.click();
  check('The arrows move a day, and a click picks it', next && (await until(() => $('#ev-day')?.dataset.value === next)), `${was} -> ${$('#ev-day')?.dataset.value} (wanted ${next})`);
  check('The keyboard goes back to the day', await until(() => document.activeElement?.id === 'ev-day', 3000), document.activeElement?.id);
  await wait(200);
  snap('event-panel');
  $('#ev-delete').click();
  check('The bin deletes the event', await until(() => !$('.cal-editor')));
});

await run('Calendar in a narrow pane', async () => {
  const cal = await until(() => $('.calendar'));
  if ($('.cal-editor')) {
    $('.cal-editor .icon-btn[title="Close"]').click();
    await until(() => !$('.cal-editor'));
  }
  // A pane beside other pages: the panel floats over the days and covers no button.
  cal.style.width = '620px';
  $('.cal-toolbar .primary-btn').click();
  key(await until(() => $('#cal-quick')), 'Enter');
  const ed = await until(() => $('.cal-editor'));
  await wait(260);
  const er = ed.getBoundingClientRect();
  const covered = $$('.cal-toolbar > *').filter((el) => {
    const r = el.getBoundingClientRect();
    return r.right > er.left + 1 && r.left < er.right - 1 && r.bottom > er.top + 1 && r.top < er.bottom - 1;
  });
  check('The event panel covers no toolbar button', covered.length === 0, covered.map((e) => e.className || e.tagName).join(', '));
  check('The event panel floats over the days', getComputedStyle(ed).position === 'absolute');
  const head = $('.cal-week-head');
  check('The day names fit their columns', head.scrollWidth <= head.clientWidth + 1, `${head.scrollWidth} > ${head.clientWidth}`);
  const bar = $('.cal-toolbar');
  const middles = $$('.cal-toolbar > *').map((el) => Math.round(el.getBoundingClientRect().top + el.getBoundingClientRect().height / 2));
  check('The toolbar fits on one line', bar.scrollWidth <= bar.clientWidth + 1 && Math.max(...middles) - Math.min(...middles) < 8, `${bar.scrollWidth}/${bar.clientWidth} middles ${middles.join(',')}`);
  snap('calendar-narrow');
  // Very narrow: the panel takes the whole calendar.
  cal.style.width = '440px';
  await wait(80);
  const cr = cal.getBoundingClientRect();
  const fr = ed.getBoundingClientRect();
  check('In a very narrow pane the panel takes the whole calendar', fr.left <= cr.left + 1 && fr.width >= cr.width - 2 && fr.top <= cr.top + 1, `${Math.round(fr.left)},${Math.round(fr.width)} in ${Math.round(cr.left)},${Math.round(cr.width)}`);
  const glass = getComputedStyle(ed);
  check('The panel blurs what is under it, so the calendar does not show through as letters', /blur\(/.test(glass.backdropFilter || glass.webkitBackdropFilter || ''), glass.backdropFilter || glass.webkitBackdropFilter);
  $('#ev-delete').click();
  await until(() => !$('.cal-editor'));
  cal.style.width = '';
});

await run('Calendar across spaces', async () => {
  await oneTab();
  const name = () => $('.ws-label')?.textContent;
  const first = name();
  const openCalendar = async () => {
    $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
    await until(() => $('.calendar'));
    if ($('.cal-editor')) {
      $('.cal-editor .icon-btn[title="Close"]').click();
      await until(() => !$('.cal-editor'));
    }
  };
  // Two spaces at least.
  if ($$('.side-foot .space-dot').length < 2) {
    $('.ws-name').click();
    menuItem(await until(() => $('.space-menu')), 'New space').click();
    const input = await until(() => $('.space-new input'));
    input.value = 'Calendar space';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(50);
    key(input, 'Enter');
    await until(() => name() === 'Calendar space');
    $('.ws-name').click();
    menuItem(await until(() => $('.space-menu')), first).click();
    await until(() => name() === first);
  }
  await openCalendar();
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
  $('.cal-nav .secondary-btn').click();
  if ($('#cal-spaces')?.dataset.value === 'here') {
    $('#cal-spaces').click();
    (await until(() => $('.ed-menu [data-value="all"]'))).click();
  }
  check('With two spaces the calendar has one quiet choice of the spaces it shows, and shows all of them', await until(() => $('#cal-spaces')?.dataset.value === 'all'), $('#cal-spaces')?.outerHTML.slice(0, 200));
  $('.cal-toolbar .primary-btn').click();
  key(await until(() => $('#cal-quick')), 'Enter');
  const title = await until(() => $('.cal-editor .ed-title'));
  title.value = 'Across the spaces';
  title.dispatchEvent(new Event('input', { bubbles: true }));
  const ev = () => $$('.cal-event[data-event], .cal-chip[data-event]').find((e) => (e.querySelector('.ev-title, .chip-title')?.textContent || '').includes('Across the spaces'));
  check('A new event is in the space in front, and its panel says so', (await until(() => ev())) && (await until(() => ($('#ev-space')?.textContent || '').includes(first))), $('#ev-space')?.textContent);
  const mark = () => ev()?.querySelector('.ev-space');
  check('The event is marked with the letter of its space', mark()?.textContent === first[0].toUpperCase() && mark().title === first, mark()?.outerHTML);
  check('The mark is not part of the name of the event', ev()?.querySelector('.ev-title, .chip-title')?.textContent.trim() === 'Across the spaces', ev()?.querySelector('.ev-title, .chip-title')?.textContent);
  $('.cal-editor .icon-btn[title="Close"]').click();
  await until(() => !$('.cal-editor'));

  // Another space: the same calendar, the event with the mark of its own space.
  const other = $$('.side-foot .space-dot').map((d) => d.title).find((t) => t && t !== first);
  $$('.side-foot .space-dot').find((d) => d.title === other).click();
  await until(() => name() === other);
  await openCalendar();
  check('The calendar of another space shows the event too, marked with the space it is in', await until(() => mark()?.title === first), `${other}: ${ev()?.outerHTML.slice(0, 200)}`);

  // "Only this space": the calendar of one space, as before.
  $('#cal-spaces').click();
  (await until(() => $('.ed-menu [data-value="here"]'))).click();
  check('"Only this space" hides the events of the other spaces, and the marks', (await until(() => !ev())) && !$('.cal-event .ev-space, .cal-chip .ev-space'));
  check('The button says what the calendar shows', $('#cal-spaces')?.dataset.value === 'here' && ($('#cal-spaces').getAttribute('aria-label') || '').includes(other), $('#cal-spaces')?.getAttribute('aria-label'));
  snap('calendar-one-space');
  $('#cal-spaces').click();
  (await until(() => $('.ed-menu [data-value="all"]'))).click();
  check('"All spaces" shows it again', await until(() => ev()));
  await wait(200);
  snap('calendar-all-spaces');

  // Move it to this space from its panel.
  ev().click();
  (await until(() => $('#ev-space'))).click();
  const row = await until(() => $$('.ed-menu .ed-menu-row').find((r) => r.textContent.includes(other)));
  check("The Space row opens Rune's own menu of the spaces", row && $$('.ed-menu .ed-menu-row').length >= 2 && $('.ed-menu .ed-menu-row.on')?.textContent.includes(first));
  row.click();
  check('The panel moves the event to another space', await until(() => mark()?.title === other && ($('#ev-space')?.textContent || '').includes(other)), `${mark()?.title} | ${$('#ev-space')?.textContent}`);

  // Home of the first space lists it too, with its mark.
  $('.cal-editor .icon-btn[title="Close"]').click();
  await until(() => !$('.cal-editor'));
  $$('.side-foot .space-dot').find((d) => d.title === first).click();
  await until(() => name() === first);
  $$('.tile').find((t) => t.title === 'Home').click();
  const up = await until(() => $$('.up-row').find((r) => r.textContent.includes('Across the spaces')), 3000);
  check('Home shows the events of every space, each with the mark of its space', up && up.querySelector('.ev-space')?.title === other, up?.outerHTML.slice(0, 200));

  // A page of another space opens among the tabs of that space.
  await openCalendar();
  ev().click();
  (await until(() => $('#ev-page'))).click();
  check('Opening a page of another space goes to that space, in a tab there', await until(() => name() === other && ($('.tab.active .tab-title')?.textContent || '').includes('Across the spaces'), 3000), `${name()}: ${$('.tab.active .tab-title')?.textContent}`);
  $('.pagebar .icon-btn[title="Page options"]').click();
  menuItem(await until(() => $('.page-menu')), 'Move to Trash').click();
  await until(() => !$$('.tab .tab-title').some((t) => t.textContent.includes('Across the spaces')));
  $$('.side-foot .space-dot').find((d) => d.title === first).click();
  await until(() => name() === first);

  // Clean up.
  await openCalendar();
  ev()?.click();
  (await until(() => $('#ev-delete')))?.click();
  await until(() => !ev());
});

await run('Home', async () => {
  $$('.tile').find((t) => t.title === 'Home').click();
  const hero = await until(() => $('.home-hero'));
  check('Home has the header and jot box, without extra buttons', hero && $('.jot-input') && !$('.quick-actions'));
  check('Home lists the upcoming event', await until(() => $$('.up-row .up-title').some((t) => t.textContent === 'Team sync')));
  // Home is "Today": the date large, and the day as a line with a mark for now.
  const now = new Date();
  check('Home shows the day of the week and the date, large', $('.hero-date')?.textContent === now.toLocaleDateString('en-US', { weekday: 'long' }) && $('.hero-title')?.textContent === now.toLocaleDateString('en-US', { month: 'long', day: 'numeric' }) && parseFloat(getComputedStyle($('.hero-title')).fontSize) >= 40, `${$('.hero-date')?.textContent} / ${$('.hero-title')?.textContent}`);
  const line = $('#day-line .day-track');
  check('The day is a line with its hours under it', line && $$('#day-line .day-hours span').length >= 6 && $('.day-rail'), String($$('#day-line .day-hours span').length));
  const hour = now.getHours() + now.getMinutes() / 60;
  const mark = $('.day-now');
  // The line is from 8 AM to 6 PM, and longer when an event is before or after that.
  const firstHour = Number(line?.dataset.first);
  const lastHour = Number(line?.dataset.last);
  check('A mark shows where now is (in the hours of the line)', firstHour <= 8 && lastHour >= 18 && (hour < firstHour || hour > lastHour ? !mark : mark && Math.abs(parseFloat(mark.style.left) - ((hour - firstHour) / (lastHour - firstHour)) * 100) < 3), `${mark ? mark.style.left : 'no mark'} on a line from ${firstHour} to ${lastHour}`);
  const cards = $$('.today-card');
  check('The pages last worked in are cards with their first words', cards.length >= 1 && cards.length <= 3 && cards.every((c) => c.querySelector('.today-card-title')?.textContent && c.querySelector('.today-card-meta')?.textContent.startsWith('Edited')) && cards.some((c) => c.querySelector('.today-card-text')?.textContent.length > 10), cards.map((c) => c.querySelector('.today-card-title')?.textContent).join(' | '));
  check('The to-dos and what comes up are at the side', $('#home-todos') && $$('.home.today .side-card').length === 2);
  await wait(150);
  snap('today');
  const jot = $('.jot-input');
  jot.value = 'Call the dentist';
  jot.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(60);
  key(jot, 'Enter');
  check('Jotting a line saves it as a page', await until(() => treeItem('Call the dentist')));
  await wait(300);
  snap('home');
});

await run('Drawing block (Excalidraw)', async () => {
  const title = await newPage('Sketches');
  key(title, 'Enter');
  const b = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(b, '/drawing');
  const item = await until(() => $$('.slash-menu .menu-item').find((i) => i.textContent.includes('Drawing')));
  check('The slash menu offers a Drawing', item);
  item.click();
  let box = await until(() => $('.drawing'), 3000);
  check('Choosing it adds a drawing block', box);
  check('Typing carries on in a new line after it', await until(() => document.activeElement?.classList.contains('blk')));
  check('The drawing frame loads Excalidraw', await until(() => box.dataset.ready === '1', 20000));
  let frame = box.querySelector('iframe');
  frame.contentWindow.postMessage({ n2draw: 'probe' }, '*');
  let info = JSON.parse((await until(() => box.dataset.probe, 8000)) || '{}');
  check('Excalidraw draws its canvas', info.canvas, JSON.stringify(info));
  check('Its hand-drawn font loads from inside the app', info.font, JSON.stringify(info));
  check('It has the expand button', info.expandButton, JSON.stringify(info));
  const cover = box.querySelector('.drawing-cover');
  check('Until clicked, the page scrolls past it', getComputedStyle(cover).display !== 'none');
  cover.click();
  check('Clicking it starts drawing', box.dataset.active === '1' && getComputedStyle(cover).display === 'none');
  frame.contentWindow.postMessage({ n2draw: 'test-add' }, '*');
  check('A new shape is saved', await until(() => Number(box.dataset.saves) >= 1, 6000), box.dataset.saves);
  frame.contentWindow.postMessage({ n2draw: 'test-expand' }, '*');
  check('Expand makes the drawing full screen', await until(() => box.dataset.expanded === '1', 3000));
  await wait(260);
  const pr = box.closest('.pane').getBoundingClientRect();
  const br = box.getBoundingClientRect();
  const fr = box.querySelector('iframe').getBoundingClientRect();
  check('It fills the whole window, over the sidebar and tabs', br.left <= 1 && br.top <= 1 && br.width >= innerWidth - 2 && br.height >= innerHeight - 2 && pr.width < innerWidth, `${Math.round(br.left)},${Math.round(br.top)} ${Math.round(br.width)}x${Math.round(br.height)} in ${innerWidth}x${innerHeight}`);
  check('The window buttons keep a clear strip on top', fr.top >= 30 && onTop(box.querySelector('iframe')), Math.round(fr.top));
  $('.pagebar').dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
  check('Clicking outside puts it back', await until(() => !box.dataset.expanded && !box.dataset.active));
  // Leave the page and come back: the shape is still there.
  $$('.tile').find((t) => t.title === 'Home').click();
  await until(() => !$('.drawing'));
  treeItem('Sketches').click();
  box = await until(() => $('.drawing'), 3000);
  check('Coming back reopens the drawing', box && (await until(() => box.dataset.ready === '1', 20000)));
  frame = box.querySelector('iframe');
  await wait(300);
  frame.contentWindow.postMessage({ n2draw: 'probe' }, '*');
  info = JSON.parse((await until(() => box.dataset.probe, 8000)) || '{}');
  check('The saved shape is back on the canvas', info.elements === 1, JSON.stringify(info));
  await oneTab();
});

await run('Many pages in one tab, on any side', async () => {
  await oneTab();
  treeItem('Getting Started').click();
  await until(() => $$('.tab').length === 2);
  $$('.tab')[0].querySelector('.tab-close').click();
  await until(() => $$('.tab').length === 1 && $$('.pane').length === 1);
  const paneOf = (title) => $$('.pane').find((p) => p.querySelector('.title-input')?.value === title);
  const titles = () => $$('.pane').map((p) => p.querySelector('.title-input')?.value || '');
  // A point just inside a pane's edge, and the element there.
  const near = (pane, edge) => {
    const r = pane.getBoundingClientRect();
    const [x, y] = {
      left: [r.left + 8, r.top + r.height / 2],
      right: [r.right - 8, r.top + r.height / 2],
      top: [r.left + r.width / 2, r.top + 52],
      bottom: [r.left + r.width / 2, r.bottom - 8],
    }[edge];
    const el = document.elementFromPoint(x, y);
    return [el && pane.contains(el) ? el : pane, x, y];
  };
  await dragAlong(treeItem('Reading List'), [near(paneOf('Getting Started'), 'right')]);
  check("A page dropped on a pane's right edge opens to its right", await until(() => $$('.pane').length === 2 && $('.split-group.split-row')));
  await dragAlong(treeItem('Typing test'), [near(paneOf('Reading List'), 'bottom')]);
  check('…on its bottom edge, below it', await until(() => $$('.pane').length === 3 && $('.split-group.split-col')));
  const rb = paneOf('Reading List').getBoundingClientRect();
  const tb = paneOf('Typing test').getBoundingClientRect();
  check('The new page sits under the one it was dropped on', tb.top >= rb.bottom - 2 && Math.abs(tb.left - rb.left) < 2, `${rb.bottom} / ${tb.top}`);
  await dragAlong(treeItem('Call the dentist'), [near(paneOf('Getting Started'), 'top')]);
  check('…on its top edge, above it', await until(() => $$('.pane').length === 4 && paneOf('Call the dentist')?.getBoundingClientRect().bottom <= paneOf('Getting Started')?.getBoundingClientRect().top + 2));
  await dragAlong(treeItem('Sketches'), [near(paneOf('Getting Started'), 'left')]);
  check('…on its left edge, to its left (five pages, no limit)', await until(() => $$('.pane').length === 5 && paneOf('Sketches')?.getBoundingClientRect().right <= paneOf('Getting Started')?.getBoundingClientRect().left + 2));
  check('Dragging over a pane shows where it will land, then hides', !$('.pane-drop.show'));
  check('The tab shows a part for every page', $$('.tab.split .tab-seg').length === 5 && $('.tab.split').title.split(' | ').length === 5, $('.tab.split')?.title);
  await wait(250);
  snap('five-panes');
  // Resize: drag the line between the two columns of the top row.
  const sash = $('.split-group.split-row > .split-cell > .sash');
  const cell = sash.parentElement.previousElementSibling;
  const before = cell.getBoundingClientRect().width;
  const sr = sash.getBoundingClientRect();
  sash.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: sr.left + 4, clientY: sr.top + 40 }));
  const pull = Math.min(60, Math.max(10, Math.round(before - 210)));
  document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: sr.left + 4 - pull, clientY: sr.top + 40 }));
  await wait(60);
  document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: sr.left + 4 - pull, clientY: sr.top + 40 }));
  check('Dragging the line between panes resizes them', await until(() => Math.abs(cell.getBoundingClientRect().width - (before - pull)) < 12), `${before} - ${pull} -> ${cell.getBoundingClientRect().width}`);
  // Move a pane by dragging its bar onto another pane's edge.
  const order = titles();
  await dragAlong(paneOf('Typing test').querySelector('.pagebar'), [near(paneOf('Sketches'), 'left')]);
  const movedLeftOf = () => {
    const t = paneOf('Typing test')?.getBoundingClientRect();
    const k = paneOf('Sketches')?.getBoundingClientRect();
    return t && k && t.right <= k.left + 2 && Math.abs(t.top - k.top) < 2;
  };
  const box = (t) => {
    const r = paneOf(t)?.getBoundingClientRect();
    return r ? `${t}[${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}x${Math.round(r.height)}]` : `${t}[none]`;
  };
  check('Dragging a pane by its bar moves it', await until(() => titles().length === 5 && movedLeftOf()), `${order.join(',')} -> ${titles().join(',')} | ${box('Typing test')} ${box('Sketches')} ${box('Call the dentist')}`);
  // Pull one page out of the tab by its part of the tab.
  const seg = $$('.tab.split .tab-seg').find((s) => s.textContent.includes('Call the dentist'));
  const card = $('.work-card');
  const bar = $('.tabs').getBoundingClientRect();
  await dragAlong(seg, [[card, card.getBoundingClientRect().left + 30, bar.bottom + 80], centre($('.new-tab'))]);
  check('Pulling a part out of the tab gives that page its own tab', await until(() => $$('.tab').length === 2 && $$('.pane').length >= 1 && $$('.tab')[1].textContent.includes('Call the dentist')));
  $$('.tab')[0].click();
  await until(() => $$('.pane').length === 4);
  // Close a pane from its bar.
  paneOf('Reading List').querySelector('.pagebar .icon-btn[title="Close pane"]').click();
  check('Closing a pane lets the others take its space', await until(() => $$('.pane').length === 3 && !paneOf('Reading List')));
  await oneTab();
});

await run('Split view', async () => {
  treeItem('Typing test').click();
  await until(() => $('.pagebar .crumb.current')?.textContent.includes('Typing test'));
  const row = treeItem('Getting Started');
  row.querySelector('.tree-actions .row-btn').click();
  const menu = await until(() => $('.sidebar-menu'));
  menuItem(menu, 'Open in split view').click();
  const panes = await until(() => $('.panes.split'));
  check('Open in split view shows two pages side by side', panes && $$('.pane').length === 2 && $$('.pane .page').length === 2);
  check('The new pane has focus', $$('.pane')[1]?.classList.contains('focused'));
  await wait(250);
  snap('split');

  // Fold the right pane to a rail: pull its divider to the right edge.
  const line = $('.split-group.split-row > .split-cell > .sash');
  const right = line.parentElement;
  const left = right.previousElementSibling;
  const group = line.closest('.split-group').getBoundingClientRect();
  const wide = right.getBoundingClientRect().width;
  const lr = line.getBoundingClientRect();
  const y = lr.top + 60;
  const pullTo = async (x) => {
    document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: x, clientY: y }));
    await wait(80);
  };
  line.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: lr.left + 4, clientY: y }));
  await pullTo(group.right - 230);
  check('A pane holds at its smallest size for a short pull', !right.classList.contains('railed') && right.getBoundingClientRect().width >= 195, Math.round(right.getBoundingClientRect().width));
  await pullTo(group.right - 60);
  check('Pulled on to the edge, it folds to a slim rail', await until(() => right.classList.contains('railed') && right.querySelector('.pane-rail')));
  document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: group.right - 60, clientY: y }));
  await wait(200);
  const railBox = right.getBoundingClientRect();
  check('The rail is 28 wide and names its page', Math.abs(railBox.width - 28) <= 2 && right.querySelector('.rail-title')?.textContent === 'Getting Started', `${Math.round(railBox.width)} ${right.querySelector('.rail-title')?.textContent}`);
  check('The other pane takes the room', Math.abs(left.getBoundingClientRect().width + railBox.width - group.width) <= 3, `${Math.round(left.getBoundingClientRect().width)} of ${Math.round(group.width)}`);
  await wait(200);
  snap('rail');
  right.querySelector('.pane-rail').click();
  check('One click opens it at the size it had', await until(() => !right.classList.contains('railed') && Math.abs(right.getBoundingClientRect().width - wide) <= 3), `${Math.round(wide)} -> ${Math.round(right.getBoundingClientRect().width)}`);
  // The same from the page menu, then open by pulling the divider back out.
  $$('.pane')[1].querySelector('.pagebar .icon-btn[title="Page options"]').click();
  menuItem(await until(() => $('.page-menu')), 'Minimize pane').click();
  check('"Minimize pane" in the page menu folds it too', await until(() => right.classList.contains('railed')));
  await wait(120);
  const l2 = right.querySelector('.sash').getBoundingClientRect();
  right.querySelector('.sash').dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: l2.left + 4, clientY: y }));
  await pullTo(group.right - 60);
  check('A short pull leaves a rail closed', right.classList.contains('railed'));
  await pullTo(group.right - 260);
  check('Pulling its divider back out opens it', await until(() => !right.classList.contains('railed')));
  document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: group.right - 260, clientY: y }));
  await wait(200);
  check('It is at least its smallest size again', right.getBoundingClientRect().width >= 195, Math.round(right.getBoundingClientRect().width));

  $$('.pane')[1].querySelector('.pagebar .icon-btn[title="Close pane"]').click();
  check('Closing the split leaves one page', await until(() => !$('.panes.split') && $$('.pane').length === 1));
});

await run('Find in page', async () => {
  await oneTab();
  treeItem('Getting Started').click();
  // Its own page, not the one that was in front before the click.
  await until(() => $('.pane .title-input')?.value === 'Getting Started' && $('.pane .blk'));
  window.__n2.find();
  const input = await until(() => $('#find-bar.show') && document.activeElement?.id === 'find-input' && $('#find-input'));
  check('⌘F opens the find bar with the cursor in it', input);
  input.value = 'the';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  const count = await until(() => /^1 of \d+$/.test($('#find-count').textContent) && $('#find-count').textContent);
  check('It counts the matches', count, $('#find-count').textContent);
  check('The matches are marked on the page', typeof CSS === 'undefined' || !CSS.highlights || CSS.highlights.has('n2-find'));
  key(input, 'Enter');
  check('Return goes to the next match', await until(() => $('#find-count').textContent.startsWith('2 of')), $('#find-count').textContent);
  key(input, 'Enter', { shiftKey: true });
  check('Shift-Return goes back', await until(() => $('#find-count').textContent.startsWith('1 of')), $('#find-count').textContent);
  input.value = 'zzqqxx';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  check('No matches is said plainly', await until(() => $('#find-count').textContent === 'No matches'));
  key(input, 'Escape');
  check('Escape closes it and clears the marks', await until(() => !$('#find-bar.show')) && !(window.CSS && CSS.highlights && CSS.highlights.has('n2-find')));
});

await run('Page history', async () => {
  const pane = await until(() => $$('.pane').find((p) => p.querySelector('.title-input')?.value === 'Getting Started' && p.offsetParent));
  const blk = pane.querySelector('.blk');
  blk.focus();
  T().setSel(blk, 0);
  typeIn(blk, 'ZZZ ');
  await until(() => blk.textContent.startsWith('ZZZ'));
  await wait(250);
  pane.querySelector('.pagebar .icon-btn[title="Page options"]').click();
  const menu = await until(() => $('.page-menu'));
  menuItem(menu, 'Page history').click();
  const modal = await until(() => $('.modal.history'));
  check('Page history opens from the page menu', modal);
  check('It lists the page as it was before the edit', await until(() => $$('.history-item').length >= 1), $$('.history-item').length);
  check('It shows a preview of that version', $('.history-preview .hp-title') && !$('.history-preview').textContent.includes('ZZZ'));
  $('#restore-version').click();
  check('Restoring puts the page back', await until(() => !$('.modal.history') && !pane.querySelector('.blk').textContent.startsWith('ZZZ')), pane.querySelector('.blk')?.textContent.slice(0, 40));
  pane.querySelector('.pagebar .icon-btn[title="Page options"]').click();
  menuItem(await until(() => $('.page-menu')), 'Page history').click();
  await until(() => $('.modal.history'));
  check('The page as it was before restoring is kept too', await until(() => $$('.history-item').length >= 2), $$('.history-item').length);
  snap('history');
  key(document.body, 'Escape');
  $('.modal-backdrop')?.click();
  await until(() => !$('.modal.history'));
});

await run('Backups', async () => {
  await openSettings();
  await settingsSection('Data');
  const btn = await until(() => $('#back-up-now'));
  check('Settings → Data offers a backup', btn);
  btn.click();
  check('Back up now makes a copy', await until(() => $$('.set-note').some((n) => n.textContent.includes('Last copy: just now')), 6000), $$('.set-note').map((n) => n.textContent).join(' | ').slice(0, 300));
  check('Export and import are offered', $$('.set-label').some((l) => l.textContent === 'Export') && $$('.set-label').some((l) => l.textContent === 'Import'));
  const updates = $$('.set-group').find((g) => g.querySelector('h2')?.textContent === 'Updates');
  const about = updates?.querySelector('.set-note')?.textContent || '';
  check('Settings → Data says which build this is and how it updates', updates && /^Version \d/.test(about) && $('#update-auto')?.classList.contains('on'), about);
  check('A build from the build folder says it does not update itself', about.includes('does not update itself') && !$('#update-now') && !$('#update-ready'), about);
  snap('settings-data');
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
});

await run('Connect AI tools', async () => {
  await openSettings();
  await settingsSection('AI');
  const group = await until(() => $$('.set-group').find((g) => g.querySelector('h2')?.textContent === 'Connect AI tools'));
  check('Settings → AI has the connector for other AI tools', group);
  const kinds = $$('[data-copy]', group || document).map((b) => b.dataset.copy).join(',');
  check('It offers Claude Code, Codex, JSON and the plain command', kinds === 'claude-code,codex,json,command', kinds);
  const shown = $$('.set-note.mono', group || document).map((n) => n.textContent).join(' ');
  check('It shows the command, which ends in --mcp', /notion2' --mcp$/.test(shown.trim()), shown);
  check('AI from anywhere has its switch, on', $('#voice-anywhere')?.classList.contains('on') && $('#voice-anywhere').getAttribute('aria-label') === 'AI from anywhere');
  await wait(150);
  snap('settings-connector');
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
});

await run('Drags are private to the app', async () => {
  const row = await until(() => treeItem('Getting Started'));
  const dt = new DataTransfer();
  row.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
  const types = [...dt.types];
  row.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
  await wait(100);
  check('A page drag carries nothing other apps can take', types.includes('application/x-notion2-drag') && !types.includes('text/plain'), types.join(', '));
});

await run('Meeting notes', async () => {
  await oneTab();
  // Stand in for the watcher noticing a Teams meeting.
  const dated = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  T().command('meeting_started');
  const pane = await until(() => $$('.pane').find((p) => dated.test(p.querySelector('.title-input')?.value || '') && p.offsetParent));
  check('A meeting start makes a notes page named with the date', pane, $$('.pane .title-input').map((t) => t.value).join(' | '));
  if (!pane) return;
  const line = () => pane.querySelector('.blk')?.textContent || '';
  check('Its first line is the day, the time and the app', line().includes(String(new Date().getFullYear())) && /\d (AM|PM)/.test(line()) && line().includes('Microsoft Teams'), line());
  const heads = $$('.blk', pane).map((b) => b.textContent);
  check('It has Notes and Action items', heads.includes('Notes') && heads.includes('Action items'), heads.join(' | '));
  check('The cursor waits in the first note', await until(() => document.activeElement?.classList.contains('blk') && pane.contains(document.activeElement) && document.activeElement.textContent === ''));
  const listed = () => $$('.sidebar .tree-item .tree-title').filter((t) => dated.test(t.textContent)).length;
  check('It is kept under "Meeting notes" in the sidebar', treeItem('Meeting notes') && listed() === 1, listed());
  T().command('meeting_started');
  await wait(400);
  check('The same meeting does not make a second page', listed() === 1, listed());
  typeIn(document.activeElement, 'Agenda first');
  await wait(300);
  T().command('meeting_ended');
  check('When it ends, the line gets the end time and length', await until(() => /\(\d+ min\)/.test(line())), line());
  snap('meeting-notes');
  // A call the person wants no notes of: "Not now" on the page, and no page is kept.
  T().command('forget_meeting');
  const tabsBefore = $$('.tab').length;
  T().command('meeting_started');
  const asked = await until(() => $$('.pane').find((p) => p.offsetParent && $('#listen-no', p)), 4000);
  check('With no pad, the page of the call asks itself', asked && listed() === 2, listed());
  $('#listen-no', asked)?.click();
  check('"Not now" on an empty page of a call takes the page away: no page is kept', await until(() => listed() === 1 && $$('.tab').length === tabsBefore, 3000), `${listed()} pages, ${$$('.tab').length - tabsBefore} new tabs`);
  T().command('meeting_ended');
  // A call nobody says yes to: at its end, its empty page goes.
  T().command('forget_meeting');
  T().command('meeting_started');
  await until(() => listed() === 2, 4000);
  T().command('meeting_ended');
  check('A call with no yes and nothing written leaves no page when it ends', await until(() => listed() === 1, 3000), listed());
  T().command('forget_meeting');
});

await run('Speak a page', async () => {
  await oneTab();
  const made = () => $$('.sidebar .tree-item .tree-title').filter((t) => t.textContent === 'Shop and call list').length;
  check('The sidebar has no AI tile and no second microphone', $$('.sidebar .tile').map((t) => t.title.replace(/ \(.*/, '')).join(', ') === 'Home, Calendar, Graph: how your pages connect', $$('.sidebar .tile').map((t) => t.title).join(', '));
  speakPage();
  const box = await until(() => $('.modal.voice #voice-text'));
  check('⇧⌘D opens the voice box with the cursor in it', box && (await until(() => document.activeElement === box)));
  check('There is no page to make before there are words', $('#voice-create')?.disabled);
  // Dictation types into the box; here the test does.
  box.value = 'buy milk and call Sam about Friday';
  box.dispatchEvent(new Event('input', { bubbles: true }));
  check('With words in it, it counts down the pause', await until(() => $('.voice-wait-bar') && !$('#voice-create').disabled));
  await wait(150);
  snap('voice');
  $('#voice-create').click();
  const pane = await until(() => $$('.pane').find((p) => p.querySelector('.title-input')?.value === 'Shop and call list' && p.offsetParent), 6000);
  check('The AI names the new page', pane, $$('.pane .title-input').map((t) => t.value).join(' | '));
  if (!pane) return;
  const lines = () => $$('.blk', pane).map((b) => b.textContent);
  check('It writes the page with a heading and to-dos', await until(() => lines().includes('To do') && lines().includes('Buy milk') && lines().includes('Call Sam about Friday'), 6000), lines().join(' | '));
  check('The words as spoken are kept under a toggle', await until(() => lines().includes('What you said'), 4000), lines().join(' | '));
  check('It says when the page is ready', await until(() => $('.toast')?.textContent === 'Your page is ready', 4000), $('.toast')?.textContent);
  check('The voice box has closed', !$('.modal.voice'));
  await wait(200);
  snap('voice-page');
  // The microphone on an empty line writes into the page that is open.
  await oneTab();
  const pagesBefore = $$('.sidebar .tree-item').length;
  const heading = await newPage('Spoken into');
  key(heading, 'Enter');
  const line = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const mic = await until(() => $('#line-mic.show'));
  const lr = line.getBoundingClientRect();
  const mr = mic ? mic.getBoundingClientRect() : { top: -99, bottom: -99, left: -99 };
  check('An empty line shows a microphone beside its hint', mic && mr.top >= lr.top - 2 && mr.bottom <= lr.bottom + 2 && mr.left > lr.left + 100 && onTop(mic), `${Math.round(mr.left)},${Math.round(mr.top)} on ${Math.round(lr.left)},${Math.round(lr.top)}`);
  mic.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
  mic.click();
  const hereBox = await until(() => $('.modal.voice #voice-text'));
  check('It opens the voice box for this page', hereBox && $('.modal.voice h2')?.textContent === 'Speak into this page' && $('#voice-create')?.textContent === 'Write here', $('.modal.voice h2')?.textContent);
  hereBox.value = 'buy milk and call Sam about Friday';
  hereBox.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => !$('#voice-create').disabled);
  $('#voice-create').click();
  const into = () => $$('.pane').find((p) => p.querySelector('.title-input')?.value === 'Spoken into' && p.offsetParent);
  const wrote = () => $$('.blk', into() || document.body).map((b) => b.textContent);
  check('The AI writes what was said into that page, at that line', await until(() => wrote().includes('Buy milk') && wrote().includes('To do') && wrote()[0] === 'For the weekend.', 6000), wrote().join(' | '));
  check('It stays one page: no new page is made', $$('.sidebar .tree-item').length === pagesBefore + 1 && !wrote().includes('What you said'), `${pagesBefore} -> ${$$('.sidebar .tree-item').length}`);
  check('The microphone goes once the line has words', await until(() => !$('#line-mic.show')));
  await wait(300);
  snap('voice-here');
  key(document.activeElement, 'z', { metaKey: true });
  check('One undo takes it all out again', await until(() => !wrote().includes('Buy milk') && wrote().join('') === '', 3000), wrote().join(' | '));
  // Again, without pressing anything: a pause makes the page.
  speakPage();
  const again = await until(() => $('.modal.voice #voice-text'));
  again.value = 'one more note for the list';
  again.dispatchEvent(new Event('input', { bubbles: true }));
  check('A pause in speech makes the page without a click', await until(() => made() === 2, 9000), made());
});

// What is "said" here is typed, and "[[reply: …]]" in it is what the stand-in
// AI answers (see ai.rs), so each check can have the AI say what it needs.
const say = async (text) => {
  const box = await until(() => $('.modal.voice #voice-text'));
  box.value = text;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => !$('#voice-create').disabled);
  $('#voice-create').click();
  await until(() => !$('.modal.voice'));
};
const shownPane = (title) => $$('.pane').find((p) => p.querySelector('.title-input')?.value === title && p.offsetParent);

await run('The AI from anywhere', async () => {
  await oneTab();
  $$('.tile').find((t) => t.title === 'Home').click();
  await until(() => $('.home'));
  T().command('ai_anywhere');
  const ask = await until(() => $('#ask-notes-input'));
  check('With no page in front, ⌃⌥Space opens Ask your notes with the cursor in it', ask && (await until(() => document.activeElement === ask)) && !$('.modal.voice'), document.activeElement?.id);
  ask.value = 'half a question';
  ask.dispatchEvent(new Event('input', { bubbles: true }));
  T().command('ai_anywhere');
  await wait(250);
  check('A second press leaves it open, with what was typed', $('#ask-notes-input')?.value === 'half a question' && document.activeElement?.id === 'ask-notes-input', $('#ask-notes-input')?.value);
  key(document.body, 'Escape');
  $('.modal-backdrop')?.click();
  await until(() => !$('#ask-notes-input'));
  treeItem('Getting Started').click();
  await until(() => shownPane('Getting Started'));
  T().command('ai_anywhere');
  const box = await until(() => $('#ai-input'));
  check('On a page, it opens the AI there with the cursor in it', box && (await until(() => document.activeElement === box)) && !$('.modal.voice'), document.activeElement?.id || document.activeElement?.className);
  check('The AI box has the one microphone', $$('[data-dictate="ai-input"]').length === 1);
  key(box, 'Escape');
  await until(() => !$('#ai-input'));
});

// Only when asked for (NOTION2_REAL_DICTATION): does the Mac's Dictation
// start from a microphone beside a text box, and type what it hears there?
if (window.__n2_real_dictation) {
  await run('Real Dictation probe', async () => {
    await oneTab();
    $$('.tile').find((t) => t.title === 'Home').click();
    const jot = await until(() => $('#jot-input'));
    const mic = $('[data-dictate="jot-input"]');
    mic.click();
    await until(() => document.body.dataset.dictationAsked, 3000);
    check('The app asked macOS to start Dictation, with the caret in the box', document.body.dataset.dictationAsked === 'true' && document.activeElement === jot, `asked: ${document.body.dataset.dictationAsked}, focus in window: ${document.hasFocus()}, active: ${document.activeElement?.id}`);
    const typed = await until(() => jot.value.trim().length > 3, 16000);
    check('Dictation typed what it heard into the box', !!typed, `"${jot.value}" | focus in window: ${document.hasFocus()}`);
  });
}

await run('Zoom keeps the page in the middle', async () => {
  await oneTab();
  T().command('zoom_reset');
  treeItem('Getting Started').click();
  const pane = await until(() => shownPane('Getting Started'));
  // Where things are: the zoomed box against the card it is in, and the
  // column of the page against its pane.
  const where = () => {
    const card = $('.work-card').getBoundingClientRect();
    const box = $('.card-zoom').getBoundingClientRect();
    const column = $('.page', pane).getBoundingClientRect();
    const p = pane.getBoundingClientRect();
    return {
      wider: Math.round(box.width - card.width),
      taller: Math.round(box.height - card.height),
      off: Math.round((column.left + column.right) / 2 - (p.left + p.right) / 2),
      paneOver: Math.round(p.right - card.right),
      letters: Math.round($('.title-input', pane).getBoundingClientRect().height),
    };
  };
  const normal = where();
  // As at the normal size: the box as big as its card, the column as far
  // from the middle of its pane (a few points, for the scroll bar), drawn
  // `zoom` times as big.
  const inPlace = (w, zoom = 1) => Math.abs(w.wider - normal.wider) <= 3 && Math.abs(w.taller - normal.taller) <= 3 && Math.abs(w.off - normal.off * zoom) <= 4 && w.paneOver <= 1;
  check('At the normal size the page is in the middle of its pane', Math.abs(normal.wider) <= 3 && Math.abs(normal.taller) <= 3 && Math.abs(normal.off) <= 8 && normal.paneOver <= 1, JSON.stringify(normal));
  for (let i = 0; i < 3; i++) T().command('zoom_out');
  await wait(350);
  const small = where();
  check('Zoomed out, the letters are smaller', small.letters < normal.letters, `${normal.letters} → ${small.letters}`);
  check('Zoomed out, the page fills its card and stays in the middle', inPlace(small, 0.7), JSON.stringify(small));
  snap('zoomed-out');
  T().command('zoom_reset');
  for (let i = 0; i < 3; i++) T().command('zoom_in');
  await wait(350);
  const big = where();
  check('Zoomed in, the letters are bigger', big.letters > normal.letters, `${normal.letters} → ${big.letters}`);
  check('Zoomed in, the page fills its card and stays in the middle', inPlace(big, 1.3), JSON.stringify(big));
  T().command('zoom_reset');
  await wait(200);
  check('Back at the normal size, it is as it was', JSON.stringify(where()) === JSON.stringify(normal), JSON.stringify(where()));
});

await run('Pictures, tables and covers', async () => {
  await oneTab();
  const title = await newPage('Budget');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Budget');

  // A screenshot on the clipboard, pasted at an empty line.
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), (c) => c.charCodeAt(0));
  const clip = new DataTransfer();
  clip.items.add(new File([png], 'Screenshot.png', { type: 'image/png' }));
  first.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const img = await until(() => $('.image-block img', pane()), 4000);
  check('A pasted screenshot becomes a picture in the page', img && img.src.startsWith('n2file://'), img?.src);
  check('The picture is kept with the workspace and shows', await until(() => img.complete && img.naturalWidth === 1, 5000), `${img?.complete} ${img?.naturalWidth}`);
  check('Typing carries on in a line after the picture', await until(() => document.activeElement?.classList.contains('blk') && pane().contains(document.activeElement)));
  const under = () => $('.image-block', pane()).closest('.block-row').nextElementSibling;
  check('The typing line is the empty line right under the picture', document.activeElement.closest('.block-row') === under() && document.activeElement.textContent.replace(/\u200b/g, '') === '', document.activeElement.textContent);

  // The slash menu: an empty picture box, then a table.
  typeIn(document.activeElement, '/image');
  (await until(() => $$('.slash-menu .menu-item').find((i) => i.textContent.includes('Image')))).click();
  const holder = await until(() => $('.image-empty', pane()));
  check('The slash menu adds a box to choose a picture with', holder && /Choose a picture/.test(holder.textContent));
  const clip2 = new DataTransfer();
  clip2.items.add(new File([png], 'Another.png', { type: 'image/png' }));
  holder.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip2, bubbles: true, cancelable: true }));
  check('A paste on that box fills it', await until(() => $$('.image-block img', pane()).length === 2 && !$('.image-empty', pane()), 4000), String($$('.image-block img', pane()).length));

  const line = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(line, '/table');
  (await until(() => $$('.slash-menu .menu-item').find((i) => i.textContent.includes('Table')))).click();
  const table = await until(() => $('.table-block', pane()));
  const cells = () => $$('.cell', table);
  const grid = () => $$('tr', table).map((tr) => $$('.cell', tr).map((c) => c.value).join('|')).join(' / ');
  check('The slash menu adds a table: a header and two rows, three columns', table && $$('tr', table).length === 3 && cells().length === 9, grid());
  const put = (cell, text) => {
    cell.focus();
    cell.value = text;
    cell.dispatchEvent(new Event('input', { bubbles: true }));
  };
  put(cells()[0], 'Item');
  key(document.activeElement, 'Tab');
  check('Tab goes to the next cell', await until(() => document.activeElement === cells()[1]));
  put(cells()[1], 'Amount');
  put(cells()[3], 'Rent');
  put(cells()[4], '$2,000');
  check('Cells keep what is typed', await until(() => grid() === 'Item|Amount| / Rent|$2,000| / ||'), grid());
  // Letters typed in one cell without a pause are one step for Undo, as in a line of text.
  put(cells()[6], 'W');
  put(cells()[6], 'Wa');
  put(cells()[6], 'Wat');
  await until(() => grid() === 'Item|Amount| / Rent|$2,000| / Wat||');
  T().command('undo');
  check('Letters typed in a cell without a pause are one step for Undo', await until(() => grid() === 'Item|Amount| / Rent|$2,000| / ||', 3000), grid());
  T().command('undo');
  check('…and the step before it is the cell before it', await until(() => grid() === 'Item|Amount| / Rent|| / ||', 3000), grid());
  T().command('redo');
  await until(() => grid() === 'Item|Amount| / Rent|$2,000| / ||', 3000);
  cells()[8].focus();
  key(document.activeElement, 'Tab');
  check('Tab past the last cell adds a row and goes to its start', await until(() => $$('tr', table).length === 4 && document.activeElement === cells()[9], 3000), `${$$('tr', table).length} rows, at ${cells().indexOf(document.activeElement)}`);
  await columnDo(table, 2, 'delete');
  check('The menu of a column takes the column out', await until(() => grid() === 'Item|Amount / Rent|$2,000 / | / |', 3000), grid());
  await columnDo(table, 1, 'right');
  check('…and puts a new column after it, with the caret in its heading', await until(() => grid() === 'Item|Amount| / Rent|$2,000| / || / ||' && document.activeElement === cells()[2], 3000), grid());
  await rowDo(table, 3, 'delete');
  check('The menu of a row takes the row out', await until(() => $$('tr', table).length === 3, 3000), grid());
  cells()[3].focus();
  key(document.activeElement, 'Enter');
  check('Return goes to the cell below', await until(() => document.activeElement === cells()[6]));
  await wait(200);
  check('The table knows that a cell has the caret', table.classList.contains('editing'));
  document.activeElement.blur();
  check('…and go when the caret leaves the table', await until(() => !table.classList.contains('editing')));
  T().command('copy_markdown');
  await wait(200);

  // Covers.
  check('A page starts with no cover', !$('.page-cover', pane()));
  $('#add-cover').click();
  const cover = await until(() => $('.page-cover', pane()));
  const picker = await until(() => $('.cover-picker'));
  check('Add cover puts one of the app’s own covers on and opens the covers to choose from', cover && picker && /gradient|rgb/.test(getComputedStyle(cover).backgroundImage + getComputedStyle(cover).backgroundColor), getComputedStyle(cover || document.body).backgroundImage.slice(0, 60));
  check('The covers are the app’s own colours and gradients, each with a name', $$('.cover-swatch').length === 16 && $$('.cover-swatch').every((s) => s.title), String($$('.cover-swatch').length));
  $('[data-cover-preset="sea"]').click();
  check('Choosing one changes the cover and marks it', await until(() => $('[data-cover-preset="sea"]').classList.contains('on') && getComputedStyle($('.page-cover', pane())).backgroundImage.includes('106, 169, 189')), getComputedStyle($('.page-cover', pane())).backgroundImage.slice(0, 80));
  check('A picture of your own can be used', $('#cover-upload'));
  await wait(250);
  snap('cover');
  key(document.body, 'Escape');
  await until(() => !$('.cover-picker'));
  check('The cover stays, with the page under it', $('.page-cover', pane()) && $('.page.has-cover', pane()));
  $('#cover-change').click();
  (await until(() => $('#cover-remove'))).click();
  check('Remove takes the cover away', await until(() => !$('.page-cover', pane()) && !$('.cover-picker')));
});

await run('Typed tables and charts', async () => {
  await oneTab();
  const title = await newPage('Run log check');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Run log check');
  const md = [
    '| Date | Run | km | Time | Pace | Stretch |',
    '| --- | --- | --- | --- | --- | --- |',
    '| 2026-11-10 | Easy | 3.0 | 19:30 |  | x |',
    '| 2026-11-12 | Easy | 3.0 | 19:12 |  |  |',
    '| 2026-11-15 | Long | 3.5 | 23:05 |  | x |',
    '<!-- columns: date count | choice | number km sum | time sum | pace average | checkbox checked -->',
    '',
    'Notes',
  ].join('\n');
  const clip = new DataTransfer();
  clip.setData('text/plain', md);
  first.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const box = await until(() => $('.table-block.typed', pane()), 4000);
  check('A pasted table with a columns line keeps the kinds of its columns', box && $$('.th-kind', box).length === 6, box ? String($$('.th-kind', box).length) : 'no typed table');
  const wrap = () => box.closest('.table-wrap');
  const paces = () => $$('.cell.computed', box).map((c) => c.value).join(' ');
  check('The pace is worked out from the distance and the time', paces() === '6:30 6:24 6:36', paces());
  check('A choice is a tag and a tick is a box', $$('.cell.pick .tag', box).length === 3 && $$('.cell-check', box).length === 3 && $$('.cell-check.on', box).length === 2);
  const totals = () => $$('.table-totals td', wrap()).map((t) => t.textContent.trim()).join(' | ');
  check('The totals show under the table', totals() === '3 rows |  | Sum 9.5 | Sum 1:01:47 | Avg 6:30 | 2 of 3', totals());
  $$('.cell-check', box)[1].click();
  check('A click ticks a box, and the total follows', await until(() => $$('.cell-check.on', box).length === 3 && totals().endsWith('3 of 3')), totals());
  check('No system list anywhere in the table', !$('select', wrap()));

  // The menu of a column: Rune's own rows, the chosen one grey.
  const kmMenu = $$('.th-menu', box)[2];
  kmMenu.click();
  const menu = await until(() => $('.column-menu', box));
  check('A column has a menu of kinds and totals', menu && $('[data-item="type"]', menu)?.textContent.includes('Number') && $('[data-item="total"]', menu)?.textContent.includes('Sum'));
  check('The menu is not cut by the table', menu && onTop(menu), menu ? JSON.stringify(menu.getBoundingClientRect()) : '');
  await wait(200);
  snap('column-menu');
  $('[data-item="total"]', menu).click();
  (await until(() => $('.column-sub [data-item="average"]', menu))).click();
  check('Choosing a total changes the foot of that column', await until(() => totals().includes('Avg 3.2')), totals());
  (await until(() => $('.backdrop'))).click();
  check('A click elsewhere closes the menu', await until(() => !$('.column-menu')));

  // A row for today.
  const rowsBefore = $$('tr[data-r]', box).length;
  $('.table-today', wrap()).click();
  check('New row for today adds a row with today in its date column', await until(() => $$('tr[data-r]', box).length === rowsBefore + 1 && /^\w{3} \d+ \w{3}/.test($$('tr[data-r]', box).pop().querySelector('.cell').value)), $$('tr[data-r]', box).pop()?.querySelector('.cell')?.value);
  check('…and puts the caret in the row to type', await until(() => document.activeElement?.classList.contains('cell') && document.activeElement.closest('tr') === $$('tr[data-r]', box).pop()), document.activeElement?.className);

  // A chart from the "/" menu: it draws the table above it.
  const notes = $$('.blk', pane()).find((b) => b.textContent.includes('Notes'));
  notes.focus();
  T().setSel(notes, 5, 5);
  key(notes, 'Enter');
  const line = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement !== notes && document.activeElement);
  typeIn(line, '/chart');
  const item = await until(() => $$('.slash-menu .menu-item').find((i) => i.textContent.includes('Chart')));
  check('The / menu offers a chart', item);
  item.click();
  const chart = await until(() => $('.chart-block', pane()), 4000);
  check('A chart draws the table above it as bars of each week', chart && (await until(() => $$('.chart-bar', chart).length >= 1)) && $('.chart-title', chart).textContent === 'km per week', chart ? $('.chart-title', chart)?.textContent + ' / ' + $$('.chart-bar', chart).length : 'no chart');
  const panel = await until(() => $('.chart-panel', chart));
  check('A new chart opens its panel', panel);
  check('The panel uses Rune’s own rows, not a system list', panel && !$('select', panel));
  $$('.chart-shape', panel).find((b) => b.textContent.includes('Line')).click();
  check('Line draws a line', await until(() => $('.chart-line', chart)));
  $$('.chart-shape', $('.chart-panel', chart)).find((b) => b.textContent.includes('Ring')).click();
  await until(() => $('.chart-ring', chart));
  const goal = $('.chart-panel input[aria-label="Goal"]', chart);
  goal.value = '10';
  goal.dispatchEvent(new Event('change', { bubbles: true }));
  check('A ring with a goal shows how far it is', await until(() => $('.ring-arc', chart) && /of 10/.test($('.ring-under', chart).textContent)), $('.ring-under', chart)?.textContent);
  $$('.chart-shape', $('.chart-panel', chart)).find((b) => b.textContent.includes('Bars')).click();
  await until(() => $$('.chart-bar', chart).length);
  await wait(250);
  snap('chart-panel');
  (await until(() => $('.backdrop'))).click();
  check('A click elsewhere closes the panel', await until(() => !$('.chart-panel')));
  T().command('copy_markdown');
  await wait(200);
  snap('typed-table');
});

await run('Tables as databases', async () => {
  await oneTab();
  const title = await newPage('Budget check');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Budget check');
  const md = [
    '| Item | Amount |',
    '| --- | --- |',
    '| Salary | 3500 |',
    '| Side gig | 450 |',
    '<!-- table: Income | icon plus:green -->',
    '',
    '| Item | Amount | Category | Day | Paid |',
    '| --- | --- | --- | --- | --- |',
    '| Rent | 2000 | Home | 2026-11-01 | x |',
    '| Groceries | 150 | Food | 2026-11-05 | x |',
    '| Dinner | 65 | Food | 2026-11-07 |  |',
    '<!-- columns: text | text | choice | date | checkbox -->',
    '<!-- table: Expenses | icon minus:red | beside -->',
    '',
    'After',
  ].join('\n');
  const clip = new DataTransfer();
  clip.setData('text/plain', md);
  first.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const wraps = () => $$('.table-wrap', pane());
  await until(() => wraps().length === 2, 4000);
  const wrap = () => wraps()[1];
  const box = () => $('.table-block', wrap());
  const cell = (r, c) => $(`.cell[data-r="${r}"][data-c="${c}"]`, box());
  const panes = () => $$('.pane').filter((p) => p.offsetParent).length;
  const closeMenu = async () => {
    $('.backdrop')?.click();
    await until(() => !$('.table-menu'));
  };
  // One of the commands of the menu of the second table; `sub` is in its second list.
  const tableDo = async (item, sub) => {
    $('.table-head .table-more', wrap()).click();
    const row = await until(() => $(`.own-menu [data-item="${item}"]`, wrap()));
    if (!row) throw new Error(`the menu of the table has no "${item}"`);
    row.click();
    if (sub) (await until(() => $(`.own-menu .column-sub [data-item="${sub}"]`, wrap()))).click();
  };

  // Names, icons, side by side.
  const names = () => $$('.table-name', pane()).map((n) => n.value).join(' / ');
  check('Two tables with their names and icons', names() === 'Income / Expenses' && $$('.table-icon', pane()).length === 2, names());
  const lines = wraps().map((w) => w.closest('.block-row'));
  const [a, b] = lines.map((l) => l.getBoundingClientRect());
  const wide = pane().getBoundingClientRect().width > 700;
  check('The second table stands beside the first (below it in a narrow pane)', lines.every((l) => l.classList.contains('side')) && (wide ? Math.abs(a.top - b.top) < 2 && b.left >= a.right : b.top >= a.bottom), `${wide} ${Math.round(a.top)},${Math.round(a.left)}-${Math.round(a.right)} / ${Math.round(b.top)},${Math.round(b.left)}`);
  check('No system list anywhere in the tables', !$('select', pane()));
  const headHeight = () => Math.round($('tr.head-row', box()).getBoundingClientRect().height);
  check('A heading of one word stands on one line, in a narrow column too', headHeight() < 44, String(headHeight()));
  const inWindow = (el) => {
    const r = el.getBoundingClientRect();
    return r.top >= 0 && r.left >= 0 && r.bottom <= innerHeight && r.right <= innerWidth;
  };
  await wait(200);
  snap('tables-side-by-side');

  // Money: a sign typed in a cell says what the column holds.
  fillCell(cell(1, 1), '$2,000');
  const amounts = () => [1, 2, 3].map((r) => cell(r, 1)?.value).join(' ');
  check('A money sign typed in a cell makes its column money', await until(() => amounts() === '$2,000.00 $150.00 $65.00', 3000), amounts());
  const totals = () => $$('.table-totals td', box()).map((t) => t.textContent.trim()).join(' | ');
  await columnDo(box(), 1, 'total');
  const sum = await until(() => $('.column-sub [data-item="sum"]', box()));
  check('The menu of a column and its second list are on top of the page', sum && onTop(sum) && onTop($('.column-menu [data-item="type"]', box())));
  const menuPlace = () =>
    [$('.column-menu', box()), $('.column-sub', box())]
      .map((m) => {
        const r = m.getBoundingClientRect();
        return [r.left, r.top, r.right, r.bottom].map(Math.round).join(',');
      })
      .join(' / ') + ' in ' + innerWidth + 'x' + innerHeight;
  check('…both are whole in the window, and the table stays in its box', inWindow($('.column-menu', box())) && inWindow($('.column-sub', box())) && getComputedStyle(box()).overflowX !== 'visible', menuPlace());
  sum.click();
  check('The sum is written as money too', await until(() => totals().includes('Sum $2,215.00')), totals());
  $('.column-menu [data-item="format"]', box()).click();
  (await until(() => $('.column-sub [data-item="EUR"]', box()))).click();
  check('Another kind of money writes every number again', await until(() => cell(1, 1).value === '€2,000.00' && totals().includes('Sum €2,215.00')), amounts() + ' ' + totals());
  await wait(150);
  snap('table-column-menu');
  $('.column-sub [data-item="USD"]', box()).click();
  await until(() => cell(1, 1).value === '$2,000.00');
  await closeMenu();

  // A row is a page: Open shows it at the side.
  $('tr[data-r="1"] .row-open', box()).click();
  check('Open shows the row as a page at the side', (await until(() => shownPane('Rent'), 4000)) && panes() === 2 && $('[data-peek="close"]', shownPane('Rent')), String(panes()));
  const valueNames = (p) => $$('.row-value input.value-name', p).map((n) => n.value).join(',');
  check('The page lists the values of its row', await until(() => valueNames(shownPane('Rent')) === 'Amount,Category,Day,Paid' && $('.row-values .cell[data-c="1"]', shownPane('Rent'))?.value === '$2,000.00'), valueNames(shownPane('Rent')));
  check('The row is marked while its page is open', $('tr[data-r="1"]', box()).classList.contains('open'));
  check('The page of a row is not in the sidebar', !treeItem('Rent'));
  check('…and opening it leaves its parent closed there', !$$('.sidebar .tree-item').some((t) => t.textContent.includes('No pages inside')) && !$$('.sidebar').some((b) => b.textContent.includes('No pages inside')), $('.sidebar')?.textContent.slice(0, 120));
  const rowTitle = $('.title-input', shownPane('Rent'));
  rowTitle.focus();
  rowTitle.value = 'Rent and heat';
  rowTitle.dispatchEvent(new Event('input', { bubbles: true }));
  check('The name typed on the page goes to the row', await until(() => cell(1, 0).value === 'Rent and heat'), cell(1, 0).value);
  fillCell($('.row-values .cell[data-c="1"]', shownPane('Rent and heat')), '2100');
  check('A value changed on the page changes the cell, in the way its column writes', await until(() => cell(1, 1).value === '$2,100.00'), cell(1, 1).value);
  const nameCell = cell(1, 0);
  nameCell.focus();
  nameCell.value = 'Rent';
  nameCell.dispatchEvent(new Event('input', { bubbles: true }));
  check('…and the name typed in the row goes to the page', await until(() => shownPane('Rent')), $$('.title-input').map((t) => t.value).join(','));
  document.activeElement.blur();
  $('tr[data-r="2"] .row-open', box()).click();
  check('The next row opens in the same pane at the side', await until(() => shownPane('Groceries') && panes() === 2 && !shownPane('Rent'), 4000), `${panes()} ${$$('.title-input').map((t) => t.value).join(',')}`);
  const marked = () => $$('tr.open', box()).map((tr) => tr.dataset.r).join(',');
  check('The mark goes to the row whose page is open now', await until(() => marked() === '2'), marked());
  await wait(250);
  snap('table-side-peek');
  $('[data-peek="close"]', shownPane('Groceries')).click();
  check('The pane at the side closes', await until(() => panes() === 1), String(panes()));
  check('…and no row is marked then', await until(() => marked() === ''), marked());

  // Sorts and filters, as chips. A row stays where it is while it is typed in.
  const shown = () => $$('tr[data-r]', box()).slice(1).map((tr) => $('.cell[data-c="0"]', tr).value).join(',');
  await columnDo(box(), 1, 'sort-up');
  check('A sort orders the rows and shows as a chip', await until(() => shown() === 'Dinner,Groceries,Rent' && $('[data-chip="sort"]', wrap())?.textContent.includes('Amount, low to high')), shown());
  const dinner = cell(3, 1);
  dinner.focus();
  await wait(150);
  dinner.value = '9999';
  dinner.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(300);
  check('A row stays in its place while it is typed in', shown() === 'Dinner,Groceries,Rent', shown());
  dinner.dispatchEvent(new Event('change', { bubbles: true }));
  cell(3, 1).blur();
  check('…and goes to its new place when the caret leaves the table', await until(() => shown() === 'Groceries,Rent,Dinner', 3000), shown());
  fillCell(cell(3, 1), '65');
  document.activeElement.blur();
  await until(() => shown() === 'Dinner,Groceries,Rent', 3000);
  await columnDo(box(), 2, 'filter');
  const chip = await until(() => $('.chip-menu', wrap()));
  check('Filter opens a chip to say what it asks', chip && $('[data-item="is"]', chip)?.classList.contains('active') && onTop($('[data-item="is"]', chip)));
  $$('[data-item="value"]', chip).find((v) => v.textContent.includes('Food')).click();
  check('A filter leaves the rows that pass, and the totals count only those', await until(() => shown() === 'Dinner,Groceries' && totals().includes('Sum $215.00')), shown() + ' ' + totals());
  await wait(150);
  snap('table-filter');
  await closeMenu();
  $('[data-new-row]', wrap()).click();
  check('A new row takes the value the filter asks, so it stays in view', await until(() => $$('tr[data-r]', box()).length === 4 && cell(4, 2)?.value === 'Food'), `${$$('tr[data-r]', box()).length} ${cell(4, 2)?.value}`);
  document.activeElement.blur();
  await rowDo(box(), 4, 'delete');
  await until(() => !cell(4, 0));
  for (const kind of ['filter', 'sort']) {
    $(`[data-chip="${kind}"]`, wrap()).click();
    (await until(() => $('.chip-menu [data-item="remove"]', wrap()))).click();
    await until(() => !$(`[data-chip="${kind}"]`, wrap()));
  }
  check('The chips go with the filter and the sort', !$('.table-chips', wrap()) && shown() === 'Rent,Groceries,Dinner', shown());

  // Groups, and the other views of the same rows.
  fillCell(cell(3, 3), 'today');
  document.activeElement.blur();
  await columnDo(box(), 2, 'group');
  const groups = () => $$('.group-row', box()).map((g) => g.textContent.trim()).join(',');
  check('Group puts the rows under the values of a column', await until(() => groups() === 'Home1,Food2'), groups());
  await tableDo('view', 'board');
  const board = () => $('.table-board', wrap());
  const cards = () => $$('.board-col', board()).map((c) => `${$('.tag', c).textContent}:${$$('.board-card', c).length}`).join(',');
  check('A board has a column of cards for each value, with its sum', (await until(() => board())) && cards() === 'Home:1,Food:2' && $('.board-sum', board())?.textContent === '$2,100.00', cards() + ' ' + $('.board-sum', board())?.textContent);
  await wait(150);
  snap('table-board');
  await dragAlong($$('.board-card', board()).find((c) => c.textContent.includes('Dinner')), [centre($$('.board-col', board())[0])]);
  check('A card dragged to another column takes its value', await until(() => cards() === 'Home:2,Food:1', 3000), cards());
  await tableDo('view', 'list');
  check('A list has a line for each row', await until(() => $$('.list-row', wrap()).length === 3), String($$('.list-row', wrap()).length));
  await tableDo('view', 'gallery');
  check('A gallery has a card for each row', await until(() => $$('.gallery-card', wrap()).length === 3), String($$('.gallery-card', wrap()).length));
  await tableDo('view', 'calendar');
  const month = () => $('.table-month', wrap());
  check('A calendar shows a month with the row of today on today', (await until(() => month())) && $$('.month-day', month()).length >= 28 && $('.month-day.today .day-row', month())?.textContent === 'Dinner', $('.month-day.today', month())?.textContent);
  const before = month().dataset.month;
  $('[data-month-step="1"]', month()).click();
  check('The arrows go to the months beside it', await until(() => month().dataset.month !== before), month().dataset.month);
  await wait(150);
  snap('table-calendar');
  await tableDo('view', 'table');
  await until(() => box());
  $('[data-chip="group"] .chip-x', wrap()).click();
  check('Back as a table, with no groups', await until(() => !$('.group-row', box()) && shown() === 'Rent,Groceries,Dinner'), shown());

  // Rows are picked by their handle, and stay picked when their menu closes.
  $('[data-row-op="row-menu"]', await rowHandle(box(), 1)).click();
  const rowMenu = await until(() => $('.row-menu', box()));
  check('The handle of a row opens its menu, on top of the page', rowMenu && onTop($('[data-item="side"]', rowMenu)) && $('.table-picked', wrap())?.textContent === '1 row selected', $('.table-picked', wrap())?.textContent);
  await wait(150);
  snap('table-row-menu');
  await closeMenu();
  check('The row stays picked when its menu closes', $$('tr.picked', box()).length === 1);
  // A zoomed page (View → Zoom): the menu still opens at its row, all of it in sight.
  for (const [how, times] of [['zoom_out', 2], ['zoom_in', 2]]) {
    T().command('zoom_reset');
    for (let i = 0; i < times; i++) T().command(how);
    await wait(250);
    $('[data-row-op="row-menu"]', await rowHandle(box(), 1)).click();
    const zoomed = await until(() => $('.row-menu.anchored', box()));
    await wait(250);
    const cell = $('tr[data-r="1"] .first-cell', box())?.getBoundingClientRect();
    const m = zoomed?.getBoundingClientRect();
    const seen = m && document.elementFromPoint(m.left + 6, m.top + 14);
    check(`In a page made ${how === 'zoom_out' ? 'smaller' : 'bigger'}, a row menu opens at its row, and all of it is seen`, m && cell && Math.abs(m.left - cell.left) < 3 && zoomed.contains(seen), m && cell ? `menu at ${Math.round(m.left)}, row at ${Math.round(cell.left)}, zoom ${getComputedStyle($('.card-zoom')).zoom}` : 'no menu');
    await closeMenu();
  }
  T().command('zoom_reset');
  await wait(250);
  $('[data-row-op="row-menu"]', await rowHandle(box(), 3)).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, shiftKey: true }));
  check('Shift and a click pick the rows up to that row', await until(() => $$('tr.picked', box()).length === 3 && $('.row-menu [data-item="delete"]', box())?.textContent.includes('Delete 3 rows')), `${$$('tr.picked', box()).length} ${$('.row-menu [data-item="delete"]', box())?.textContent}`);
  await closeMenu();
  $('h1, .title-input', pane()).dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
  check('A press elsewhere lets the rows go', await until(() => $$('tr.picked', box()).length === 0));

  // Rows nested under a row.
  await rowDo(box(), 3, 'nest');
  check('A row nested under the row above stands in, and that row can fold it', await until(() => $('tr[data-r="3"] .first-cell', box())?.style.paddingLeft === '18px' && $('tr[data-r="2"] .row-fold', box())), $('tr[data-r="3"] .first-cell', box())?.style.paddingLeft);
  $('tr[data-r="2"] .row-fold', box()).click();
  check('Folding hides the rows inside', await until(() => !$('tr[data-r="3"]', box())));
  $('tr[data-r="2"] .row-fold', box()).click();
  await until(() => $('tr[data-r="3"]', box()));
  await rowDo(box(), 3, 'unnest');
  await until(() => !$('.row-fold', box()));

  // A formula column, with bars.
  $('[data-add-column]', wrap()).click();
  const kinds = await until(() => $('.kinds-menu', wrap()));
  check('A new column can be one of fifteen kinds, in a menu on top of the page', kinds && $$('[data-item]', kinds).length === 15 && onTop($('[data-item="text"]', kinds)), String($$('[data-item]', kinds || document).length));
  await wait(150);
  snap('table-kinds');
  $('[data-item="formula"]', kinds).click();
  await until(() => $$('tr.head-row td', box()).length === 6);
  const heading = cell(0, 5);
  heading.value = 'Double';
  heading.dispatchEvent(new Event('input', { bubbles: true }));
  $$('.th-menu', box())[5].click();
  const formula = await until(() => $('.column-menu [data-item="formula"]', box()));
  const typeFormula = (text) => {
    const field = $('.column-menu [data-item="formula"]', box());
    field.value = text;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  };
  typeFormula('Amount *');
  check('A formula that cannot be read says why', formula && (await until(() => $('.column-error', box())?.textContent.length > 5)), $('.column-error', box())?.textContent);
  typeFormula('Amount * 2');
  const worked = () => $$('.cell.computed', box()).map((c) => c.value).join(',');
  check('A formula is worked out for each row', await until(() => worked() === '4200,300,130' && !$('.column-error', box())), worked());
  $('.column-menu [data-item="format"]', box()).click();
  (await until(() => $('.column-sub [data-item="show-as"]', box()))).click();
  (await until(() => $('.column-sub [data-show="Bar"]', box()))).click();
  check('Bars show how large each number is', await until(() => $$('.meter-bar', box()).length === 3 && $('.meter-bar span', box()).style.width === '100%'), String($$('.meter-bar', box()).length));
  await closeMenu();

  // A column that stays in place, a colour rule, the template of new rows, the lock.
  await columnDo(box(), 0, 'freeze');
  check('The first column can be frozen (its cell at the foot too)', await until(() => $$('tbody td.frozen', box()).length === 4 && $$('tfoot td.frozen', box()).length === 1 && getComputedStyle($('td.frozen', box())).position === 'sticky'), String($$('td.frozen', box()).length));
  await tableDo('paint');
  const paint = await until(() => $('.paint-menu', wrap()));
  check('The menu of a table and its colour rules are on top of the page', paint && onTop($('.pick', paint)));
  $$('.pick', paint).find((p) => p.textContent === 'Paid').click();
  (await until(() => $('.paint-menu [data-op="notticked"]', wrap()))).click();
  check('A colour rule gives its colour to the rows that pass', await until(() => $$('tr.painted', box()).length === 1 && $('tr.painted .cell', box()).value === 'Dinner'), String($$('tr.painted', box()).length));
  await wait(150);
  snap('table-colours');
  await closeMenu();
  await tableDo('template');
  const template = await until(() => shownPane('Template'), 4000);
  check('The template of new rows opens at the side', template && $('.values-note', template));
  const category = $$('.row-value', template).find((v) => $('.value-name', v)?.textContent === 'Category')?.querySelector('input.cell');
  category.value = 'Fun';
  category.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(150);
  $('[data-peek="close"]', shownPane('Template')).click();
  await until(() => panes() === 1);
  $('[data-new-row]', wrap()).click();
  check('A new row starts with what the template says', await until(() => cell(4, 2)?.value === 'Fun'), String(cell(4, 2)?.value));
  document.activeElement.blur();
  await tableDo('lock');
  check('A locked table keeps its columns: no new column, no other heading', await until(() => $('.table-block.locked', wrap()) && cell(0, 0).readOnly && !$('[data-add-column]', wrap())));
  await tableDo('lock');
  check('…and takes them again when it is unlocked', await until(() => !$('.table-block.locked', wrap()) && $('[data-add-column]', wrap())));
  await wait(150);
  snap('tables-end');
});

await run('A table with many columns', async () => {
  // The tables the AI made for a trading journal: eleven and twelve columns.
  // In a page they were pressed so thin that a heading had one letter in a line.
  await oneTab();
  const title = await newPage('Trading journal');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Trading journal');
  const md = [
    "| Date | Sleep | Mood | Market bias | News and key levels | Trades | P&L | Rules kept | Best decision | Worst decision | Tomorrow's focus |",
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |',
    '|  |  |  |  |  |  |  |  |  |  |  |',
    '<!-- columns: date count | number | choice | choice | text | number | number USD sum | checkbox checked | text | text | text -->',
    '',
    '| Date | Ticker | Market | Side | Setup | Session | Entry | Stop | Target | Exit | Size | Hold |',
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |',
    '| 2026-11-10 | Btc | Crypto | Long | Breakout | London | 68000 | 67200 | 70000 |  | 0.5 | 45:00 |',
    '<!-- columns: date count | text | choice | choice | choice | choice | number | number | number | number | number | time -->',
    '',
    '| Week starting | P&L | What worked | One change for next week |',
    '| --- | --- | --- | --- |',
    '|  |  |  |  |',
    '<!-- columns: date count | number | text | text -->',
    '',
    '| Date | Ticker | Side | Setup | Risk | P/L | R | Followed plan | Notes |',
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- |',
    '| 2026-10-20 | AAPL | Long | Breakout | 100 | 180 | 1.8 | Yes | Clean break on volume |',
    '| 2026-10-22 | TSLA | Short | Reversal | 100 | -100 | -1 | Yes | Stopped out |',
    '| 2026-10-27 | NVDA | Long | Pullback | 100 | 240 | 2.4 | Yes | Held to target |',
    '| 2026-10-28 | AMD | Long | A very long setup name here | 100 | -130 | -1.3 | No | Moved my stop |',
    '<!-- columns: date | text | choice | choice | number | number | number | choice | text -->',
    '',
    'After',
  ].join('\n');
  const clip = new DataTransfer();
  clip.setData('text/plain', md);
  first.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const wraps = () => $$('.table-wrap', pane());
  await until(() => wraps().length === 4, 4000);
  await wait(200);
  const heads = (w) => $$('tr.head-row textarea.cell', w);
  const tall = (h) => Math.round(h.getBoundingClientRect().height);
  // A heading that is whole: no word of it is wider than its box, and it has two lines at most.
  const broken = (w) => heads(w).filter((h) => h.scrollWidth > h.clientWidth + 1 || tall(h) > 60).map((h) => `${h.value} ${h.scrollWidth}/${h.clientWidth} ${tall(h)}`);
  for (const [n, count, name] of [[0, 11, 'eleven'], [1, 12, 'twelve']]) {
    check(`The headings of a table with ${name} columns are whole words`, heads(wraps()[n]).length === count && broken(wraps()[n]).length === 0, broken(wraps()[n]).join(' | ') || heads(wraps()[n]).map(tall).join(','));
  }
  const box = (n) => $('.table-block', wraps()[n]);
  const edge = pane().getBoundingClientRect().right;
  check('…and the table scrolls sideways in its box in place of thinner columns', [0, 1].every((n) => box(n).scrollWidth > box(n).clientWidth + 100 && box(n).getBoundingClientRect().right <= edge), [0, 1].map((n) => `${box(n).scrollWidth} in ${box(n).clientWidth}`).join(' | '));
  const widths = $$('tr.head-row > td', box(0)).map((td) => Math.round(td.getBoundingClientRect().width));
  check('A column has the width that its kind and its heading need', Math.min(...widths) >= 66 && widths[4] > widths[7] && widths[10] > widths[1], widths.join(','));
  check('A table with no row yet has no line of totals at its foot', !$('.table-totals', wraps()[0]), $('.table-totals', wraps()[0])?.textContent);
  const foot = $('.table-totals td', wraps()[1]);
  check('…and one with a row has, as plain words under the table', foot?.textContent.includes('1 row') && getComputedStyle(foot).borderLeftColor !== getComputedStyle($('tr[data-r="1"] td', box(1))).borderLeftColor, `${foot?.textContent} ${foot && getComputedStyle(foot).borderLeftColor}`);
  const tags = $$('.cell.pick .tag', wraps()[3]);
  const twoLines = tags.filter((t) => t.getBoundingClientRect().height > 30).map((t) => t.value);
  check('A tag stands on one line, and a long one is cut at its end', tags.length === 12 && twoLines.length === 0, twoLines.join(' | ') || String(tags.length));
  check('The headings of the trade log are whole words', broken(wraps()[3]).length === 0, broken(wraps()[3]).join(' | '));
  snap('table-wide');
  // "New row for today" in a table whose only row is empty: that row takes
  // the day (no empty row stays above a new one), and the caret is in it.
  const rows = () => $$('tr[data-r]', box(2)).slice(1).map((tr) => $('.cell[data-c="0"]', tr).value).join('|');
  const caret = () => `${document.activeElement?.dataset.r},${document.activeElement?.dataset.c}`;
  $('[data-new-row]', wraps()[2]).click();
  check('A row for today uses the empty row of a new table', (await until(() => /^\w{3} \d+ \w{3}$/.test(rows()) && caret() === '1,1', 3000)) && !$('.table-totals', wraps()[2])?.textContent.includes('0 rows'), `${rows()} caret ${caret()}`);
  $('[data-new-row]', wraps()[2]).click();
  check('…and the next one is a new row under it, with the caret in it', await until(() => rows().split('|').length === 2 && rows().split('|').every((d) => d) && caret() === '2,1', 3000), `${rows()} caret ${caret()}`);
  check('The count at the foot says how many rows there are', $('.table-totals', wraps()[2])?.textContent.includes('2 rows'), $('.table-totals', wraps()[2])?.textContent);
  document.activeElement.blur();
  // A frozen column stays where it is while the table scrolls under it.
  const closeWide = async () => {
    $('.backdrop')?.click();
    await until(() => !$('.table-menu'));
  };
  await columnDo(box(1), 0, 'freeze');
  await closeWide();
  await until(() => $$('td.frozen', box(1)).length > 0);
  await wait(150);
  const frozenCell = () => $('tr[data-r="1"] td.frozen', box(1));
  const movingCell = () => $$('tr[data-r="1"] td', box(1))[3];
  const startFrozen = frozenCell().getBoundingClientRect().left;
  const startMoving = movingCell().getBoundingClientRect().left;
  box(1).scrollLeft = 260;
  await wait(200);
  const nowFrozen = frozenCell().getBoundingClientRect();
  const nowMoving = movingCell().getBoundingClientRect().left;
  const frozenHit = document.elementFromPoint(nowFrozen.left + nowFrozen.width - 4, nowFrozen.top + nowFrozen.height / 2);
  check('A frozen column stays while the others scroll under it', Math.abs(nowFrozen.left - startFrozen) < 1 && startMoving - nowMoving > 200 && frozenCell().contains(frozenHit), `${startFrozen} ${nowFrozen.left} ${startMoving} ${nowMoving} ${frozenHit?.className}`);
  snap('table-frozen-scrolled');
  box(1).scrollLeft = 0;
  // The last columns are there when the table is scrolled.
  box(0).scrollLeft = box(0).scrollWidth;
  await wait(200);
  snap('table-wide-end');
});

await run('Choices like Notion', async () => {
  await oneTab();
  const title = await newPage('Choices check');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Choices check');
  const md = [
    '| Ticker | Side | Tags |',
    '| --- | --- | --- |',
    '| AAPL | Long | Swing, Earnings |',
    '| TSLA | Short |  |',
    '| NVDA |  |  |',
    '<!-- columns: text | choice | multi -->',
    '',
    'After',
  ].join('\n');
  const clip = new DataTransfer();
  clip.setData('text/plain', md);
  first.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const box = () => $('.table-block', pane());
  await until(() => box(), 4000);
  await wait(200);
  const cell = (r, c) => $('.cell[data-r="' + r + '"][data-c="' + c + '"]', box());
  const picker = () => $('.choice-picker');
  const tagsOf = (el) => $$('.tag', el).map((t) => t.textContent.trim()).join(',');
  const typeIn = (text) => {
    const input = $('.pick-input', picker());
    input.value = text;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const press = (k) => key($('.pick-input', picker()), k);
  const colour = (el) => getComputedStyle(el).backgroundColor;

  check('A choice cell shows a tag, and each option has its own colour', tagsOf(cell(1, 1)) === 'Long' && tagsOf(cell(2, 1)) === 'Short' && colour($('.tag', cell(1, 1))) !== colour($('.tag', cell(2, 1))), `${tagsOf(cell(1, 1))} ${colour($('.tag', cell(1, 1)))} ${colour($('.tag', cell(2, 1)))}`);
  check('Several choices show as several tags', tagsOf(cell(1, 2)) === 'Swing,Earnings', tagsOf(cell(1, 2)));

  // The list.
  cell(3, 1).click();
  check('A click on the cell opens the list, with the caret in its box', (await until(() => picker())) && (await until(() => document.activeElement === $('.pick-input', picker()))), document.activeElement?.className);
  const options = () => $$('.pick-row[data-option]', picker()).map((o) => o.dataset.option).join(',');
  check('The list has the options of the column, on top of the page and whole in the window', options() === 'Long,Short' && onTop($('.pick-row', picker())) && picker().getBoundingClientRect().bottom <= innerHeight, options());
  await wait(150);
  snap('choice-list');
  $('.pick-row[data-option="Short"]', picker()).click();
  check('A click on an option fills the cell and closes the list', await until(() => tagsOf(cell(3, 1)) === 'Short' && !picker()), tagsOf(cell(3, 1)));

  // Search, make with Return.
  cell(3, 1).click();
  await until(() => picker());
  typeIn('lo');
  check('Typing narrows the list', await until(() => options() === 'Long'), options());
  typeIn('Hedge');
  check('A new word offers to create it', await until(() => $('[data-make]', picker())?.textContent.includes('Hedge')), $('[data-make]', picker())?.textContent);
  await wait(100);
  snap('choice-create');
  press('Enter');
  check('Return creates the option and chooses it', await until(() => tagsOf(cell(3, 1)) === 'Hedge' && !picker()), tagsOf(cell(3, 1)));

  // Arrows.
  cell(2, 1).click();
  await until(() => picker());
  press('ArrowDown');
  await wait(60);
  check('The arrows move through the options', $('.pick-row.active', picker())?.dataset.option === 'Short', $('.pick-row.active', picker())?.dataset.option);
  press('Escape');
  check('Escape closes the list and changes nothing', await until(() => !picker()) && tagsOf(cell(2, 1)) === 'Short', tagsOf(cell(2, 1)));

  // Several: the list stays open.
  cell(2, 2).click();
  await until(() => picker());
  $('.pick-row[data-option="Swing"]', picker()).click();
  check('With several choices the list stays open, and shows the chosen in its box', await until(() => tagsOf(cell(2, 2)) === 'Swing' && picker() && tagsOf($('.pick-bar', picker())) === 'Swing'), tagsOf(cell(2, 2)));
  $('.pick-row[data-option="Earnings"]', picker()).click();
  check('…takes a second one', await until(() => tagsOf(cell(2, 2)) === 'Swing,Earnings'), tagsOf(cell(2, 2)));
  $('.pick-bar .tag-x', picker()).click();
  check('…and the cross on a tag takes it away', await until(() => tagsOf(cell(2, 2)) === 'Earnings'), tagsOf(cell(2, 2)));
  press('Backspace');
  check('Backspace in the empty box takes the last tag away', await until(() => tagsOf(cell(2, 2)) === ''), tagsOf(cell(2, 2)));
  press('Escape');
  await until(() => !picker());

  // Name and colour.
  cell(3, 1).click();
  await until(() => picker());
  $('.pick-row[data-option="Long"] .pick-more', picker()).click();
  const editor = await until(() => $('.option-editor'));
  check('The dots of an option open its name, its colours and Delete', editor && $$('[data-color]', editor).length === 10 && $('[data-item="delete-option"]', editor) && onTop($('[data-color="red"]', editor)));
  await wait(120);
  snap('choice-editor');
  const before = colour($('.tag', cell(1, 1)));
  $('[data-color="red"]', editor).click();
  check('A colour changes the tag in every row', await until(() => colour($('.tag', cell(1, 1))) !== before), `${before} ${colour($('.tag', cell(1, 1)))}`);
  const name = $('.pop-input', $('.option-editor'));
  name.value = 'Buy';
  name.dispatchEvent(new Event('input', { bubbles: true }));
  name.dispatchEvent(new Event('change', { bubbles: true }));
  check('A new name changes the cells that have it', await until(() => tagsOf(cell(1, 1)) === 'Buy'), tagsOf(cell(1, 1)));
  await until(() => $('.option-editor [data-item="delete-option"]'));
  $('.option-editor [data-item="delete-option"]').click();
  check('Delete takes the option out of its cells', await until(() => tagsOf(cell(1, 1)) === ''), tagsOf(cell(1, 1)));
  check('…and goes back to the list without it', await until(() => picker() && !$('.option-editor') && !options().includes('Buy')), options());
  press('Escape');
  await until(() => !picker());

  // An option dragged in its list.
  cell(3, 1).click();
  await until(() => picker());
  const rowOf = (name) => $('.pick-row[data-option="' + name + '"]', picker());
  const dragData = new DataTransfer();
  rowOf('Hedge').dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dragData }));
  await wait(60);
  rowOf('Short').dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dragData }));
  rowOf('Short').dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dragData }));
  check('An option dragged in the list takes its new place', await until(() => options() === 'Hedge,Short'), options());
  typeIn('zzz');
  check('…and a search that finds nothing, in a list that can make it, offers to create it', await until(() => $('[data-make]', picker())));
  press('Escape');
  await until(() => !picker());

  // By the keyboard from the cell.
  cell(1, 1).focus();
  key(cell(1, 1), 'h');
  check('A letter typed on the cell opens the list to search', await until(() => picker() && $('.pick-input', picker()).value === 'h' && options() === 'Hedge,Short'), options());
  press('Escape');
  await until(() => !picker());
  check('The caret goes back to the cell', await until(() => document.activeElement === cell(1, 1)), document.activeElement?.className);

  // In the page of a row.
  $('tr[data-r="3"] .row-open', box()).click();
  const side = await until(() => shownPane('NVDA'), 4000);
  const valueCell = side && $('.row-values .cell.pick[data-c="1"]', side);
  check('The page of a row shows the same tags', valueCell && tagsOf(valueCell) === 'Hedge', valueCell && tagsOf(valueCell));
  valueCell.click();
  const list = await until(() => $('.choice-picker', side));
  await wait(200);
  const pickRect = list && $('.pick-row', list).getBoundingClientRect();
  const pickHit = pickRect && document.elementFromPoint(pickRect.left + pickRect.width / 2, pickRect.top + pickRect.height / 2);
  snap('choice-row-page');
  check('…and the same list', list && onTop($('.pick-row', list)), pickRect && `${Math.round(pickRect.left)},${Math.round(pickRect.top)} hit ${pickHit?.tagName}.${pickHit?.className}`);
  $('.pick-row[data-option="Short"]', list).click();
  check('…which changes the row', await until(() => tagsOf(cell(3, 1)) === 'Short'), tagsOf(cell(3, 1)));
  $('[data-peek="close"]', shownPane('NVDA')).click();
  await until(() => !shownPane('NVDA'));
});

await run('The mock pages', async () => {
  // The same pages and the same moments as the mock boards, to lay beside them.
  const closeMenu = async () => {
    $('.backdrop')?.click();
    await until(() => !$('.table-menu'));
  };
  await oneTab();
  const budgetTitle = await newPage('Monthly Budget');
  key(budgetTitle, 'Enter');
  let first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const paste = (el, md) => {
    const clip = new DataTransfer();
    clip.setData('text/plain', md);
    el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  };
  paste(first, [
    'What comes in and what goes out this month. Each table adds itself up.',
    '',
    '| Item | Amount |',
    '| --- | --- |',
    '| Salary | 3500 |',
    '| Side gig | 450 |',
    '| Birthday money | 150 |',
    '<!-- columns: text | number USD sum -->',
    '<!-- table: Income | icon plus:green -->',
    '',
    '| Item | Amount |',
    '| --- | --- |',
    '| Rent | 2000 |',
    '| Groceries | 150 |',
    '| Dinner with Rachel | 65 |',
    '<!-- columns: text | number USD sum -->',
    '<!-- table: Expenses | icon minus:red | beside -->',
  ].join('\n'));
  const budget = () => shownPane('Monthly Budget');
  await until(() => $$('.table-wrap', budget()).length === 2, 4000);
  await wait(250);
  const money = (w) => $$('tr[data-r] td.num .cell', w).map((c) => c.value).join(' ');
  check('Markdown money is written as money', money($$('.table-wrap', budget())[0]) === '$3,500.00 $450.00 $150.00', money($$('.table-wrap', budget())[0]));
  snap('mock-main');
  const income = () => $$('.table-wrap', budget())[0];
  const expenses = () => $$('.table-wrap', budget())[1];
  await columnDo($('.table-block', income()), 1, 'format');
  await until(() => $('.column-sub [data-item="USD"]', income()));
  check('The list of formats starts short, with a way to the others', $$('.column-sub [data-item]', income()).filter((m) => /^[A-Z]{3}$/.test(m.dataset.item)).length === 4 && $('.column-sub [data-item="more-money"]', income()));
  const placesRow = $('.column-sub [data-item="places"]', income());
  check('Decimal places is a row that says what it is set to', placesRow?.textContent.includes('Decimal places') && placesRow.textContent.includes('Auto') && !$('.column-sub [data-places]', income()), placesRow?.textContent);
  await wait(200);
  snap('mock-column');
  placesRow.click();
  (await until(() => $('.column-sub [data-places="1"]', income()))).click();
  check('…and its row sets how the numbers are written', await until(() => money(income()) === '$3,500.0 $450.0 $150.0' && $('.column-sub [data-item="places"]', income()).textContent.includes('1')), money(income()));
  $('.column-sub [data-places="Auto"]', income()).click();
  await until(() => money(income()) === '$3,500.00 $450.00 $150.00');
  $('.column-sub [data-item="more-money"]', income()).click();
  check('Other currency shows the other kinds of money', await until(() => $('.column-sub [data-item="CHF"]', income()) && !$('.column-sub [data-item="more-money"]', income())));
  $('.column-sub [data-item="CHF"]', income()).click();
  check('…and one of them can be chosen', await until(() => money(income()).startsWith('CHF 3,500.00')), money(income()));
  $('.column-sub [data-item="USD"]', income()).click();
  await until(() => money(income()).startsWith('$3,500.00'));
  await closeMenu();
  $('tr[data-r="1"] .row-open', expenses()).click();
  const rent = await until(() => shownPane('Rent'), 4000);
  const crumbWords = () => $$('.crumb', rent).map((c) => c.textContent.trim()).join(' / ');
  check('The page of a row sits under its table', crumbWords().endsWith('Expenses / Rent'), crumbWords());
  await wait(250);
  snap('mock-peek');
  $('[data-peek="close"]', rent).click();
  await until(() => !shownPane('Rent'));

  await oneTab();
  const title = await newPage('Spending');
  key(title, 'Enter');
  first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  paste(first, [
    'Each expense of November, one row for each.',
    '',
    '| Item | Amount | Category | Date | Paid |',
    '| --- | --- | --- | --- | --- |',
    '| Rent | 2000 | Home | 2026-11-01 | x |',
    '| Internet | 120 | Home | 2026-11-03 | x |',
    '| Groceries | 150 | Food | 2026-11-05 | x |',
    '| Dinner with Rachel | 65 | Food | 2026-11-07 | x |',
    '| Coffee beans | 56.40 | Food | 2026-11-08 | x |',
    '| Cinema | 32 | Fun | 2026-11-08 | x |',
    '| Concert tickets | 63 | Fun | 2026-11-13 |  |',
    '| Train to Portland | 48 | Travel | 2026-11-14 |  |',
    '<!-- columns: text count | number USD sum | choice {Home:gray; Food:orange; Fun:purple; Travel:blue} | date | checkbox checked -->',
    '<!-- table: Expenses | icon minus:red -->',
  ].join('\n'));
  const pane = () => shownPane('Spending');
  await until(() => $('.table-wrap', pane()), 4000);
  await wait(250);
  const wrap = () => $('.table-wrap', pane());
  const box = () => $('.table-block', wrap());
  const cell = (r, c) => $('.cell[data-r="' + r + '"][data-c="' + c + '"]', box());
  check('Markdown days are written as days', /^\w{3} 1 Nov/.test(cell(1, 3).value), cell(1, 3).value);
  check('Colours given in the Markdown show', getComputedStyle($('.tag', cell(1, 2))).backgroundColor !== getComputedStyle($('.tag', cell(3, 2))).backgroundColor);
  const widths = $$('tr.head-row > td', box()).map((td) => Math.round(td.getBoundingClientRect().width));
  check('The name column takes the room that is left, and five columns need no scrolling', widths[0] > widths[1] * 1.6 && box().scrollWidth <= box().clientWidth + 1, widths.join(','));
  snap('mock-table');
  $('[data-row-op="row-menu"]', await rowHandle(box(), 4)).click();
  await until(() => $('.row-menu', box()));
  await wait(200);
  snap('mock-row-menu');
  await closeMenu();
  await columnDo(box(), 2, 'filter');
  const chip = await until(() => $('.chip-menu', wrap()));
  $$('[data-item="value"]', chip).find((v) => v.textContent.includes('Food')).click();
  await closeMenu();
  await columnDo(box(), 1, 'sort-down');
  check('A filter and a sort show as chips, each with a cross', await until(() => $$('.table-chips .chip-x.on-chip', wrap()).length === 2));
  await wait(200);
  snap('mock-filter');
  $('[data-chip-x="filter"]', wrap()).click();
  check('The cross of a chip takes it away', await until(() => !$('[data-chip="filter"]', wrap()) && $$('tr[data-r]', box()).length === 9), String($$('tr[data-r]', box()).length));
  $('[data-chip-x="sort"]', wrap()).click();
  await until(() => !$('[data-chip="sort"]', wrap()));
  $('.table-head .table-more', wrap()).click();
  (await until(() => $('.own-menu [data-item="view"]', wrap()))).click();
  await until(() => $('.own-menu .column-sub [data-item="table"]', wrap()));
  await wait(200);
  snap('mock-table-menu');
  $('.own-menu .column-sub [data-item="board"]', wrap()).click();
  const board = await until(() => $('.table-board', wrap()));
  check('A board says what it is by, in the line of its name', board && $('.table-by', wrap())?.textContent === 'by Category' && !$('[data-chip="group"]', wrap()), $('.table-by', wrap())?.textContent);
  check('A card has its amount and its day on one line', $$('.board-card .card-facts span', board).length === 16, String($$('.board-card .card-facts span', board).length));
  check('The foot of a board says its rows and its sum', $('.table-summary', wrap())?.textContent === '8 rows  ·  Sum $2,534.40', $('.table-summary', wrap())?.textContent);
  await closeMenu();
  await wait(200);
  snap('mock-board');
});

await run('Gains, losses, status and pies', async () => {
  await oneTab();
  const title = await newPage('Gains check');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Gains check');
  const md = [
    '| Item | Date | State | Result | Amount |',
    '| --- | --- | --- | --- | --- |',
    '| One | 2026-11-02 | Done | Good | 300 |',
    '| Two | 2026-11-03 | In progress | Bad | -100 |',
    '| Three | 2026-11-09 | Done | Good | 50 |',
    '<!-- columns: text | date | status | choice {Good:green; Bad:red} | number USD sum signed -->',
    '',
    'End',
  ].join('\n');
  const clip = new DataTransfer();
  clip.setData('text/plain', md);
  first.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const box = await until(() => $('.table-block', pane()), 4000);
  check('A status column shows its words as tags with a dot', box && (await until(() => $$('.tag.dot', box).length === 3)), box ? String($$('.tag.dot', box).length) : 'no table');
  check('A signed column shows a gain green and a loss red', $$('.cell.pos', box).length === 2 && $$('.cell.neg', box).length === 1, `${$$('.cell.pos', box).length} / ${$$('.cell.neg', box).length}`);
  const end = $$('.blk', pane()).find((b) => b.textContent.includes('End'));
  const makeChart = async (json) => {
    const paste = new DataTransfer();
    paste.setData('text/plain', '```chart\n' + json + '\n```');
    end.focus();
    T().setSel(end, 3, 3);
    end.dispatchEvent(new ClipboardEvent('paste', { clipboardData: paste, bubbles: true, cancelable: true }));
    await wait(350);
  };
  await makeChart('{"shape":"bars","x":"Date","by":"row","y":"Amount","add":"sum","title":"Each row"}');
  await makeChart('{"shape":"line","x":"Date","by":"row","y":"Amount","add":"sum","running":true,"title":"So far"}');
  await makeChart('{"shape":"pie","x":"Result","add":"count","title":"Good and bad"}');
  const chart = (t) => $$('.chart-block', pane()).find((c) => $('.chart-title', c)?.textContent === t);
  check('A loss draws below the line at zero, in red', await until(() => chart('Each row') && $('.chart-bar.neg', chart('Each row')) && $('.chart-bar.pos', chart('Each row')), 3000));
  check('A running total ends at the sum', await until(() => chart('So far') && /250/.test($('.chart-note', chart('So far')).textContent), 3000), chart('So far')?.textContent);
  check('A pie shows each word with its share, in its colour', await until(() => chart('Good and bad') && $$('.pie-slice', chart('Good and bad')).length === 2 && $('.pie-slice.c-green', chart('Good and bad')) && /67%/.test(chart('Good and bad').textContent), 3000), chart('Good and bad')?.textContent);
  const settings = $('.chart-settings', chart('So far'));
  settings.click();
  const panel = await until(() => $('.chart-panel', chart('So far')));
  check('The chart panel offers a pie and a running total', panel && $$('.chart-shape', panel).some((b) => b.textContent.includes('Pie')) && $('[data-chart-running]', panel)?.getAttribute('aria-checked') === 'true');
  (await until(() => $('.backdrop'))).click();
  await until(() => !$('.chart-panel'));
  await wait(200);
  snap('gains-charts');
});

await run('Move a page in the sidebar by a drag', async () => {
  await oneTab();
  await newPage('Drag Parent');
  await newPage('Drag Child');
  await newPage('Drag Third');
  const row = (title) => $$('.side-scroll .tree-item[data-fav="0"]').find((t) => t.querySelector('.tree-title')?.textContent === title);
  const top = () => $$('.side-scroll > div > .tree-item[data-fav="0"] .tree-title').map((t) => t.textContent);
  const kidsOf = (title) => $$(':scope > .tree-children > div > .tree-item .tree-title', row(title).parentElement).map((t) => t.textContent);
  // Hold a page over a part of another page's line (0 its top, 1 its bottom),
  // say what is marked, then let it go there.
  const drag = async (from, to, part) => {
    const r = to.getBoundingClientRect();
    const [x, y] = [r.left + r.width / 2, r.top + r.height * part];
    const dt = new DataTransfer();
    from.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
    const over = new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt });
    to.dispatchEvent(over);
    const marks = ['into', 'before', 'after'].filter((m) => to.classList.contains(`page-drop-${m}`)).join(',');
    to.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
    from.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
    await wait(200);
    return { marks, taken: over.defaultPrevented };
  };
  check('The new pages start at the top level, in the order they were made', top().slice(-3).join('|') === 'Drag Parent|Drag Child|Drag Third', top().join('|'));

  // Onto a page: inside it.
  let did = await drag(row('Drag Child'), row('Drag Parent'), 0.5);
  check('A page held over the middle of another is ringed as the place to go in', did.marks === 'into', did.marks);
  check('Let go there, the page goes inside it, and that page opens to show it', await until(() => row('Drag Parent')?.querySelector('.chevron.open') && kidsOf('Drag Parent').join('|') === 'Drag Child' && !top().includes('Drag Child'), 3000), `${top().join('|')} / ${row('Drag Parent') ? kidsOf('Drag Parent').join('|') : ''}`);
  check('The marks are gone after the drop', !$('.page-drop-into, .page-drop-before, .page-drop-after'));
  check('A short message says where it went', await until(() => ($('.toast')?.textContent || '').includes('Moved into Drag Parent'), 1500), $('.toast')?.textContent);

  // Not into itself or a page inside it.
  did = await drag(row('Drag Parent'), row('Drag Child'), 0.5);
  check('A page cannot go into a page that is inside it', did.marks === '' && kidsOf('Drag Parent').join('|') === 'Drag Child' && top().includes('Drag Parent'), `${did.marks} ${top().join('|')} / ${kidsOf('Drag Parent').join('|')}`);

  // Between pages.
  did = await drag(row('Drag Third'), row('Drag Parent'), 0.1);
  check('Held at the top edge of a page, a line shows above it', did.marks === 'before', did.marks);
  check('Let go there, the page goes before it', await until(() => top().indexOf('Drag Third') >= 0 && top().indexOf('Drag Third') === top().indexOf('Drag Parent') - 1, 3000), top().join('|'));
  did = await drag(row('Drag Third'), row('Drag Parent'), 0.9);
  check('The bottom edge of an open page means inside it, with its pages', did.marks === 'into' && (await until(() => kidsOf('Drag Parent').join('|') === 'Drag Child|Drag Third', 3000)), `${did.marks} / ${kidsOf('Drag Parent').join('|')}`);
  did = await drag(row('Drag Third'), row('Drag Child'), 0.1);
  check('Pages inside a page change places the same way', did.marks === 'before' && (await until(() => kidsOf('Drag Parent').join('|') === 'Drag Third|Drag Child', 3000)), kidsOf('Drag Parent').join('|'));
  did = await drag(row('Drag Third'), row('Drag Child'), 0.9);
  check('…and after', did.marks === 'after' && (await until(() => kidsOf('Drag Parent').join('|') === 'Drag Child|Drag Third', 3000)), `${did.marks} / ${kidsOf('Drag Parent').join('|')}`);
  await wait(150);
  snap('page-move');

  // Out again: below the list.
  const add = $('.side-scroll .tree-item.muted');
  did = await drag(row('Drag Child'), add, 0.5);
  check('Let go below the list, a page goes back to the top level, at the end', (await until(() => top()[top().length - 1] === 'Drag Child' && kidsOf('Drag Parent').join('|') === 'Drag Third', 3000)), `${top().join('|')} / ${kidsOf('Drag Parent').join('|')}`);
  // The page and what is inside it stay as they were.
  row('Drag Third').click();
  check('A moved page opens as before, with its place shown in its bar', await until(() => shownPane('Drag Third') && /Drag Parent/.test($('.pagebar', shownPane('Drag Third'))?.textContent || ''), 3000), $('.pagebar')?.textContent);
});

await run('The AI changes what is on the page', async () => {
  await oneTab();
  const title = await newPage('Workout');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Workout');
  const nextLine = async (from) => {
    key(from, 'Enter');
    return until(() => document.activeElement !== from && document.activeElement?.classList.contains('blk') && document.activeElement);
  };
  typeIn(first, 'Monday: run');
  const second = await nextLine(first);
  typeIn(second, 'Tuesday: rest');
  const third = await nextLine(second);
  typeIn(third, 'Old note');
  const rows = () => $$('.block-row', pane()).map((r) => r.querySelector('.blk')?.textContent.replace(/​/g, '') ?? '');
  const idOf = (text) => $$('.block-row', pane()).find((r) => r.querySelector('.blk')?.textContent.includes(text))?.dataset.id;
  const tuesday = idOf('Tuesday');
  // Ask for a change. The stand-in AI answers with the page as it must be.
  T().command('ai_anywhere');
  const box = await until(() => $('#ai-input'));
  box.value = 'make Monday a swim and take out the old note [[reply: @replace\\n# Workout\\n\\nMonday: swim\\n\\nTuesday: rest\\n\\n- [ ] Buy goggles]]';
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(box, 'Enter');
  const replace = await until(() => $('#ai-replace'), 6000);
  check('For a request to change the page, the AI box offers Replace', replace && $$('.ai-actions button').map((b) => b.textContent).join('|') === 'Replace|Insert below|Try again|Discard', $$('.ai-actions button').map((b) => b.textContent).join('|'));
  check('What it shows is the page as it will be, without the mark of its reply', $('.ai-output') && !$('.ai-output').textContent.includes('@replace') && $('.ai-output').textContent.includes('Monday: swim'), $('.ai-output')?.textContent.slice(0, 60));
  check('The page is not changed before Replace is pressed', rows().join('|').startsWith('Monday: run|Tuesday: rest|Old note'), rows().join('|'));
  replace.click();
  check('Replace puts the changed page in place of the page', await until(() => rows().join('|') === 'Monday: swim|Tuesday: rest|Buy goggles' && !$('.ai-panel'), 4000), rows().join('|'));
  check('A line that did not change is the same block as before', idOf('Tuesday') === tuesday, `${idOf('Tuesday')} / ${tuesday}`);
  check('The new to-do is a to-do', $$('.block-row.kind-todo', pane()).length === 1);
  T().command('undo');
  check('One Undo brings the page back as it was', await until(() => rows().join('|') === 'Monday: run|Tuesday: rest|Old note', 4000), rows().join('|'));
  // A question is answered as before: Insert, not Replace.
  T().command('ai_anywhere');
  const again = await until(() => $('#ai-input'));
  again.value = 'what is on Tuesday? [[reply: Tuesday is a rest day.]]';
  again.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(again, 'Enter');
  const insert = await until(() => $$('.ai-actions button').find((b) => b.textContent === 'Insert'), 6000);
  check('A question is answered with Insert, and no Replace', insert && !$('#ai-replace'));
  $$('.ai-actions button').find((b) => b.textContent === 'Discard').click();
  await until(() => !$('.ai-panel'));
});

await run('The AI makes sub-pages and links them', async () => {
  await oneTab();
  const title = await newPage('Training');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Training');
  typeIn(first, 'All the sessions in one long list');
  await wait(150);
  const rows = () => $$('.block-row', pane()).map((r) => (r.querySelector('.blk')?.textContent ?? r.textContent).replace(/\u200b/g, '').trim());
  const links = () => $$('.block-row .page-link:not(.missing) .pl-title', pane()).filter((t) => /^Session [AB]$/.test(t.textContent)).length;
  askAi();
  const box = await until(() => $('.ai-dock #ai-input'));
  check('The button at the foot of a page opens the AI bar of that page', box);
  box.value = 'give each session its own page [[reply: @replace\\n# Training\\n\\nPick a session:\\n\\n[[Session A]]\\n\\n[[session b]]\\n\\n@page Session A\\nTen squats.\\n\\n@page Session B\\n- [ ] Five push-ups]]';
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(box, 'Enter');
  const plan = await until(() => $('#ai-plan'), 6000);
  const planRows = () => $$('#ai-plan .ai-plan-row').map((r) => r.textContent.trim()).join('|');
  check('For sub-pages, the AI first says what it will do', plan && planRows() === 'Change this page|New sub-page: Session A|New sub-page: Session B|Link the sub-pages in this page', planRows());
  check('…with one button to do it, one to read the pages and one to stop', $$('.ai-actions button').map((b) => b.textContent).join('|') === 'Do it|Show the pages|Not this', $$('.ai-actions button').map((b) => b.textContent).join('|'));
  check('Nothing is made before the person says so', !treeItem('Session A') && rows().join('|') === 'All the sessions in one long list' && !$('.ai-output'), rows().join('|'));
  const dock = $('.ai-dock.answer');
  check('The card grows out of the bar with a motion', dock && $('.ai-panel', dock).getAnimations().some((a) => a.animationName === 'ai-card-grow') || getComputedStyle($('.ai-panel', dock)).animationName === 'ai-card-grow', dock && getComputedStyle($('.ai-panel', dock)).animationName);
  dock.querySelectorAll('*').forEach((el) => el.getAnimations().forEach((a) => a.finish()));
  await wait(60);
  const card = dock.getBoundingClientRect();
  const paneBox = pane().getBoundingClientRect();
  check('The card is small: it covers less than a third of the page', card.height > 60 && card.height < paneBox.height / 3 && card.width <= 481 && card.bottom <= paneBox.bottom && card.top >= paneBox.top, `${Math.round(card.width)} x ${Math.round(card.height)} in a pane ${Math.round(paneBox.height)} high`);
  await wait(250);
  snap('ai-plan');
  $('#ai-show-pages').click();
  check('The pages can be read first', await until(() => $('.ai-output')?.textContent.includes('Ten squats') && $('.ai-output').textContent.includes('Five push-ups') && !$('.ai-output').textContent.includes('@page') && $('#ai-show-pages').textContent === 'Hide the pages'), $('.ai-output')?.textContent.slice(0, 90));
  {
    const tall = $('.ai-dock').getBoundingClientRect();
    check('…and the card stays in the pane, with its own scroll for a long text', tall.top >= paneBox.top && tall.height < paneBox.height * 0.7, `${Math.round(tall.height)} of ${Math.round(paneBox.height)}`);
  }
  $('#ai-replace').click();
  check('Do it makes the sub-pages under the page, in the sidebar', await until(() => treeItem('Session A') && treeItem('Session B'), 4000));
  check('The page is changed and links to each sub-page', await until(() => rows()[0] === 'Pick a session:' && links() === 2, 3000), `${rows().join('|')} / ${links()} links`);
  const done = await until(() => $('#ai-done'), 2000);
  check('A short line at the foot says what was done, with Undo', done && /2 sub-pages made and linked/.test(done.textContent) && $('#ai-undo') && !$('.ai-panel'), done?.textContent);
  await wait(300);
  snap('ai-done');
  // A sub-page has its text.
  treeItem('Session B').click();
  check('A sub-page holds what the AI wrote for it', await until(() => shownPane('Session B') && $$('.block-row.kind-todo', shownPane('Session B')).some((r) => r.textContent.includes('Five push-ups')), 3000));
  treeItem('Training').click();
  await until(() => pane());
  // Undo at the foot takes it all back: the sub-pages and the change.
  const undo = await until(() => $('#ai-undo'), 2000);
  if (undo) {
    undo.click();
  } else {
    T().command('undo');
  }
  check('One Undo takes it all back: the page and the sub-pages', await until(() => !treeItem('Session A') && !treeItem('Session B') && rows().join('|') === 'All the sessions in one long list', 4000), `${rows().join('|')} / ${!!treeItem('Session A')}`);
  check('…and the line at the foot goes', !$('#ai-done'));
  // Redo brings the pages and the change back.
  T().command('redo');
  check('Redo brings them back', await until(() => treeItem('Session A') && treeItem('Session B') && rows()[0] === 'Pick a session:', 4000), rows().join('|'));
  T().command('undo');
  await until(() => !treeItem('Session A'), 3000);
});

await run('The AI makes a database', async () => {
  await oneTab();
  const title = await newPage('Trading home');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(first, 'My notes.');
  await wait(200);
  askAi();
  const box = await until(() => $('.ai-dock #ai-input'));
  // What the real AI gave for "Make me a trading journal" (2026-11-11), with three trades in it.
  const journal = [
    '@replace', '# Trading home', '', 'My notes.', '', '[[Trading Journal]]', '',
    '@page Trading Journal', 'Log each trade when you close it.', '',
    '| Trade | Date | Side | Setup | Entry | P&L | Result | Followed plan |',
    '| --- | --- | --- | --- | --- | --- | --- | --- |',
    '| AAPL breakout | 2026-11-10 | Long | Breakout | 185.2 | 160 | Win | x |',
    '| TSLA fade | 2026-11-11 | Short | Reversal | 242 | -80 | Loss |  |',
    '| NVDA pullback | 2026-11-09 | Long | Pullback | 131.5 | 45 | Win | x |',
    '<!-- columns: text count | date | choice {Long:green; Short:red} | choice {Breakout:blue; Pullback:purple; Reversal:orange} | number USD | number USD sum | choice {Win:green; Loss:red; Breakeven:gray} | checkbox checked -->',
    '<!-- table: Trades | icon target:green | sort Date down | frozen 1 | color P&L below 0 red | color P&L above 0 green -->',
  ].join('\\n');
  box.value = `make me a trading journal [[reply: ${journal}]]`;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(box, 'Enter');
  const go = await until(() => $('#ai-replace'), 6000);
  check('The AI offers to make the journal page', go);
  go?.click();
  check('The journal is made as a sub-page', await until(() => treeItem('Trading Journal'), 4000));
  treeItem('Trading Journal')?.click();
  const pane = await until(() => shownPane('Trading Journal'), 3000);
  const block = await until(() => pane && $('.table-block', pane), 3000);
  check('Its table is there', block);
  if (!block) return;
  const wrap = block.closest('.table-wrap');
  check('The table has the name and the icon the AI gave it', $('.table-name', wrap)?.value === 'Trades' && $('.table-icon', wrap), $('.table-name', wrap)?.value);
  const names = () => $$('tbody tr[data-r]:not(.head-row)', block).map((r) => r.querySelector('.first-cell .cell')?.value);
  check('Its rows are newest first, as the AI said', names().join('|') === 'TSLA fade|AAPL breakout|NVDA pullback', names().join('|'));
  check('Each row with a P&L has its colour', $$('tbody tr.painted', block).length === 3, $$('tbody tr.painted', block).length);
  check('The options are coloured tags', $$('.tag.c-green', block).length >= 4 && $$('.tag.c-red', block).length === 2 && $$('.tag.c-purple', block).length === 1, `${$$('.tag.c-green', block).length} green, ${$$('.tag.c-red', block).length} red`);
  check('The first column stays in place', $$('.frozen', block).length > 0);
  check('Money shows as money, and the P&L adds up at the foot', $$('textarea.cell', block).some((t) => t.value === '$185.20') && $('.table-totals', block)?.textContent.includes('$125.00'), $('.table-totals', block)?.textContent);
  check('The ticks show as ticks', $$('tbody .cell-check.on', block).length === 2, $$('tbody .cell-check.on', block).length);
  await wait(250);
  snap('ai-database');
});

await run('Diagrams', async () => {
  await oneTab();
  const title = await newPage('Diagram home');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(first, 'Home.');
  await wait(200);
  askAi();
  const box = await until(() => $('.ai-dock #ai-input'));
  // A page with a diagram, as the AI writes one, and one with a mistake in it.
  const md = ['Steps of a trade.', '', '```mermaid', 'flowchart LR', '  A[Plan] --> B[Enter]', '  B --> C[Manage]', '  C --> D[Exit]', '  D --> E[Review]', '  E -. lessons .-> A', '```', '', '```mermaid', 'flowchart LR', '  A[Broken --> ', '```'].join('\n');
  box.value = `make a diagram of my trade steps [[tool: create_page ${JSON.stringify({ title: 'Trade steps', markdown: md })}]] [[reply: I made the page Trade steps.]]`;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(box, 'Enter');
  check('The AI makes the page with the diagram', await until(() => treeItem('Trade steps'), 8000));
  ($('#ai-ok') || $('#ai-done-run'))?.click();
  treeItem('Trade steps')?.click();
  const pane = await until(() => shownPane('Trade steps'), 3000);
  if (!pane) return;
  const wraps = () => $$('.diagram-wrap', pane);
  const drawn = await until(() => $('.diagram svg', pane), 10000);
  check('A code block of the language Mermaid is drawn as a diagram', drawn && wraps().length === 2, `${wraps().length} diagrams | ${$$('.diagram', pane).map((d) => `${d.className} [${d.dataset.drawn ? 'tried' : 'not tried'}] ${d.textContent.slice(0, 160)}`).join(' / ')} | lib ${typeof window.mermaid}`);
  if (!drawn) return;
  const words = drawn.textContent;
  check('The diagram has every step and the label of the way back', ['Plan', 'Enter', 'Manage', 'Exit', 'Review', 'lessons'].every((w) => words.includes(w)), words);
  const box1 = drawn.querySelector('.node rect, .node polygon, .node path');
  const fill = box1 && getComputedStyle(box1).fill;
  check('Its boxes take the colours of the app, not the library’s own', fill && !/236, 236, 255/.test(fill) && !/ececff/i.test(fill), fill);
  check('Its code is out of sight while it shows the diagram, and its language says Mermaid', getComputedStyle($('.diagram-code', wraps()[0])).display === 'none' && $('.code-lang', wraps()[0])?.value === 'Mermaid', $('.code-lang', wraps()[0])?.value);
  check('Its boxes have round corners', [...drawn.querySelectorAll('.node rect')].some((r) => Number(r.getAttribute('rx')) > 0));
  const wrong = await until(() => $('.diagram.wrong', pane), 6000);
  check('A diagram with a mistake says so in the page', wrong && /cannot be drawn/.test(wrong.textContent), wrong?.textContent);
  check('…and no error drawing of the library is left in the window', !$('body > [id^="dn2-diagram"], body > svg[id^="n2-diagram"]'));
  $('[data-diagram-toggle]', wraps()[0]).click();
  check('"Edit" shows the code under the diagram', await until(() => wraps()[0].classList.contains('coding') && getComputedStyle($('.diagram-code', wraps()[0])).display === 'block' && $('.diagram-code .blk', wraps()[0])?.textContent.includes('flowchart LR'), 3000));
  $('[data-diagram-toggle]', wraps()[0]).click();
  check('"Done" goes back to the diagram', await until(() => !wraps()[0].classList.contains('coding'), 3000));
  await wait(300);
  snap('diagram');
});

await run('Screenshots in a picture cell', async () => {
  await oneTab();
  const title = await newPage('Shots home');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(first, 'Home.');
  await wait(200);
  askAi();
  const box = await until(() => $('.ai-dock #ai-input'));
  const md = ['| Trade | Chart |', '| --- | --- |', '| AAPL |  |', '<!-- columns: text | file -->'].join('\n');
  box.value = `make a table [[tool: create_page ${JSON.stringify({ title: 'Trade shots', markdown: md })}]] [[reply: I made the page Trade shots.]]`;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(box, 'Enter');
  await until(() => treeItem('Trade shots'), 8000);
  ($('#ai-ok') || $('#ai-done-run'))?.click();
  treeItem('Trade shots')?.click();
  const pane = await until(() => shownPane('Trade shots'), 3000);
  const cell = await until(() => pane && $('[data-pic-cell]', pane), 3000);
  check('A picture cell can be chosen to take a picture', cell && cell.tabIndex === 0);
  if (!cell) return;
  // A screenshot: a small picture made here, as the Mac puts one on the clipboard.
  const shot = async (color) => {
    const canvas = document.createElement('canvas');
    canvas.width = 24;
    canvas.height = 16;
    const g = canvas.getContext('2d');
    g.fillStyle = color;
    g.fillRect(0, 0, 24, 16);
    const blob = await new Promise((done) => canvas.toBlob(done, 'image/png'));
    return new File([blob], 'Screenshot.png', { type: 'image/png' });
  };
  const pics = () => $$('[data-pic-cell] .cell-pic img', pane).length;
  cell.focus();
  const clip = new DataTransfer();
  clip.items.add(await shot('#2e9d5b'));
  cell.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  check('⌘V with the cell chosen puts the screenshot in the cell', await until(() => pics() === 1, 4000), pics());
  check('…and not in the page as a picture of its own', !$('.image-block img, .blk-image img', pane) && $$('.block-row.kind-image', pane).length === 0);
  const drop = new DataTransfer();
  drop.items.add(await shot('#d94848'));
  const target = $('[data-pic-cell]', pane);
  target.dispatchEvent(new DragEvent('dragover', { dataTransfer: drop, bubbles: true, cancelable: true }));
  check('A picture dragged over the cell marks it', target.classList.contains('pic-drop'));
  target.dispatchEvent(new DragEvent('drop', { dataTransfer: drop, bubbles: true, cancelable: true }));
  check('A picture dropped on the cell goes in beside the first', await until(() => pics() === 2, 4000), pics());
  check('The mark goes when the picture is in', !$('[data-pic-cell].pic-drop', pane));
  await wait(200);
  snap('cell-screenshots');
});

await run('The AI does things in Rune with its tools', async () => {
  await oneTab();
  const title = await newPage('Tools page');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Tools page');
  typeIn(first, 'A line that stays');
  await wait(200);
  const rows = () => $$('.block-row', pane()).map((r) => (r.querySelector('.blk')?.textContent ?? r.textContent).replace(/​/g, '').trim());
  const here = treeItem('Tools page')?.dataset.page;
  check('A page has its id in the sidebar (the check needs it)', here);
  askAi();
  const box = await until(() => $('.ai-dock #ai-input'));
  // The stand-in AI uses the real tools: a new page, a line added to this page, an event in the calendar.
  box.value = `make a page for my runs, note it here, and plan a run [[tool: create_page {"title":"Run log","markdown":"5 km on Monday."}]] [[tool: append_to_page {"page_id":"${here}","markdown":"See the run log."}]] [[tool: create_event {"title":"Morning run","start":"2031-03-04T07:00"}]] [[reply: I made the page Run log, added a line to this page, and put Morning run in the calendar.]]`;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(box, 'Enter');
  // What it is at shows at once, before a tool call is complete (a long page takes a while), with a clock.
  const seen = { writing: '', clock: '', step: '' };
  await until(() => {
    const line = $('#ai-step')?.textContent || '';
    if (/^Writing the page “Run log”/.test(line)) seen.writing = line;
    if (/Made the page|Added/.test(line)) seen.step = line;
    seen.clock = $('.ai-clock')?.textContent || seen.clock;
    return seen.writing && seen.step && seen.clock;
  }, 3000, 5);
  const step = seen.step;
  check('While the AI writes a page, the card says so at once', seen.writing, JSON.stringify(seen));
  check('…with a clock that shows how long the run goes on', /^\d+:\d\d$/.test(seen.clock), seen.clock);
  check('While it works, the card says the step it is at', step, String(step));
  const did = await until(() => $('#ai-did'), 8000);
  const lines = () => $$('#ai-did .ai-plan-row').map((r) => r.textContent.trim()).join('|');
  check('At the end the AI says what it did, a line for each thing', did && lines() === 'Made the page “Run log”|Changed the page “Tools page”|Added “Morning run” to the calendar', lines());
  check('Its answer shows, with Done and Undo (it is done already: no Insert, no Try again)', $('.ai-output')?.textContent.includes('I made the page Run log') && $$('.ai-actions button').map((b) => b.textContent).join('|') === 'Done|Undo', $$('.ai-actions button').map((b) => b.textContent).join('|'));
  check('The new page is in the sidebar and the new line is on this page, with no press of a button', await until(() => treeItem('Run log') && rows().join('|') === 'A line that stays|See the run log.', 6000), rows().join('|'));
  {
    $('.ai-dock').querySelectorAll('*').forEach((el) => el.getAnimations().forEach((a) => a.finish()));
    await wait(60);
    const card = $('.ai-dock').getBoundingClientRect();
    const paneBox = pane().getBoundingClientRect();
    check('The card stays small', card.height < paneBox.height / 3 && card.width <= 481, `${Math.round(card.width)} x ${Math.round(card.height)} in a pane ${Math.round(paneBox.height)} high`);
  }
  await wait(250);
  snap('ai-did');
  $('#ai-undo-run').click();
  check('Undo takes all of it back: the new page goes and the line goes', await until(() => !treeItem('Run log') && rows().join('|') === 'A line that stays' && !$('.ai-panel'), 6000), `${rows().join('|')} / ${!!treeItem('Run log')}`);
  // A second run, kept with Done: a line at the foot says what was done, with Undo for some seconds.
  askAi();
  const again = await until(() => $('.ai-dock #ai-input'));
  again.value = 'one more page [[tool: create_page {"title":"Second log"}]] [[reply: I made the page Second log.]]';
  again.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(again, 'Enter');
  const ok = await until(() => $('#ai-ok'), 8000);
  check('The second run made its page', ok && (await until(() => treeItem('Second log'), 5000)));
  ok?.click();
  const pill = await until(() => $('#ai-done'), 3000);
  check('After Done, a line at the foot says what was done, with Undo', pill && /1 page made/.test(pill.textContent) && $('#ai-undo') && !$('.ai-panel') && treeItem('Second log'), pill?.textContent);
  $('#ai-undo')?.click();
  check('…and that Undo takes it back too', await until(() => !treeItem('Second log') && !$('#ai-done'), 6000));
  // A picture (a screenshot) pasted into the bar goes to the AI with the request.
  askAi();
  const shotBox = await until(() => $('.ai-dock #ai-input'));
  const pictureFile = async () => {
    const canvas = document.createElement('canvas');
    canvas.width = 40;
    canvas.height = 30;
    const g = canvas.getContext('2d');
    g.fillStyle = '#3a7';
    g.fillRect(0, 0, 40, 30);
    const blob = await new Promise((done) => canvas.toBlob(done, 'image/png'));
    return new File([blob], 'shot.png', { type: 'image/png' });
  };
  const pasteInto = async (el) => {
    const data = new DataTransfer();
    data.items.add(await pictureFile());
    el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  };
  const blocksBefore = $$('.block-row', pane()).length;
  await pasteInto(shotBox);
  const shot = await until(() => $('.ai-dock .ai-shot img'), 3000);
  check('A picture pasted into the AI bar shows there, small', shot && shot.src.startsWith('data:image/jpeg;base64,') && shot.getBoundingClientRect().width <= 30 && $('.ai-dock').getBoundingClientRect().height <= 46, shot ? `${Math.round(shot.getBoundingClientRect().width)} wide` : 'no picture');
  check('…and it is not put into the page', $$('.block-row', pane()).length === blocksBefore && !$('.kind-image', pane()));
  $('.ai-dock .ai-shot-x').click();
  check('A press on it takes it out again', await until(() => !$('.ai-dock .ai-shot'), 2000));
  await pasteInto($('.ai-dock #ai-input'));
  await until(() => $('.ai-dock .ai-shot img'), 3000);
  await pasteInto($('.ai-dock #ai-input'));
  await until(() => $$('.ai-dock .ai-shot').length === 2, 3000);
  const withShots = $('.ai-dock #ai-input');
  withShots.value = 'what is in these? [[reply: I see {pictures} pictures.]]';
  withShots.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(withShots, 'Enter');
  check('The pictures go to the AI with the request', await until(() => $('.ai-output')?.textContent.includes('I see 2 pictures.'), 6000) && $('.ai-caption')?.textContent.includes('2 pictures'), `${$('.ai-output')?.textContent} / ${$('.ai-caption')?.textContent}`);
  $$('.ai-actions button').find((b) => b.textContent === 'Discard').click();
  await until(() => !$('.ai-panel'));
  // A large job: the AI says its plan first, and does nothing before "Do it".
  askAi();
  const big = await until(() => $('.ai-dock #ai-input'));
  big.value = 'build me a plan of weeks [[tool: create_page {"title":"Week 1"}]] [[tool: create_page {"title":"Week 2"}]] [[then: I made the two week pages.]] [[reply: @plan\\nTwo pages, one for each week.\\n- New page: Week 1\\n- New page: Week 2]]';
  big.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(big, 'Enter');
  const go = await until(() => $('#ai-plan-go'), 8000);
  const planRows = () => $$('#ai-plan .ai-plan-row').map((r) => r.textContent.trim()).join('|');
  check('For a large job the AI shows its plan first', go && planRows() === 'New page: Week 1|New page: Week 2' && $('.ai-output')?.textContent.trim() === 'Two pages, one for each week.' && !$('.ai-output').textContent.includes('@plan'), `${planRows()} / ${$('.ai-output')?.textContent}`);
  check('…with Do it, Change the plan and Not this, and nothing is made yet', $$('.ai-actions button').map((b) => b.textContent).join('|') === 'Do it|Change the plan|Not this' && !treeItem('Week 1') && !$('#ai-did'), $$('.ai-actions button').map((b) => b.textContent).join('|'));
  await wait(250);
  snap('ai-job-plan');
  // "Change the plan" goes back to the request, with its words still there.
  $('#ai-plan-change').click();
  const back = await until(() => $('.ai-dock #ai-input'), 3000);
  check('Change the plan goes back to the request, with its words', back && back.value.startsWith('build me a plan of weeks') && !$('#ai-plan'), back?.value.slice(0, 40));
  await until(() => $('#ai-send'), 3000);
  key(back, 'Enter');
  const go2 = await until(() => $('#ai-plan-go'), 8000);
  go2?.click();
  const made = await until(() => $('#ai-did'), 8000);
  check('Do it does the work: the pages are made and listed', made && $$('#ai-did .ai-plan-row').map((r) => r.textContent.trim()).join('|') === 'Made the page “Week 1”|Made the page “Week 2”' && (await until(() => treeItem('Week 1') && treeItem('Week 2'), 5000)) && $('.ai-output')?.textContent.includes('I made the two week pages'), $('#ai-did')?.textContent);
  $('#ai-undo-run').click();
  check('…and one Undo takes the job back', await until(() => !treeItem('Week 1') && !treeItem('Week 2') && !$('.ai-panel'), 6000));
  // A question is answered as before: no list of things done, and Insert.
  askAi();
  const third = await until(() => $('.ai-dock #ai-input'));
  third.value = 'what is on this page? [[reply: One line.]]';
  third.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ai-send'));
  key(third, 'Enter');
  const insert = await until(() => $$('.ai-actions button').find((b) => b.textContent === 'Insert'), 6000);
  check('A plain answer has no list of things done', insert && !$('#ai-did') && !$('#ai-ok'));
  $$('.ai-actions button').find((b) => b.textContent === 'Discard').click();
  await until(() => !$('.ai-panel'));
});

await run('Select all, copy and delete', async () => {
  await oneTab();
  const title = await newPage('Pick me');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Pick me');
  const nextLine = async (from) => {
    key(from, 'Enter');
    return until(() => document.activeElement !== from && document.activeElement?.classList.contains('blk') && document.activeElement);
  };
  typeIn(first, 'First line');
  const second = await nextLine(first);
  typeIn(second, '---');
  // "---" makes a divider and a new line after it.
  const third = await until(() => $('.kind-divider', pane()) && document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(third, 'Last line');
  const rows = () => $$('.block-row', pane());
  const picked = () => $$('.block-row.picked', pane());
  const texts = () => rows().map((r) => `${[...r.classList].find((c) => c.startsWith('kind-')).slice(5)}:${r.querySelector('.blk')?.textContent.replace(/​/g, '') ?? ''}`).join('|');
  const cmd = (el, k) => key(el, k, { metaKey: true });
  check('The page has a line, a divider and a line', texts() === 'text:First line|divider:|text:Last line', texts());

  // In a line, ⌘A is first for the text of that line.
  T().setSel(third, 2);
  cmd(third, 'a');
  await wait(200);
  check('With the caret in a line, the first ⌘A is left for the text of the line', picked().length === 0);
  // With all of the line selected, ⌘A picks the whole page.
  T().setSel(third, 0, 9);
  cmd(third, 'a');
  check('⌘A again picks every block of the page', await until(() => picked().length === 3, 3000), String(picked().length));
  check('The picked blocks are marked, and no line has the caret', picked().every((r) => getComputedStyle(r).backgroundColor !== 'rgba(0, 0, 0, 0)') && !document.activeElement?.classList.contains('blk'));
  delete document.body.dataset.copied;
  cmd(document.body, 'c');
  check('⌘C copies them all, as text with their marks', await until(() => document.body.dataset.copied === 'First line\n\n---\n\nLast line', 3000), JSON.stringify(document.body.dataset.copied));
  check('They stay picked after the copy', picked().length === 3);
  key(document.body, 'Escape');
  check('Escape lets them go', await until(() => picked().length === 0, 3000));

  // A drag of the text selection from one line into another picks the lines it goes over.
  {
    const lines = $$('.blk', pane());
    const at = (el, dy = 0) => {
      const r = el.getBoundingClientRect();
      return { clientX: r.left + 20, clientY: r.top + r.height / 2 + dy };
    };
    const move = (el, type, opts) => el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, ...opts }));
    move(lines[0], 'mousedown', { ...at(lines[0]), buttons: 1 });
    move(lines[0], 'mousemove', { ...at(lines[0]), buttons: 1 });
    await wait(80);
    check('A drag inside one line does not pick it', picked().length === 0);
    move(lines[1], 'mousemove', { ...at(lines[1]), buttons: 1 });
    check('Dragged into another line, the drag picks both lines and what is between them', await until(() => picked().length === 3, 3000), String(picked().length));
    move(lines[0], 'mousemove', { ...at(lines[0]), buttons: 1 });
    check('Dragged back into the first line, it is a text selection again', await until(() => picked().length === 0, 3000), String(picked().length));
    move($('.kind-divider', pane()), 'mousemove', { ...at($('.kind-divider', pane())), buttons: 1 });
    await until(() => picked().length === 2, 3000);
    window.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, button: 0 }));
    delete document.body.dataset.copied;
    cmd(document.body, 'c');
    check('Let go, ⌘C copies the picked lines', await until(() => document.body.dataset.copied === 'First line\n\n---', 3000), JSON.stringify(document.body.dataset.copied));
    key(document.body, 'Escape');
    await until(() => picked().length === 0, 3000);
  }

  // A press on a block that is not text picks it; Delete takes it out.
  const mouse = (el, type, opts = {}) => el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, ...opts }));
  mouse($('.kind-divider .divider', pane()), 'mousedown');
  check('A press on a divider picks it', await until(() => picked().length === 1 && picked()[0].classList.contains('kind-divider'), 3000), String(picked().length));
  key(document.body, 'Delete');
  check('Delete takes the picked block out', await until(() => texts() === 'text:First line|text:Last line', 3000), texts());
  T().command('undo');
  check('Undo brings it back', await until(() => texts() === 'text:First line|divider:|text:Last line', 3000), texts());
  // With Shift, a press picks up to there.
  mouse($('.kind-divider .divider', pane()), 'mousedown');
  await until(() => picked().length === 1, 3000);
  mouse($$('.blk', pane())[1], 'mousedown', { shiftKey: true });
  check('A press with Shift picks all the blocks up to there', await until(() => picked().length === 2 && picked()[1].querySelector('.blk')?.textContent.includes('Last line'), 3000), String(picked().length));
  // A press in text lets them go and is a normal press.
  mouse($$('.blk', pane())[0], 'mousedown');
  check('A press in a line of text lets them go', await until(() => picked().length === 0, 3000));

  // The whole page: pick, cut, and the page is empty; undo brings it all back.
  $$('.blk', pane())[0].focus();
  T().setSel($$('.blk', pane())[0], 0, 10);
  cmd($$('.blk', pane())[0], 'a');
  await until(() => picked().length === 3, 3000);
  delete document.body.dataset.copied;
  cmd(document.body, 'x');
  check('⌘X copies the whole page and leaves one empty line', await until(() => texts() === 'text:' && document.body.dataset.copied === 'First line\n\n---\n\nLast line', 3000), `${texts()} / ${JSON.stringify(document.body.dataset.copied)}`);
  check('The caret is in that line', await until(() => document.activeElement === $('.blk', pane()), 2000));
  T().command('undo');
  check('One Undo brings the whole page back', await until(() => texts() === 'text:First line|divider:|text:Last line', 3000), texts());
});

await run('Links between notes', async () => {
  await oneTab();
  await newPage('Link Target');
  const title = await newPage('Link Source');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = (name) => shownPane(name);
  const menu = () => $('.slash-menu.page-links');
  const names = () => $$('.page-links .menu-item .link-title').map((t) => t.textContent);
  const text = (el) => el.textContent.replace(/​/g, '');

  // "@" and the start of a name.
  typeIn(first, 'See @Link Tar');
  check('"@" and the start of a name lists the pages that fit', await until(() => menu() && names().join('|') === 'Link Target', 3000), names().join('|'));
  check('The page itself is not in the list', !names().includes('Link Source'));
  key(first, 'Enter');
  const link = await until(() => $('a[href^="notion2://page/"]', first), 3000);
  check('Return puts a link to the page in place of what was typed', link && link.textContent === 'Link Target' && text(first).startsWith('See Link Target') && !text(first).includes('@') && !menu(), text(first));
  check('The link looks like a tag, not like a web address', link && getComputedStyle(link).textDecorationLine === 'none' && getComputedStyle(link).backgroundColor !== 'rgba(0, 0, 0, 0)');
  check('Typing goes on after the link', document.activeElement === first && T().selIn(first)?.start === text(first).length, `${T().selIn(first)?.start} of ${text(first).length}`);

  // "@" with nothing that fits stays text, and Return stays Return.
  key(first, 'Enter');
  const second = await until(() => document.activeElement !== first && document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(second, 'Lunch @5pm');
  await wait(300);
  check('"@" with no page that fits opens no list', !menu() && second.dataset.menu !== '1', String(second.dataset.menu));
  typeIn(second, ' mail a@b.co');
  await wait(200);
  check('An email address opens no list', !menu());

  // "[[" also offers a new page.
  key(second, 'Enter');
  const third = await until(() => document.activeElement !== second && document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(third, '[[Fresh Idea');
  const fresh = await until(() => $('.page-links [data-link-new]'), 3000);
  check('"[[" and a name that no page has offers a new page of that name', fresh && /New page “Fresh Idea”/.test(fresh.textContent), fresh?.textContent);
  key(third, 'Enter');
  const made = await until(() => $('a[href^="notion2://page/"]', third), 3000);
  check('Return makes the page inside this one and links it', made && made.textContent === 'Fresh Idea' && (await until(() => treeItem('Fresh Idea'), 3000)) && shownPane('Link Source'), made?.textContent);

  // The page that is linked shows what links to it.
  link.click();
  await until(() => pane('Link Target'), 3000);
  const back = await until(() => $('.backlinks .backlink', pane('Link Target')), 3000);
  check('A click on the link opens the page', pane('Link Target'));
  check('The page shows the pages that link to it', back && $$('.backlinks .backlink', pane('Link Target')).map((b) => b.textContent.trim()).join('|') === 'Link Source' && /Linked from/.test($('.backlinks', pane('Link Target')).textContent), $('.backlinks', pane('Link Target'))?.textContent);
  await wait(150);
  snap('backlinks');

  // A new name for the page goes to the links.
  const renamed = $('.title-input', pane('Link Target'));
  renamed.value = 'Link Goal';
  renamed.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => treeItem('Link Goal'), 3000);
  back.click();
  await until(() => pane('Link Source'), 3000);
  const again = await until(() => $$('a[href^="notion2://page/"]', pane('Link Source')).find((a) => a.textContent === 'Link Goal'), 3000);
  check('From there, a click goes back to the page with the link', pane('Link Source'));
  check('The link has the page’s new name', again, $$('a[href^="notion2://page/"]', pane('Link Source')).map((a) => a.textContent).join('|'));
  check('A page with no links to it shows no such line', !$('.backlinks', pane('Link Source')));
  snap('page-link');
});

await run('A screenshot added in the middle of text', async () => {
  await oneTab();
  const title = await newPage('Shots');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Shots');
  typeIn(first, 'Above');
  key(first, 'Enter');
  const second = await until(() => document.activeElement !== first && document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(second, 'Below');
  // The caret goes back to the first line, and a screenshot is pasted there.
  first.focus();
  T().setSel(first, 5);
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), (c) => c.charCodeAt(0));
  const clip = new DataTransfer();
  clip.items.add(new File([png], 'Screenshot.png', { type: 'image/png' }));
  first.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  await until(() => $('.image-block > img', pane()), 4000);
  const kinds = () => $$('.block-row', pane()).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-')).slice(5)}:${r.querySelector('.blk')?.textContent.replace(/​/g, '') ?? ''}`);
  check('The screenshot goes under the line with the caret, and an empty line under the screenshot', await until(() => kinds().join('|') === 'text:Above|image:|text:|text:Below', 3000), kinds().join('|'));
  const at = document.activeElement;
  check('The typing line is that empty line, not the text that was after', at?.classList.contains('blk') && at.closest('.block-row') === $('.image-block', pane()).closest('.block-row').nextElementSibling && at.textContent.replace(/​/g, '') === '', at?.textContent);
  check('It is below the picture and in view', at && at.getBoundingClientRect().top >= $('.image-block', pane()).getBoundingClientRect().bottom - 1 && at.getBoundingClientRect().bottom <= $('.scroller', pane().closest('.pane') || document).getBoundingClientRect().bottom);
});

await run('A YouTube link shows its video', async () => {
  await oneTab();
  const title = await newPage('Videos');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Videos');
  typeIn(first, 'https://youtu.be/jNQXAC9IVRw?t=5');
  const card = await until(() => $('.video-embed', pane()), 3000);
  check('A line that is only a YouTube link shows the video under it', card && card.dataset.video === 'jNQXAC9IVRw' && card.closest('.block-row') === first.closest('.block-row') && $('.video-play', card), card?.outerHTML.slice(0, 140));
  check('It is a button with a name, and no player is loaded in the page', card && !$('iframe', pane()) && card.tagName === 'BUTTON' && card.title);
  const r = card ? card.getBoundingClientRect() : { width: 0, height: 1, top: 0 };
  check('It has the shape of a video and sits under its line', Math.abs(r.width / r.height - 16 / 9) < 0.02 && r.width >= 200 && r.width <= 260 && r.top >= first.getBoundingClientRect().bottom - 1, `${r.width} x ${r.height}`);
  T().setSel(first, first.textContent.length);
  key(first, 'Enter');
  const second = await until(() => document.activeElement !== first && document.activeElement?.classList.contains('blk') && document.activeElement);
  check('The next line is below the video', second && second.getBoundingClientRect().top >= $('.video-embed', pane()).getBoundingClientRect().bottom - 1);
  typeIn(second, 'See https://youtu.be/jNQXAC9IVRw later');
  await wait(400);
  check('A link inside a sentence stays a link: no second video', $$('.video-embed', pane()).length === 1, String($$('.video-embed', pane()).length));
  check('Its play button says that the video plays here, and keeps the place the link names', $('.video-embed', pane()).title.includes('here') && $('.video-embed', pane()).dataset.watch === 'https://www.youtube.com/watch?v=jNQXAC9IVRw&t=5s', $('.video-embed', pane()).dataset.watch);
  await wait(150);
  snap('video');
  // A click plays it in the corner of the window (a test run has the frame, and no player).
  $('.video-embed', pane()).click();
  const dock = await until(() => $('#player-dock'), 3000);
  const screen = dock && $('#player-screen', dock).getBoundingClientRect();
  check('A click opens a small player in the corner of the window, for that video', dock && dock.dataset.video === 'jNQXAC9IVRw' && Math.abs(screen.width / screen.height - 16 / 9) < 0.03 && innerWidth - dock.getBoundingClientRect().right < 40 && innerHeight - dock.getBoundingClientRect().bottom < 40, dock && `${Math.round(screen.width)} x ${Math.round(screen.height)}`);
  check('The page itself still holds no player of YouTube', !$('iframe', document.body) && $('#player-browser', dock) && $('#player-close', dock));
  await wait(200);
  snap('player');
  // The player is moved by its bar and gets another size by a corner.
  const press = (type, x, y, target, down = 1) => target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, buttons: down, clientX: x, clientY: y }));
  const pull = async (el, dx, dy) => {
    const b = el.getBoundingClientRect();
    const from = { x: b.left + b.width / 2, y: b.top + b.height / 2 };
    press('mousedown', from.x, from.y, el);
    press('mousemove', from.x + dx / 2, from.y + dy / 2, window);
    press('mousemove', from.x + dx, from.y + dy, window);
    press('mouseup', from.x + dx, from.y + dy, window, 0);
    await wait(250);
  };
  const box = () => $('#player-dock').getBoundingClientRect();
  const at0 = box();
  check('The player starts at the bottom right corner', dock.dataset.h === 'right' && dock.dataset.v === 'bottom' && Math.abs(innerWidth - at0.right - 18) < 1.5 && Math.abs(at0.width - 440) < 1.5, `${dock.dataset.h} ${dock.dataset.v} ${Math.round(at0.width)}`);
  await pull($('.player-title', dock), -220, -160);
  const at1 = box();
  check('A drag on its bar moves the player', Math.abs(at1.left - (at0.left - 220)) < 2 && Math.abs(at1.top - (at0.top - 160)) < 2 && Math.abs(at1.width - at0.width) < 1, `${Math.round(at1.left - at0.left)}, ${Math.round(at1.top - at0.top)}`);
  check('…and its frame goes with it', Math.abs($('#player-screen').getBoundingClientRect().left - at1.left) < 12);
  // The player itself is a view over the page: it goes where the app is told the frame is.
  const told = async () => {
    const f = $('#player-screen').getBoundingClientRect();
    const want = [f.left, f.top, f.width, f.height].map((n) => Math.round(n));
    return until(() => {
      const got = (document.body.dataset.playerAt || '').split(',').map(Number);
      return got.length === 4 && got.every((n, i) => Math.abs(n - want[i]) <= 1);
    }, 2000);
  };
  check('…and the app is told the new place of the frame, for the player', await told(), document.body.dataset.playerAt);
  // A corner: the corner across from it stays, the frame keeps its shape.
  await pull($('.player-grip[data-corner="nw"]', dock), -120, 0);
  const at2 = box();
  const frame = $('#player-screen').getBoundingClientRect();
  check('A drag on a corner makes it larger; the corner across from it stays', Math.abs(at2.width - (at1.width + 120)) < 2 && Math.abs(at2.right - at1.right) < 2 && Math.abs(at2.bottom - at1.bottom) < 2, `${Math.round(at2.width)} wide, right ${Math.round(at2.right - at1.right)}, bottom ${Math.round(at2.bottom - at1.bottom)}`);
  check('…and the picture keeps its shape, 16 to 9', Math.abs(frame.width / frame.height - 16 / 9) < 0.02, String(frame.width / frame.height));
  check('…and the app is told the new size of the frame', await told(), document.body.dataset.playerAt);
  await pull($('.player-grip[data-corner="se"]', dock), -2000, -2000);
  check('It does not get smaller than it can be used', Math.abs(box().width - 260) < 2, String(box().width));
  // It never leaves the window.
  await pull($('.player-title', dock), -5000, -5000);
  const at3 = box();
  check('It stays inside the window', at3.left >= 7 && at3.top >= 7 && $('#player-dock').dataset.h === 'left' && $('#player-dock').dataset.v === 'top', `${Math.round(at3.left)}, ${Math.round(at3.top)}`);
  await wait(150);
  snap('player-moved');
  // Two clicks on the bar: large, and small again.
  $('.player-title', dock).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  check('Two clicks on the bar make it large', await until(() => box().width > 600, 2000), String(box().width));
  $('.player-title', dock).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  check('…and two more make it small again', await until(() => Math.abs(box().width - 440) < 2, 2000), String(box().width));
  const kept = box();
  $('#player-close', dock).click();
  check('Its close button puts it away', await until(() => !$('#player-dock'), 2000));
  $('.video-embed', pane()).click();
  const again = await until(() => $('#player-dock'), 3000);
  const back = again ? again.getBoundingClientRect() : { left: -1, top: -1, width: 0 };
  check('It opens again where the person left it, at that size', again && Math.abs(back.left - kept.left) < 2 && Math.abs(back.top - kept.top) < 2 && Math.abs(back.width - kept.width) < 2, `${Math.round(back.left)}, ${Math.round(back.top)}, ${Math.round(back.width)}`);
  $('#player-close', again)?.click();
  await until(() => !$('#player-dock'), 2000);
  // The picture can be put away; the link stays. The menu of the line brings it back.
  const hide = $('.video-card .video-hide', pane());
  check('The picture of the video has a button to remove it', hide && hide.getAttribute('aria-label') && hide.closest('.video-card').contains($('.video-embed', pane())));
  hide?.click();
  const gone = await until(() => !$('.video-embed', pane()), 3000);
  check('The button removes the picture, and the link stays in the line', gone && first.textContent.includes('youtu.be/jNQXAC9IVRw') && !$('.video-card', pane()), `gone ${!!gone}, text ${first.textContent.slice(0, 60)}`);
  first.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: first.getBoundingClientRect().left + 12, clientY: first.getBoundingClientRect().top + 8 }));
  const lineMenu = await until(() => $('.block-menu'), 3000);
  const showIt = lineMenu && $('#show-video', lineMenu);
  check('The menu of the line can show the video again', showIt && showIt.textContent.includes('Show the video'));
  showIt?.click();
  check('…and it comes back', await until(() => $('.video-embed', pane()) && !$('.block-menu'), 3000));
  // The words of the line change: it is no longer only a link.
  first.focus();
  T().setSel(first, 0);
  typeIn(first, 'Watch ');
  check('When the line is no longer only the link, the video goes', await until(() => !$('.video-embed', pane()), 3000));
});

await run('Pictures anywhere on a page, and large on a click', async () => {
  await oneTab();
  const title = await newPage('Board');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Board');
  const nextLine = async (from) => {
    key(from, 'Enter');
    return until(() => document.activeElement !== from && document.activeElement?.classList.contains('blk') && document.activeElement);
  };
  typeIn(first, 'One');
  const second = await nextLine(first);
  typeIn(second, 'Two');
  const third = await nextLine(second);
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), (c) => c.charCodeAt(0));
  const clip = new DataTransfer();
  clip.items.add(new File([png], 'Screenshot.png', { type: 'image/png' }));
  third.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const pasted = await until(() => $('.image-block > img', pane()), 4000);
  await until(() => pasted.complete && pasted.naturalWidth === 1, 5000);
  const box = () => $('.image-block', pane());
  const pic = () => $('.image-block > img', pane());
  const rowOf = (el) => el.closest('.block-row');
  const at = (el) => el.getBoundingClientRect();
  const col = () => at($('.blocks', pane()));
  const inner = (text) => $$('.block-row', pane()).find((r) => (r.querySelector('.blk')?.textContent.replace(/​/g, '') ?? null) === text)?.querySelector('.block-inner');
  // Do two boxes keep clear of each other?
  const apart = (a, b) => a.right <= b.left + 0.5 || b.right <= a.left + 0.5 || a.bottom <= b.top + 0.5 || b.bottom <= a.top + 0.5;
  // Is every line of the page clear of the picture (not under it, not over it)?
  const clear = () => $$('.block-row:not(.free) > .block-inner', pane()).every((el) => apart(at(el), at(box())));
  const mouse = (el, type, x, y) => el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y }));
  // Press on the middle of something and pull it; the answer lets it go.
  const pull = (el, dx, dy) => {
    const r = at(el);
    const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
    mouse(el, 'mousedown', x, y);
    mouse(document, 'mousemove', x + dx / 2, y + dy / 2);
    mouse(document, 'mousemove', x + dx, y + dy);
    return () => mouse(document, 'mouseup', x + dx, y + dy);
  };
  const press = (el) => {
    const r = at(el);
    mouse(el, 'mousedown', r.left + r.width / 2, r.top + r.height / 2);
    mouse(document, 'mouseup', r.left + r.width / 2, r.top + r.height / 2);
  };
  const settled = (side) => until(() => !$('.image-ghost') && box()?.classList.contains('free') && rowOf(box()).classList.contains(side), 3000);

  // The corner makes it wider.
  check('A picture has a corner to change its size with', $('.image-size', box()) && getComputedStyle($('.image-size', box())).cursor === 'nwse-resize');
  let letGo = pull($('.image-size', box()), 199, 0);
  check('The corner makes the picture wider while it is pulled', box().classList.contains('sizing') && Math.round(at(box()).width) === 200, `${at(box()).width}`);
  letGo();
  check('…and the width is kept', await until(() => box().dataset.place === 'false,line,0,200' && !box().classList.contains('sizing'), 3000), box().dataset.place);
  check('A picture keeps its shape at its new width', Math.round(at(pic()).width) === 200 && Math.round(at(pic()).height) === 200, `${at(pic()).width} x ${at(pic()).height}`);

  // A click shows it large.
  press(pic());
  let shade = await until(() => $('.lightbox.open'));
  await wait(320);
  let big = shade && $('img', shade);
  check('A click on a picture shows it large, over the whole window', shade && big && big.src === pic().src && getComputedStyle(shade).position === 'fixed' && Math.round(at(shade).width) === innerWidth && Math.round(at(shade).height) === innerHeight, shade ? `${at(shade).width} x ${at(shade).height} of ${innerWidth} x ${innerHeight}` : 'none');
  check('…in the middle of the window, on top of all else', big && Math.abs((at(big).left + at(big).right) / 2 - innerWidth / 2) < 1.5 && Math.abs((at(big).top + at(big).bottom) / 2 - innerHeight / 2) < 1.5 && onTop(big), big ? `${at(big).left} ${at(big).top}` : 'none');
  check('A click is not a drag: the picture stays in its line', !$('.image-ghost') && !box().classList.contains('free') && !box().classList.contains('lifting'));
  key(document.body, 'Escape');
  check('Escape puts the large picture away', await until(() => !$('.lightbox'), 1500));
  press(pic());
  shade = await until(() => $('.lightbox.open'));
  // The second press of a double click does not put it away again.
  mouse(shade, 'mousedown', 6, 6);
  await wait(120);
  check('A second press right after it opened (a double click) leaves it open', $('.lightbox'));
  await wait(400);
  mouse(shade, 'mousedown', 6, 6);
  check('…and a press anywhere after that puts it away', shade && (await until(() => !$('.lightbox'), 1500)));

  // A small slip of the hand is not a move.
  letGo = pull(pic(), 10, 6);
  letGo();
  await wait(250);
  check('A small slip of the hand moves nothing and opens nothing', !box().classList.contains('free') && !$('.image-ghost') && !$('.lightbox') && box().dataset.place === 'false,line,0,200', box().dataset.place);

  // A drag takes it off its line, to a side of the page.
  const before = at(pic());
  letGo = pull(pic(), 300, 120);
  const ghost = $('.image-ghost');
  check('A dragged picture comes off its line and follows the pointer', ghost && box().classList.contains('lifting') && Math.abs(at(ghost).left - (before.left + 300)) < 1.5 && Math.abs(at(ghost).top - (before.top + 120)) < 1.5, ghost ? `${at(ghost).left - before.left}, ${at(ghost).top - before.top}` : 'no copy');
  letGo();
  await settled('right');
  await wait(80);
  const want = Math.round(col().right - (before.left + 300 + 200));
  check('Let go at the right, it goes to the right side of the page, as far in as it was put', box().classList.contains('free') && rowOf(box()).classList.contains('right') && box().dataset.place === `true,right,${want},200` && Math.abs(at(pic()).left - (before.left + 300)) <= 2, `${box().dataset.place} (want ${want}); ${at(pic()).left - before.left}`);
  check('It is part of the page: the page ends below it, and no line is under or over it', at($('.blocks', pane())).bottom >= at(pic()).bottom - 1 && clear() && getComputedStyle(rowOf(box())).position !== 'absolute' && getComputedStyle(box()).position !== 'absolute');
  check('It keeps its width', Math.round(at(pic()).width) === 200, `${at(pic()).width}`);

  // Let go below the text, it is after the last line: a click below the text
  // goes to the empty last line and adds no new one.
  const lines = () => $$('.blk', pane()).length;
  const n = lines();
  $('.page-tail', pane()).click();
  await wait(200);
  $('.page-tail', pane()).click();
  await wait(200);
  check('A click below the text adds no line after a free picture', lines() === n && document.activeElement === $$('.blk', pane()).pop(), `${lines()} of ${n}`);

  // Level with a line: that line and the next run beside it.
  letGo = pull(pic(), 0, at(inner('Two')).top + 8 - at(pic()).top);
  letGo();
  await settled('right');
  check('Let go level with a line, the picture is level with it and the line runs beside it', await until(() => Math.abs(at(pic()).top - at(inner('Two')).top) <= 8 && at(inner('Two')).right <= at(pic()).left && at(inner('Two')).width >= 219, 3000), `${at(pic()).top - at(inner('Two')).top}; ${at(inner('Two')).right - at(pic()).left}; ${at(inner('Two')).width}`);
  check('The line before it keeps the whole width', Math.abs(at(inner('One')).width - col().width) <= 1, `${at(inner('One')).width} of ${col().width}`);
  check('No line is under or over the picture', clear());

  // It stays with the text it was put at.
  const gap = () => at(pic()).top - at(inner('Two')).top;
  const [gap0, top0] = [gap(), at(pic()).top];
  const one = inner('One').querySelector('.blk');
  one.focus();
  T().setSel(one, 3);
  await nextLine(one);
  check('A new line above moves the picture down with its text', await until(() => at(pic()).top - top0 > 10 && Math.abs(gap() - gap0) <= 1, 3000), `${at(pic()).top - top0}; ${gap() - gap0}`);

  // A wide thing has no room beside it: it goes below.
  const last = $$('.blk', pane()).pop();
  last.focus();
  typeIn(last, '/table');
  (await until(() => $$('.slash-menu .menu-item').find((i) => i.textContent.includes('Table')))).click();
  const table = await until(() => $('.table-block', pane()));
  check('A table has no room beside the picture, so it goes below it', table && (await until(() => at(table).top >= at(pic()).bottom - 1, 3000)) && clear(), table ? `${at(table).top - at(pic()).bottom}` : 'no table');

  // To the left side.
  const from = at(pic());
  letGo = pull(pic(), Math.round(col().left + 20 - from.left), 0);
  letGo();
  await settled('left');
  check('Dragged to the left, it goes to the left side and the line runs at its right', await until(() => box().dataset.place === 'true,left,20,200' && Math.abs(at(pic()).left - (col().left + 20)) <= 2 && at(inner('Two')).left >= at(pic()).right, 3000), `${box().dataset.place}; ${at(pic()).left - col().left}; ${at(inner('Two')).left - at(pic()).right}`);
  check('Still no line is under or over it', clear());
  press(pic());
  check('A click on a free picture shows it large too', await until(() => $('.lightbox.open')));
  key(document.body, 'Escape');
  await until(() => !$('.lightbox'), 1500);
  await wait(120);
  snap('picture-free');

  // Its two small buttons.
  check('A free picture has three small buttons, each with a name', $$('.image-tool', box()).map((b) => b.title).join(' | ') === 'Put back in line | Add a caption | Delete', $$('.image-tool', box()).map((b) => b.title).join(' | '));
  $('[data-image-op="in-line"]', box()).click();
  check('Put back in line: it is a line of the page again, at its width', await until(() => !box().classList.contains('free') && box().dataset.place === 'false,line,0,200' && at(rowOf(box())).height > 190 && Math.abs(at(rowOf(box())).width - col().width) <= 1, 3000), `${box().dataset.place}; ${at(rowOf(box())).height}; row ${at(rowOf(box())).width} of ${col().width}; style ${rowOf(box()).getAttribute('style')}; class ${rowOf(box()).className}`);
  check('A picture in line has the caption and delete buttons, and no "in line" one', $$('.image-tool', box()).map((b) => b.dataset.imageOp).join('|') === 'caption|delete', $$('.image-tool', box()).map((b) => b.dataset.imageOp).join('|'));
  // A size change given up with Escape leaves the picture as it was.
  letGo = pull($('.image-size', box()), 80, 0);
  key(document.body, 'Escape');
  letGo();
  await wait(150);
  check('Escape gives up a size change', Math.round(at(box()).width) === 200 && box().dataset.place.endsWith(',200') && !box().classList.contains('sizing'), `${at(box()).width} ${box().dataset.place}`);
  // The corner of a free picture changes its size too.
  letGo = pull(pic(), 320, 0);
  letGo();
  await settled('right');
  letGo = pull($('.image-size', box()), 60, 0);
  letGo();
  check('The corner of a free picture makes it wider too', await until(() => box().dataset.place.endsWith(',260') && Math.round(at(pic()).width) === 260, 3000), `${box().dataset.place}; ${at(pic()).width}; row ${at(rowOf(box())).width}; row style ${rowOf(box()).getAttribute('style')}; box style ${box().getAttribute('style')}; col ${col().width}`);
  $('[data-image-op="delete"]', box()).click();
  check('Delete takes the picture away', await until(() => !box(), 3000));
  T().command('undo');
  check('…and Undo brings it back where it was', await until(() => box()?.classList.contains('free'), 3000));
  // Delete while a picture is shown large takes it out of the page.
  press(pic());
  await until(() => $('.lightbox.open'));
  key(document.body, 'Delete');
  check('Delete takes out the picture that is shown large, and puts the large view away', await until(() => !box() && !$('.lightbox'), 3000));
  T().command('undo');
  await until(() => box(), 3000);
});

await run('Microphones beside text boxes', async () => {
  await oneTab();
  const mic = await until(() => $('[data-dictate="jot-input"]'));
  mic.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
  mic.click();
  check('The quick note box has a microphone that puts the caret in it', document.activeElement?.id === 'jot-input' && document.body.dataset.dictating === 'jot-input', document.activeElement?.id);
});

await run('Ask your notes', async () => {
  await oneTab();
  askAi();
  const ask = await until(() => $('.modal.ask-notes #ask-notes-input'));
  check('With no page open, Ask AI asks all your notes', ask);
  check('The question can be spoken', $('[data-dictate="ask-notes-input"]'));
  {
    // Glass: a clear tint with a blur of its own, one hairline, and no blue button.
    const modal = $('.modal.ask-notes');
    const cs = getComputedStyle(modal);
    const alpha = Number((cs.backgroundColor.match(/[\d.]+/g) || [])[3] ?? 1);
    const blur = cs.backdropFilter || cs.webkitBackdropFilter || '';
    check('The box is translucent glass: a clear tint, a blur of its own, one hairline', alpha > 0.3 && alpha < 0.75 && /blur\(/.test(blur) && parseFloat(cs.borderTopWidth) === 1, `${cs.backgroundColor} ${blur}`);
    const back = getComputedStyle(modal.parentElement, '::before');
    check('The window behind it stays dimmed and a little blurred', /blur\(4px\)/.test(back.backdropFilter || back.webkitBackdropFilter || '') && back.backgroundColor !== 'rgba(0, 0, 0, 0)', `${back.backgroundColor} ${back.backdropFilter}`);
    const go = $('#ask-notes-go');
    const gs = getComputedStyle(go);
    check('Its send button is a round in the ink of the text, with no word and no blue, there once there are words', go.disabled && gs.display === 'none' && gs.borderRadius === '50%' && go.textContent.trim() === '' && gs.backgroundColor === getComputedStyle(ask).color, `${gs.display} ${gs.backgroundColor} vs ${getComputedStyle(ask).color}`);
    check('The box has no icon before its words, and its foot names the keys', !$('.ask-icon', modal) && $('.ask-row', modal).firstElementChild === ask && $$('.ask-key', modal).map((k) => k.textContent).join(' ') === '↵ esc' && /Reads and changes your notes/.test($('.ask-foot .modal-foot-note', modal).textContent), $('.ask-foot', modal)?.textContent);
  }
  ask.value = 'What is on my reading list? [[reply: It lists three books.\\n\\nSources:\\n- Reading List]]';
  ask.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => !$('#ask-notes-go').disabled);
  $('#ask-notes-go').click();
  check('The AI answers from the notes', await until(() => $('.ask-answer')?.textContent.includes('It lists three books.'), 6000), $('.ask-answer')?.textContent);
  check('It names the page it used, as a link to it', await until(() => $$('.ask-source').some((b) => b.textContent === 'Reading List'), 4000), $$('.ask-source').map((b) => b.textContent).join(', '));
  check('It says which pages the AI read', $('.ask-foot .modal-foot-note')?.textContent.includes('Reading List'), $('.ask-foot .modal-foot-note')?.textContent);
  await wait(150);
  snap('ask-notes');
  $('#ask-notes-save').click();
  check('The answer can be kept as a page', await until(() => !$('.modal.ask-notes') && $$('.sidebar .tree-item .tree-title').some((t) => t.textContent.startsWith('What is on my reading list'))));
  // From Home the AI can do things too, with the tools of Rune.
  await oneTab();
  askAi();
  const tell = await until(() => $('.modal.ask-notes #ask-notes-input'));
  tell.value = 'make a page for my trip [[tool: create_page {"title":"From Home"}]] [[reply: I made the page From Home.]]';
  tell.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => !$('#ask-notes-go').disabled);
  $('#ask-notes-go').click();
  const did = await until(() => $('#ask-did'), 8000);
  check('From Home, the AI does what it is told and says what it did', did && $$('#ask-did .ai-plan-row').map((r) => r.textContent.trim()).join('|') === 'Made the page “From Home”' && $('.ask-answer')?.textContent.includes('I made the page From Home'), did?.textContent);
  check('The new page is in the sidebar', await until(() => treeItem('From Home'), 5000));
  await wait(150);
  snap('ask-did');
  $('#ask-undo').click();
  check('Undo in the box takes it back', await until(() => !treeItem('From Home') && !$('#ask-did'), 6000));
  key(document.body, 'Escape');
  $('.modal-backdrop')?.click();
  await until(() => !$('#ask-notes-input'));
});

await run('Voice to calendar and to-dos', async () => {
  await oneTab();
  const d = new Date(Date.now() + 86400000);
  const day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const pagesNow = $$('.sidebar .tree-item').length;
  speakPage();
  await say(`lunch with Sam tomorrow at one, every week [[reply: @event {"title": "Lunch with Sam", "start": "${day}T13:00", "repeat": "weekly"}]]`);
  check('Asked for the calendar, it makes the event and shows it', await until(() => $('.cal-editor .ed-title')?.value === 'Lunch with Sam', 6000), `${$('.cal-editor .ed-title')?.value} | ${$('.toast')?.textContent}`);
  check('The event is at the time that was said, and repeats', $('#ev-start')?.dataset.value === '13:00' && $('#ev-repeat')?.dataset.value === 'week', `${$('#ev-start')?.dataset.value} | ${$('#ev-repeat')?.dataset.value}`);
  check('It says what went on the calendar', await until(() => ($('.toast')?.textContent || '').startsWith('On your calendar: Lunch with Sam'), 2000), $('.toast')?.textContent);
  check('No page is made for it', $$('.sidebar .tree-item').length === pagesNow, `${pagesNow} -> ${$$('.sidebar .tree-item').length}`);
  check('The event notes have a microphone', $('[data-dictate="ev-notes"]'));
  speakPage();
  await say('call the bank and book the car [[reply: @todo Call the bank\\n@todo Book the car]]');
  const todos = await until(() => shownPane('To-dos'), 6000);
  const tasks = () => $$('.kind-todo .blk', todos || document.body).map((b) => b.textContent);
  check('Bare tasks go on a To-dos page', await until(() => tasks().includes('Call the bank') && tasks().includes('Book the car'), 4000), tasks().join(' | '));
});

await run('Speak to edit', async () => {
  await oneTab();
  const title = await newPage('Edit by voice');
  key(title, 'Enter');
  const line = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(line, 'Please send the very long report today');
  await until(() => line.textContent.endsWith('today'));
  await wait(150);
  T().setSel(line, 16, 25);
  const speak = await until(() => $('#fmt-bar.show [data-fmt="speak"]'));
  check('Selected text offers a microphone on the format bar', speak, getSelection().toString());
  if (!speak) return;
  speak.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
  check('It asks what to do with the text', await until(() => $('.modal.voice h2')?.textContent === 'Say what to do with the text'), $('.modal.voice h2')?.textContent);
  await say('shorter [[reply: short]]');
  const text = () => $$('.blk', shownPane('Edit by voice') || document.body).map((b) => b.textContent).join(' | ');
  check('The AI changes only the selected words', await until(() => text() === 'Please send the short report today', 6000), text());

  // Read aloud, from the same page's menu.
  const menu = async (item) => {
    // Let the menu close (and the page bar redraw) before opening it again.
    await until(() => !$('.page-menu'));
    await wait(120);
    shownPane('Edit by voice').querySelector('.pagebar .icon-btn[title="Page options"]').click();
    return until(() => $('.page-menu') && menuItem($('.page-menu'), item));
  };
  const items = async () => {
    await until(() => !$('.page-menu'));
    await wait(120);
    shownPane('Edit by voice').querySelector('.pagebar .icon-btn[title="Page options"]').click();
    const m = await until(() => $('.page-menu'));
    return $$('.menu-item', m).map((i) => i.querySelector(':scope > span:not(.menu-icon):not(.menu-shortcut)')?.textContent);
  };
  check('The page menu holds only what belongs to this page', (await items()).join(', ') === 'Copy link, Copy for Aven, Move to…, Page history, Move to Trash', (await (async () => $$('.page-menu .menu-item').map((i) => i.textContent).join(', '))()));
  $('#send-to-aven').click();
  check('The page can be given to a chat in Aven: the note names the live page', await until(() => /^From my Rune note “Edit by voice” \(page id [^;]+; the notion2 tools can read and change the live page\):$/.test(document.body.dataset.sentToAven || '')), document.body.dataset.sentToAven);
  check('It says what to do next, and the menu closes', await until(() => $('.toast')?.textContent === 'Copied. Paste it in the Aven chat (⌘V)') && !$('.page-menu'), $('.toast')?.textContent);
  key(document.body, 'Escape');
  T().command('read_aloud');
  const stop = await menu('Stop reading');
  check('While a page is read aloud, its menu offers to stop', stop);
  stop?.click();
  check('Stop ends the reading', await until(async () => !(await items()).includes('Stop reading'), 4000));
  key(document.body, 'Escape');
  await until(() => !$('.page-menu'));
});

await run('Right-click shows only what applies', async () => {
  await oneTab();
  // A right-click; true if the system's own menu was kept from opening.
  const rightClick = (el) => {
    const r = el.getBoundingClientRect();
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 12, clientY: r.top + 8 });
    el.dispatchEvent(ev);
    return ev.defaultPrevented;
  };
  const shownItems = () => $$('#ctx-menu .menu-item').filter((i) => i.offsetParent).map((i) => i.querySelector('span').textContent).join(', ');
  check('On a page in the sidebar: that page’s menu', rightClick(treeItem('Reading List')) && (await until(() => $('.sidebar-menu'))));
  key(document.body, 'Escape');
  await until(() => !$('.sidebar-menu'));
  check('On empty space: nothing at all', rightClick($('.home-hero')) && !$('#ctx-menu.show') && !$('.popover'));
  $$('.tab')[0].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  const tabMenu = await until(() => $('.tab-menu'));
  check('On the only tab: no "Close Other Tabs"', tabMenu && !menuItem(tabMenu, 'Close Other Tabs') && menuItem(tabMenu, 'Close Tab'), tabMenu?.textContent);
  key(document.body, 'Escape');
  await until(() => !$('.tab-menu'));
  treeItem('Typing test').click();
  const pane = await until(() => shownPane('Typing test'));
  const line = pane.querySelector('.blk');
  line.focus();
  T().setSel(line, 0);
  check('On a line with nothing selected: the line’s own menu', rightClick(line) && (await until(() => $('.block-menu'))) && !$('#ctx-menu.show'));
  key(document.body, 'Escape');
  await until(() => !$('.block-menu'));
  line.focus();
  T().setSel(line, 0, 5);
  await wait(100);
  check('On selected text: Cut, Copy and Paste, and no more', rightClick(line) && (await until(() => $('#ctx-menu.show'))) && shownItems() === 'Cut, Copy, Paste', shownItems());
  key(document.body, 'Escape');
  check('Escape closes it', await until(() => !$('#ctx-menu.show')));
  const titleBox = pane.querySelector('.title-input');
  titleBox.focus();
  titleBox.setSelectionRange(0, 0);
  check('In a text box with nothing selected: Paste only', rightClick(titleBox) && (await until(() => $('#ctx-menu.show'))) && shownItems() === 'Paste', shownItems());
  key(document.body, 'Escape');
  await until(() => !$('#ctx-menu.show'));
});

await run('Meeting notes in a page you choose', async () => {
  await oneTab();
  await openSettings();
  await settingsSection('Notifications');
  check('Settings does not ask for one page for all meeting notes', $('#meeting-notes-switch') && !$('#meetings-home'));
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
  const datedName = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  T().command('meeting_notes');
  const made = await until(() => $$('.pane').find((p) => datedName.test(p.querySelector('.title-input')?.value || '') && p.offsetParent), 4000);
  const crumbs = () => (made ? $$('.pagebar .crumb', made).map((c) => c.textContent.trim()) : []);
  check('A new meeting’s page starts in "Meeting notes"', made && crumbs().includes('Meeting notes'), crumbs().join(' / '));
  check('While the meeting goes on, its page does not ask where to keep it', made && !$('#meeting-place'));

  // "Move to…" in the menu of the page asks at once.
  const ask = async (pane) => {
    $('.pagebar .icon-btn[title="Page options"]', pane).click();
    const menu = await until(() => $('.page-menu'));
    $('#move-page', menu).click();
    return until(() => !$('.page-menu') && $('#meeting-place', pane), 3000);
  };
  const place = made && (await ask(made));
  const names = place ? await placeNames(made) : [];
  check('"Move to…" in the menu of the page asks which page to keep it in', place && $('.place-bar .listen-what', made).textContent === 'Keep these notes in' && placeNow(made) === 'Meeting notes' && names.includes('Reading List') && !names.some((n) => datedName.test(n)), names.join(' | '));
  if (!place) return;
  await choosePlace(made, 'Reading List');
  check('A choice moves the page there at once', await until(() => crumbs().includes('Reading List') && placeNow() === 'Reading List', 3000), crumbs().join(' / '));
  await wait(120);
  snap('move-to');
  $('#meeting-place-done').click();
  check('"Done" puts the question away', await until(() => !$('#meeting-place'), 2000));
  treeItem('Reading List').click();
  const list = await until(() => shownPane('Reading List'));
  const rows = () => $$('.block-row', list).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-'))}:${(r.querySelector('.blk') || r).textContent.trim()}`);
  const linked = () => {
    const r = rows();
    const h = r.indexOf('kind-heading2:Meetings');
    return h >= 0 && (r[h + 1] || '').startsWith('kind-page:') && datedName.test(r[h + 1]);
  };
  check('That page lists it under a "Meetings" heading', await until(linked, 3000), rows().slice(-4).join(' | '));

  // A page that is not notes of a meeting: the same question, and a plain move.
  await newPage('Loose page');
  const loose = await until(() => shownPane('Loose page'));
  const where = loose && (await ask(loose));
  // (With several spaces the top of the list has the name of the space.)
  const topOfList = () => $$('.side-foot .space-dot').length > 1 ? $('.ws-label').textContent : 'No page (top of the list)';
  check('On any other page it asks the same, with the top of the list chosen for a page at the top', where && $('.place-bar .listen-what', loose).textContent === 'Keep this page in' && placeNow(loose) === topOfList(), placeNow(loose));
  if (!where) return;
  await choosePlace(loose, 'Getting Started');
  const looseCrumbs = () => $$('.pagebar .crumb', loose).map((c) => c.textContent.trim());
  check('…and a choice puts the page inside that page', await until(() => looseCrumbs().includes('Getting Started'), 3000), looseCrumbs().join(' / '));
  await choosePlace(loose, topOfList());
  check('"No page" puts it back at the top of the list', await until(() => !looseCrumbs().includes('Getting Started') && treeItem('Loose page'), 3000), looseCrumbs().join(' / '));
  $('#meeting-place-done').click();
  await until(() => !$('#meeting-place'));
  T().command('forget_meeting_slots');
});

await run('A meeting with no event is known by its time', async () => {
  await oneTab();
  T().command('forget_meeting_slots');
  await wait(60);
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  const dated = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  const named = (pane) => pane.querySelector('.title-input')?.value || '';
  const crumbsOf = (pane) => $$('.pagebar .crumb', pane).map((c) => c.textContent.trim());
  const seen = new Set();
  // The notes of a meeting that starts now, with no event on the calendar.
  const meet = async () => {
    T().command('meeting_notes');
    const pane = await until(() => $$('.pane').find((p) => dated.test(named(p)) && p.offsetParent && !seen.has(p.dataset.pane)), 4000);
    if (pane) seen.add(pane.dataset.pane);
    return pane;
  };
  const ask = async (pane) => {
    $('.pagebar .icon-btn[title="Page options"]', pane).click();
    $('#move-page', await until(() => $('.page-menu'))).click();
    return until(() => !$('.page-menu') && $('#meeting-place', pane), 3000);
  };

  // The first one: the person says where its notes are kept.
  const first = await meet();
  check('The notes of a meeting with no event start in "Meeting notes"', first && named(first).startsWith('Meeting – ') && crumbsOf(first).includes('Meeting notes'), first && crumbsOf(first).join(' / '));
  if (!first) return;
  const place = await ask(first);
  check('The first time, there is no page to propose', place && !$('.place-same', first));
  await choosePlace(first, 'Reading List');
  await until(() => crumbsOf(first).includes('Reading List'));
  // The person gives these notes a name.
  const title = first.querySelector('.title-input');
  title.focus();
  title.value = named(first).replace(/^Meeting/, 'Atlas sync');
  title.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(150);
  $('#meeting-place-done', first).click();
  await until(() => !$('#meeting-place'));

  // The next one at this time: the name is kept, and the page of the last time is one click away.
  const second = await meet();
  check('The next meeting at that time takes the name the person gave, and still starts in "Meeting notes"', second && named(second).startsWith('Atlas sync – ') && crumbsOf(second).includes('Meeting notes'), second && `${named(second)} in ${crumbsOf(second).join(' / ')}`);
  if (!second) return;
  await ask(second);
  const same = await until(() => $('.place-same', second), 3000);
  check('Its question proposes the page of the last time', same && same.textContent.trim() === 'Reading List' && $('.place-before', second), same?.textContent);
  await wait(150);
  snap('meeting-slot');
  same?.click();
  check('One click keeps the notes there, and the page says that the next ones go there by themselves', await until(() => crumbsOf(second).includes('Reading List') && !$('.place-same', second) && $('#meeting-place-kept', second)?.textContent.includes('by themselves'), 3000), $('#meeting-place-kept', second)?.textContent);
  $('#meeting-place-done', second).click();
  await until(() => !$('#meeting-place'));

  // The third: by itself.
  const third = await meet();
  check('After two choices, the notes of the next meeting at that time start in that page', third && named(third).startsWith('Atlas sync – ') && crumbsOf(third).includes('Reading List') && !crumbsOf(third).includes('Meeting notes') && !$('#meeting-place', third), third && crumbsOf(third).join(' / '));
  T().command('forget_meeting_slots');
});

await run('A meeting remembers its page', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  // An event that is on now: from the start of this hour, for an hour.
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar'));
  const hour = new Date().getHours();
  const label = `${hour % 12 || 12}${hour < 12 ? 'am' : 'pm'}`;
  $('.cal-toolbar .primary-btn').click();
  const box = await until(() => document.activeElement?.id === 'cal-quick' && document.activeElement, 3000);
  box.value = `Apollo standup today ${label}`;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  key(box, 'Enter');
  await until(() => !$('#cal-quick'));
  await wait(200);

  const named = (pane) => pane.querySelector('.title-input')?.value || '';
  const meetings = () => $$('.pane').filter((p) => named(p).startsWith('Apollo standup – ') && p.offsetParent);
  const crumbsOf = (pane) => $$('.pagebar .crumb', pane).map((c) => c.textContent.trim());
  T().command('meeting_notes');
  const first = await until(() => meetings()[0], 4000);
  check('The notes of a meeting take the name of the event that is on, and start in "Meeting notes"', first && crumbsOf(first).includes('Meeting notes'), first && crumbsOf(first).join(' / '));
  if (!first) return;

  // The person says where these notes are kept.
  $('.pagebar .icon-btn[title="Page options"]', first).click();
  $('#move-page', await until(() => $('.page-menu'))).click();
  const place = await until(() => $('#meeting-place', first), 3000);
  check('Before a choice, the page does not say where the next notes go', place && $('#meeting-place-kept', first).textContent === '', $('#meeting-place-kept', first)?.textContent);
  await choosePlace(first, 'Reading List');
  check('After a choice, the page says that the next notes of this meeting go there too', await until(() => crumbsOf(first).includes('Reading List') && $('#meeting-place-kept', first)?.textContent === 'The next “Apollo standup” notes go here too.', 3000), $('#meeting-place-kept', first)?.textContent);
  await wait(150);
  snap('meeting-kept');
  $('#meeting-place-done', first).click();
  await until(() => !$('#meeting-place'));

  // The next meeting of the same name: its notes start in that page.
  const firstId = first.dataset.pane;
  T().command('meeting_notes');
  const second = await until(() => meetings().find((p) => p.dataset.pane !== firstId) || ($$('.tab').length > 2 && meetings()[0]), 4000);
  const tabs = $$('.tab').filter((t) => t.textContent.includes('Apollo standup – ')).length;
  check('The notes of the next meeting of that name start in the same page, with no question', second && tabs === 2 && crumbsOf(second).includes('Reading List') && !crumbsOf(second).includes('Meeting notes'), second && `${tabs} tabs, ${crumbsOf(second).join(' / ')}`);
  treeItem('Reading List')?.click();
  const list = await until(() => shownPane('Reading List'));
  const links = () => $$('.block-row.kind-page', list).filter((r) => r.textContent.includes('Apollo standup – ')).length;
  check('That page lists the two under "Meetings"', await until(() => links() === 2 && $$('.block-row.kind-heading2', list).some((r) => r.textContent.trim() === 'Meetings'), 3000), String(links()));
  // The event goes away again, so that the checks after this one start clean.
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar'));
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
  $('.cal-nav .secondary-btn').click();
  const made = await until(() => $$('.cal-event[data-event]').find((e) => e.querySelector('.ev-title')?.textContent === 'Apollo standup'), 3000);
  made?.click();
  // (The panel of another event may be open from an earlier group: the bin
  // to press is the one of this event's panel.)
  const del = await until(() => $('.cal-editor .ed-title')?.value === 'Apollo standup' && $('#ev-delete'), 3000);
  del?.click();
  await until(() => !$$('.cal-event[data-event]').some((e) => e.querySelector('.ev-title')?.textContent === 'Apollo standup'), 3000);
  T().command('forget_meeting_slots');
});

await run('Notes kept in another space', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  const startSpace = $('.ws-label').textContent;
  // A space of its own, with a page in a page.
  $('.ws-name').click();
  const menu = await until(() => $('.space-menu'));
  menuItem(menu, 'New space').click();
  const input = await until(() => $('.space-new input'));
  input.value = 'Far space';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(50);
  key(input, 'Enter');
  await until(() => $('.ws-label')?.textContent === 'Far space');
  await newPage('Far home');
  await until(() => shownPane('Far home'));
  await newPage('Far inner');
  const farInner = await until(() => shownPane('Far inner'));
  $('.pagebar .icon-btn[title="Page options"]', farInner).click();
  $('#move-page', await until(() => $('.page-menu'))).click();
  await until(() => $('#meeting-place', farInner), 3000);
  await choosePlace(farInner, 'Far home');
  $('#meeting-place-done', farInner).click();
  await until(() => !$('#meeting-place', farInner));
  // Back to the first space: a meeting there.
  $('.ws-name').click();
  menuItem(await until(() => $('.space-menu')), startSpace).click();
  await until(() => $('.ws-label')?.textContent === startSpace);
  const datedName = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  const before = new Set($$('.pane').map((p) => p.dataset.pane));
  T().command('meeting_notes');
  const made = await until(() => $$('.pane').find((p) => datedName.test(p.querySelector('.title-input')?.value || '') && p.offsetParent && !before.has(p.dataset.pane)), 4000);
  const crumbs = () => $$('.pagebar .crumb', made).map((c) => c.textContent.trim());
  check('The notes of a meeting start in the space on the screen', made && crumbs().includes('Meeting notes'), made && crumbs().join(' / '));
  if (!made) return;
  $('.pagebar .icon-btn[title="Page options"]', made).click();
  $('#move-page', await until(() => $('.page-menu'))).click();
  const field = await until(() => $('#meeting-place', made), 3000);
  check('The bar has no system list', field && field.tagName === 'BUTTON' && !$('.place-bar select', made), field?.tagName);
  check('Closed, the field says where the notes are, and in which space', placeNow(made) === 'Meeting notes' && $('#meeting-place .place-aside', made)?.textContent === startSpace, `${placeNow(made)} | ${$('#meeting-place .place-aside', made)?.textContent}`);
  const list = await placesOpen(made);
  await wait(200);
  const rows = () => $$('.place-row', list);
  const names = () => rows().map(placeRowName);
  const spaces = $$('.side-foot .space-dot').map((d) => d.title);
  const at = (name) => names().indexOf(name);
  check('The menu is Rune’s own, under the field', list?.getAttribute('role') === 'menu' && getComputedStyle(list).position === 'fixed' && Math.abs(list.getBoundingClientRect().left - field.getBoundingClientRect().left) <= 1 && list.getBoundingClientRect().top >= field.getBoundingClientRect().bottom, `${getComputedStyle(list).position}`);
  check('Every space is in the menu, each with a badge', spaces.length >= 2 && spaces.every((s) => names().includes(s)) && $$('.place-row.place-space .space-badge', list).length === spaces.length, names().join(' | '));
  check('The pages are under their own space, and as deep as they go', at(startSpace) < at('Getting Started') && at('Far space') < at('Far home') && at('Far home') < at('Far inner') && parseInt(rows()[at('Far inner')].style.paddingLeft) > parseInt(rows()[at('Far home')].style.paddingLeft), names().join(' | '));
  check('The place the notes are in is the chosen row, and has the keyboard', $('.place-row.on', list) && placeRowName($('.place-row.on', list)) === 'Meeting notes' && document.activeElement === $('.place-row.on', list), document.activeElement?.className);
  snap('place-menu');
  key(document.activeElement, 'Escape');
  check('Escape shuts the menu', await until(() => !$('.place-menu', made)));
  await choosePlace(made, 'Far inner');
  const label = () => $('.ws-label')?.textContent;
  check('A page of another space: the notes move there, and the sidebar goes with them', await until(() => crumbs().includes('Far inner') && crumbs().includes('Far home') && !crumbs().includes('Meeting notes') && placeNow(made) === 'Far inner' && label() === 'Far space', 3000), `${crumbs().join(' / ')} | ${label()}`);
  check('The field names the new space', await until(() => $('#meeting-place .place-aside', made)?.textContent === 'Far space'), $('#meeting-place .place-aside', made)?.textContent);
  const reopened = await placesOpen(made);
  check('Opened again, that is the chosen row', reopened && placeRowName($('.place-row.on', reopened)) === 'Far inner', $('.place-row.on', reopened)?.textContent);
  await placesShut(made);
  const title = made.querySelector('.title-input').value;
  check('In the sidebar the notes are in that space', await until(() => treeItem(title), 3000), title);
  // To the top of a space's list.
  await choosePlace(made, startSpace);
  check('The row of a space: the notes go to the top of its list, and the sidebar goes with them', await until(() => !crumbs().includes('Far inner') && placeNow(made) === startSpace && label() === startSpace && treeItem(title), 3000), `${crumbs().join(' / ')} | ${placeNow(made)} | ${label()}`);
  // And back: the place stays for the next meeting at this time.
  await choosePlace(made, 'Far inner');
  await until(() => label() === 'Far space');
  $('#meeting-place-done', made).click();
  check('"Done" puts the question away', await until(() => !$('#meeting-place', made), 2000));
  const backHome = async () => {
    await oneTab();
    $('.ws-name').click();
    menuItem(await until(() => $('.space-menu')), startSpace).click();
    await until(() => $('.ws-label')?.textContent === startSpace);
    await until(() => !$('.side-ghost'), 1500);
  };
  await backHome();
  T().command('meeting_notes');
  const next = await until(() => $$('.pane').find((p) => datedName.test(p.querySelector('.title-input')?.value || '') && p.offsetParent), 4000);
  check('The next meeting at this time starts in the space on the screen', next && $$('.pagebar .crumb', next).some((c) => c.textContent.trim() === 'Meeting notes'), next && $('.crumbs', next)?.textContent);
  if (next) {
    $('.pagebar .icon-btn[title="Page options"]', next).click();
    $('#move-page', await until(() => $('.page-menu'))).click();
    const same = await until(() => $('.place-same', next), 3000);
    check('Its question proposes the place of the last time, in the other space', same?.textContent.trim() === 'Far inner', same?.textContent);
    same?.click();
    check('One click keeps the notes there', await until(() => placeNow(next) === 'Far inner' && label() === 'Far space', 3000), `${placeNow(next)} | ${label()}`);
    await choosePlace(next, 'Meeting notes');
    await until(() => label() === startSpace && placeNow(next) === 'Meeting notes', 3000);
    $('#meeting-place-done', next)?.click();
    await until(() => !$('#meeting-place', next));
  }
  await backHome();
  T().command('forget_meeting_slots');
});

await run('Meeting recap by voice', async () => {
  const dated = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  const row = $$('.sidebar .tree-item').find((r) => {
    const t = r.querySelector('.tree-title')?.textContent || '';
    return dated.test(t) && !t.startsWith('Voice note') && !r.closest('.side-ghost');
  });
  check('The meeting page from before is in the sidebar', row);
  if (!row) return;
  row.click();
  const page = await until(() => $$('.pane').find((p) => dated.test(p.querySelector('.title-input')?.value || '') && p.offsetParent));
  check('Going straight from a page with a typed title to another shows that page’s own title', page, $$('.pane').map((p) => `${JSON.stringify(p.querySelector('.title-input')?.value)}/${p.offsetParent ? 'shown' : 'hidden'}/${p.querySelector('.crumb.current')?.textContent}`).join(' ; '));
  if (!page) return;
  page.querySelector('.pagebar .icon-btn[title="Page options"]').click();
  const item = await until(() => $('.page-menu') && menuItem($('.page-menu'), 'Write a recap'));
  check('A meeting page offers a recap', item);
  item.click();
  // The one AI box takes the recap: no second box.
  const recapBox = await until(() => $('#ai-input'));
  check('The recap is asked for in the AI box, not in a second box', recapBox && !$('.modal.voice') && /meeting/.test(recapBox.placeholder) && (await until(() => document.activeElement === recapBox)), recapBox?.placeholder);
  check('The AI box has its microphone for a spoken recap', $$('[data-dictate="ai-input"]').length === 1);
  recapBox.value = 'we agreed the budget and Sam sends the numbers [[reply: ## Notes\\n- Budget agreed\\n\\n## Action items\\n- [ ] Sam: send the numbers]]';
  recapBox.dispatchEvent(new Event('input', { bubbles: true }));
  const write = await until(() => $('#ai-send'));
  check('Its send button says what it does', write?.title === 'Write the recap', write?.title);
  key(recapBox, 'Enter');
  check('The box goes away when the recap is sent', await until(() => !$('.ai-panel')));
  const rows = () => $$('.block-row', page).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-'))}:${r.querySelector('.blk')?.textContent ?? ''}`);
  const filled = () => {
    const r = rows();
    const n = r.indexOf('kind-heading2:Notes');
    const a = r.indexOf('kind-heading2:Action items');
    return n >= 0 && a > n && r[n + 1] === 'kind-bulleted:Budget agreed' && r[a + 1] === 'kind-todo:Sam: send the numbers';
  };
  check('The recap goes under Notes and Action items', await until(filled, 6000), rows().join(' | '));
  await wait(200);
  snap('recap');
});

await run('Listen to a meeting', async () => {
  T().command('forget_meeting_slots');
  await oneTab();
  const datedName = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  T().command('meeting_notes');
  const page = await until(() => $$('.pane').find((p) => datedName.test(p.querySelector('.title-input')?.value || '') && p.offsetParent), 4000);
  const ask = await until(() => $('#listen-start'));
  check('A meeting page asks before it listens, and does not listen by itself', page && ask && !$('.listen-bar.on') && !$('#listening-row'), $('.listen-bar')?.textContent);
  if (!ask) return;
  ask.click();
  check('"Listen" starts it, and the page and the sidebar say so', await until(() => $('.listen-bar.on') && $('#listening-row')), $('.listen-bar')?.textContent);
  check('It shows the last words it heard', await until(() => ($('.listen-latest')?.textContent || '').includes('Friday'), 4000), $('.listen-latest')?.textContent);
  await wait(150);
  snap('listening');
  $('#listen-stop').click();
  const rows = () => $$('.block-row', page).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-'))}:${(r.querySelector('.blk') || r).textContent.trim()}`);
  check('Stop puts the transcript in the page, in a closed toggle', await until(() => rows().includes('kind-toggle:Transcript'), 8000), rows().join(' | '));
  const written = () => {
    const r = rows();
    const n = r.indexOf('kind-heading2:Notes');
    const a = r.indexOf('kind-heading2:Action items');
    return n >= 0 && a > n && r[n + 1] === 'kind-text:For the weekend.' && r[a + 1] === 'kind-todo:Buy milk';
  };
  check('Then the AI writes the notes and the action items from it', await until(written, 8000), rows().join(' | '));
  check('The listening lines go away when it is done', await until(() => !$('.listen-bar.on') && !$('#listening-row'), 3000));

  // Who spoke when: a band at the top of the page, a lane for each voice.
  const band = await until(() => $('.meeting-band', page), 3000);
  const lanes = () => $$('.band-row', page);
  const names = () => lanes().map((l) => l.dataset.voice);
  check('The page of the meeting shows who spoke when, a lane for each voice', band && lanes().length >= 1 && lanes().every((l) => $$('.band-part', l).length >= 1 && $('.band-track', l).getBoundingClientRect().width > 80), `${lanes().length} lanes: ${names().join(', ')}`);
  check('It says how long the meeting was, how many voices, how many words', band && /^\d+ min · \d+ voices? · [\d,]+ words$/.test($('.band-meta', band).textContent), band && $('.band-meta', band).textContent);
  const shares = lanes().map((l) => Number($('.band-share', l).textContent.replace('%', '')));
  check('The parts of the speaking add up', Math.abs(shares.reduce((a, b) => a + b, 0) - 100) <= 1, shares.join(' + '));
  const partsIn = lanes().every((l) => {
    const track = $('.band-track', l).getBoundingClientRect();
    return $$('.band-part', l).every((part) => part.getBoundingClientRect().left >= track.left - 1 && part.getBoundingClientRect().width >= 2);
  });
  check('Each stretch of speech is a mark on its lane', partsIn);
  await wait(120);
  snap('meeting-band');

  // A voice can be given a name: its lines in the transcript change with it.
  const was = names()[names().length - 1];
  $('.band-name', lanes()[lanes().length - 1]).click();
  const nameBox = await until(() => document.activeElement?.classList.contains('band-name-input') && document.activeElement, 3000);
  check('A click on a voice opens a field for its name, with the caret in it', nameBox && nameBox.value === was, nameBox?.value);
  if (nameBox) {
    nameBox.value = 'Sam';
    nameBox.dispatchEvent(new Event('input', { bubbles: true }));
    key(nameBox, 'Enter');
  }
  check('Return gives the voice its name', await until(() => names().includes('Sam') && !names().includes(was) && !$('.band-name-input'), 3000), names().join(', '));
  delete document.body.dataset.copied;
  T().command('copy_markdown');
  const text = await until(() => document.body.dataset.copied);
  check('…and its lines in the transcript have the name', text && text.includes('**Sam** · ') && !text.includes(`**${was}** · `), text?.slice(-200));
  T().command('undo');
  check('Undo takes the name back', await until(() => names().includes(was) && !names().includes('Sam'), 3000), names().join(', '));
  // The meeting is over: its page asks which page to keep it in.
  const place = await until(() => $('#meeting-place'), 4000);
  const places = place ? await placeNames(page) : [];
  check('When the meeting is over, its page asks which page to keep it in', place && placeNow(page) === 'Meeting notes' && places.includes('Getting Started') && !places.some((o) => datedName.test(o)), places.join(' | '));
  await wait(120);
  snap('meeting-place');
  if (place) await choosePlace(page, 'Getting Started');
  check('A choice moves the page there at once', await until(() => $('.crumbs', page)?.textContent.includes('Getting Started') && placeNow(page) === 'Getting Started', 3000), $('.crumbs', page)?.textContent);
  $('#meeting-place-done')?.click();
  check('"Done" puts the question away', await until(() => !$('#meeting-place'), 2000));
  // A page that is not a meeting's has none of this.
  treeItem('Typing test').click();
  await until(() => shownPane('Typing test'));
  check('Other pages show no listening line, and no band of voices', !$('.listen-bar') && !$('.meeting-band', shownPane('Typing test')));
});

// The groups of the pad stand on their own. Their meetings take no event of
// the calendar (they are known by their time), whatever an earlier group left
// on it at this hour. And what they switch on ends with them, also when a
// check throws, so that the groups after them start clean.
const padRun = (name, fn) => run(name, async () => {
  T().command('meetings_alone');
  try {
    await fn();
  } finally {
    T().command('meetings_alone_off');
    T().command('pad_inline_off');
    T().command('pad_window_off');
  }
});

await padRun('Meeting pad', async () => {
  // The pad is a window of its own, outside the app's; a test run draws the
  // same card in this window (so no window comes up over the person's work).
  T().command('forget_meeting_slots');
  T().command('forget_meeting');
  await oneTab();
  const datedName = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  const pad = () => $('#pad-inline #pad-card');
  const form = () => pad()?.dataset.form;
  const shown = () => $$('.pane').find((p) => datedName.test(p.querySelector('.title-input')?.value || '') && p.offsetParent);
  const rowsOf = (page) => $$('.block-row', page).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-'))}:${(r.querySelector('.blk') || r).textContent.trim()}`);
  const notepad = () => until(() => window.__n2pad && $('#pad-notes')?.dataset.ready, 3000);
  // The next meeting: its page opens in a tab of its own (the pane on screen
  // may be the same element as before, with the new page in it).
  // A call makes no page the person sees until they say yes (Listen, Notes
  // only, or a click on the small form): then its page opens in a tab.
  const meeting = async () => {
    T().command('meeting_ended');
    T().command('forget_meeting');
    T().command('meeting_started');
    return until(() => form() === 'ask', 5000);
  };
  const opened = () => until(shown, 5000);
  const dated = () => $$('.sidebar .tree-item .tree-title').filter((t) => datedName.test(t.textContent)).length;
  T().command('pad_inline');

  // 1. A meeting starts: the pad asks; nothing listens, and no page opens.
  const tabsAtStart = $$('.tab').length;
  T().command('meeting_started');
  const ask = await until(() => form() === 'ask' && $('#pad-listen'), 4000);
  check('When a meeting starts, its pad asks whether to listen, and the app does not listen by itself', ask && /Tell the others first\. The sound stays on this Mac\./.test(pad().textContent) && !$('.listen-bar.on') && !$('#listening-row'), pad()?.textContent);
  if (!ask) return T().command('pad_inline_off');
  check('While the pad asks, no page of the call opens', !shown() && $$('.tab').length === tabsAtStart, `${$$('.tab').length - tabsAtStart} new tabs`);
  check('The page itself does not ask while the pad does', !$('#listen-start'));
  // (Its page is in the sidebar, under the name the pad shows.)
  const name = $('.pad-title b')?.textContent || '';
  const listedNames = () => $$('.sidebar .tree-item .tree-title').map((t) => t.textContent).filter((t) => datedName.test(t)).map((t) => t.replace(datedName, ''));
  check('The pad has the name of the meeting and when it started', name && listedNames().includes(name) && /^(Started \d|\d)/.test($('.pad-title span').textContent), `${$('.pad-title').textContent} / ${listedNames().join(', ')}`);
  const look = getComputedStyle(pad());
  check('The pad is drawn as a card: its width, its round corners, its buttons on one line', Math.round(pad().getBoundingClientRect().width) === 328 && look.borderTopLeftRadius === '13px' && look.display === 'flex' && Math.abs($('#pad-listen').getBoundingClientRect().top - $('#pad-notes-only').getBoundingClientRect().top) < 6, `${Math.round(pad().getBoundingClientRect().width)} wide, corners ${look.borderTopLeftRadius}, ${look.display}`);
  check('The question has three ways out: Listen, Notes only, and not now', $('#pad-listen') && $('#pad-notes-only') && $('#pad-not-now') && $$('#pad-card button').length === 3, $$('#pad-card button').length);
  await wait(120);
  snap('pad-ask');

  // No answer: it becomes the small form, and does not listen.
  T().command('pad_old');
  check('With no answer, the question becomes the small form, and the app does not listen', (await until(() => form() === 'small', 3000)) && !$('.listen-bar.on') && $('#pad-away') && $('.pad-pill-name').textContent === name, form());
  check('Still no page opens: the pad asks, nothing else', !shown());
  await wait(120);
  snap('pad-small');
  $('#pad-pill').click();
  const page = await opened();
  check('A click on the small form is a yes: the page of the call opens', page);
  if (!page) return T().command('pad_inline_off');
  check('A click on the small form opens the notepad, which says that it does not listen', (await until(() => form() === 'open' && $('#pad-notes'))) && /Not listening/.test($('.pad-foot').textContent) && $('#pad-listen'), $('.pad-foot')?.textContent);

  // What is typed goes into the page.
  await notepad();
  check('An empty notepad says what it is for', $('#pad-notes').classList.contains('is-empty') && /They go into the page/.test($('#pad-notes').dataset.placeholder));
  window.__n2pad.type(['Beta moves to Friday', 'Sam writes the notes']);
  const typedIn = () => {
    const r = rowsOf(page);
    const n = r.indexOf('kind-heading2:Notes');
    return n >= 0 && r[n + 1] === 'kind-bulleted:Beta moves to Friday' && r[n + 2] === 'kind-bulleted:Sam writes the notes' && r[n + 3] === 'kind-heading2:Action items';
  };
  check('What is typed in the pad goes under Notes of the page, a bullet for each line', await until(typedIn, 4000), rowsOf(page).join(' | '));
  window.__n2pad.type(['Beta moves to Friday the 14th']);
  check('A line that is changed or taken out in the pad is changed or taken out in the page', await until(() => {
    const r = rowsOf(page);
    const n = r.indexOf('kind-heading2:Notes');
    return r[n + 1] === 'kind-bulleted:Beta moves to Friday the 14th' && r[n + 2] === 'kind-heading2:Action items';
  }, 4000), rowsOf(page).join(' | '));

  // Listening, from the pad.
  $('#pad-listen').click();
  check('"Listen" in the pad starts it: the pad and the page both say so', await until(() => $('.pad-foot .listen-dot') && $('#pad-stop') && $('.listen-bar.on') && $('#listening-row'), 4000), $('.pad-foot')?.textContent);
  check('The pad shows the last words it heard', await until(() => ($('.pad-latest')?.textContent || '').includes('Friday'), 4000), $('.pad-latest')?.textContent);
  await wait(120);
  snap('pad-listening');
  $('#pad-small').click();
  check('Made small while it listens, the pad still shows that it listens, and cannot be put away', (await until(() => form() === 'small')) && $('.pad-pill .listen-dot') && /^\d+:\d\d$/.test($('.pad-pill .pad-clock').textContent) && !$('#pad-away'), pad()?.textContent);
  $('#pad-pill').click();
  await until(() => form() === 'open');
  await notepad();
  check('Opened again, the notepad has what was typed', window.__n2pad.lines().join('|') === 'Beta moves to Friday the 14th', window.__n2pad.lines().join('|'));

  // Stop while the meeting goes on: a pause (the person may not want a part
  // heard). The notes of what was heard are written; the pad stays.
  $('#pad-stop').click();
  check('Stop while the meeting goes on is a pause: the pad stays the notepad, says that it does not listen, and offers Listen again', (await until(() => !$('.listen-bar.on') && /Not listening/.test($('.pad-foot')?.textContent || '') && $('#pad-listen'), 12000)) && form() === 'open' && !$('#meeting-place') && !$('#pad-keep-in'), `${form()} | ${$('.pad-foot')?.textContent}`);
  check('The notes of what was heard are written in the page then', await until(() => rowsOf(page).includes('kind-text:For the weekend.'), 12000), rowsOf(page).join(' | '));
  // The meeting ends: the last card.
  T().command('meeting_ended');
  check('When the meeting ends, the pad turns into its last card', await until(() => form() === 'end' && /^Over · \d+ min$/.test($('.pad-title span').textContent), 4000), pad()?.textContent);
  check('The card says that the notes are in the page', await until(() => $('#pad-status')?.textContent === 'The notes are in the page.', 12000), $('#pad-status')?.textContent);
  const r = rowsOf(page);
  const n = r.indexOf('kind-heading2:Notes');
  check('The notes of the AI come after the typed ones', r[n + 1] === 'kind-bulleted:Beta moves to Friday the 14th' && r[n + 2] === 'kind-text:For the weekend.', r.join(' | '));
  check('The card asks where to keep the notes, and the page does not', $('#pad-keep-in') && $('#pad-keep-name').textContent === 'Meeting notes' && !$('#meeting-place'), $('#pad-keep-name')?.textContent);
  $('#pad-keep-in').click();
  const places = () => $$('#pad-places .pad-place .pad-place-name').map((b) => b.textContent.trim());
  const spacesNow = $$('.side-foot .space-dot').map((d) => d.title);
  check('The list of places is the app’s own, with the page the notes are in marked', (await until(() => $('#pad-places'))) && !$('#pad-inline select') && (spacesNow.length > 1 ? spacesNow.every((s) => places().includes(s)) : places()[0] === 'No page (top of the list)') && places().includes('Getting Started') && !places().some((p) => datedName.test(p)) && $('#pad-places .pad-place.active .pad-place-name')?.textContent.trim() === 'Meeting notes', places().join(' | '));
  await wait(220);
  snap('pad-places');
  key($('#pad-card'), 'Escape');
  check('Escape shuts the list', await until(() => !$('#pad-places')));
  $('#pad-keep-in').click();
  (await until(() => $$('#pad-places .pad-place').find((b) => b.querySelector('.pad-place-name')?.textContent.trim() === 'Getting Started'))).click();
  check('A choice keeps the notes there, and the pad says so', (await until(() => form() === 'kept' && $('#pad-kept').textContent.trim() === 'Kept in Getting Started', 3000)) && $('.crumbs', page)?.textContent.includes('Getting Started'), `${pad()?.textContent} | ${$('.crumbs', page)?.textContent}`);
  check('Then the pad goes, and the page asks no more', (await until(() => !$('#pad-inline'), 4000)) && !$('#meeting-place'));

  // 2. Notes only, then the meeting ends by itself: the page of the last time, as one click.
  check('The next meeting gets a pad of its own', await meeting(), form());
  $('#pad-notes-only').click();
  await until(() => form() === 'open');
  const second = await opened();
  check('"Notes only" is a yes: the page of the call opens', second);
  await notepad();
  window.__n2pad.type(['Only typed']);
  await until(() => rowsOf(second).includes('kind-bulleted:Only typed'), 4000);
  T().command('meeting_ended');
  check('When a meeting with typed notes ends, the card says where they are and offers the page of the last time', (await until(() => form() === 'end', 4000)) && $('#pad-status')?.textContent === 'Your notes are in the page.' && $('.pad-same')?.textContent.trim() === 'Getting Started', pad()?.textContent);
  await wait(120);
  snap('pad-end');
  $('.pad-same').click();
  check('One click on it keeps the notes there', (await until(() => form() === 'kept', 3000)) && (await until(() => $('.crumbs', second)?.textContent.includes('Getting Started'), 3000)), $('.crumbs', second)?.textContent);
  await until(() => !$('#pad-inline'), 4000);

  // 3. The notes of the next meeting at this time start there: the card only says so.
  await meeting();
  $('#pad-notes-only')?.click();
  await until(() => form() === 'open');
  const third = await opened();
  await notepad();
  window.__n2pad.type(['Third']);
  await until(() => rowsOf(third).includes('kind-bulleted:Third'), 4000);
  T().command('meeting_ended');
  check('Notes that are where the person chose before are not asked about again: the card says where they are', (await until(() => form() === 'end', 4000)) && !$('#pad-keep-in') && $('#pad-told')?.textContent === 'These notes are in “Getting Started”, as before.', pad()?.textContent);
  $('#pad-done').click();
  check('"Done" puts that card away', await until(() => !$('#pad-inline'), 4000));

  // 4. A pad that was not used goes when the meeting ends, and so does its empty page; the × leaves no page.
  T().command('meeting_ended');
  const before = dated();
  await meeting();
  T().command('meeting_ended');
  check('A pad that was not used goes when the meeting ends, with no question, and leaves no page', (await until(() => !$('#pad-inline'), 3000)) && (await until(() => dated() === before, 3000)), `${dated() - before} pages more`);
  await meeting();
  const tabsNow = $$('.tab').length;
  $('#pad-not-now').click();
  check('The × of the question is a no: the pad goes, and no page of the call is kept or opened', (await until(() => !$('#pad-inline'), 3000)) && (await until(() => dated() === before, 3000)) && $$('.tab').length === tabsNow && !$('#listen-start'), `${dated() - before} pages more`);
  T().command('meeting_started');
  await wait(600);
  check('After a no, the same call asks no more', !$('#pad-inline') && dated() === before);
  await meeting();
  $('#pad-notes-only').click();
  await until(() => form() === 'open');
  const sixth = await opened();
  await notepad();
  window.__n2pad.type(['Sixth']);
  await until(() => rowsOf(sixth).includes('kind-bulleted:Sixth'), 4000);
  T().command('forget_meeting_slots');
  T().command('meeting_ended');
  await until(() => form() === 'end', 4000);
  T().command('pad_old');
  check('A last card that nobody answers goes away, and the page asks in its place', (await until(() => !$('#pad-inline'), 3000)) && (await until(() => $('#meeting-place', sixth), 3000)));
  $('#meeting-place-done')?.click();
  T().command('meeting_ended');
  T().command('pad_inline_off');
});

// One meeting has one page: when the person stops the listening for a part
// and starts it again, when the app takes the meeting as over while the call
// goes on (the call went quiet), when the page was put in the Trash, and when
// the app was closed and opened again during the call.
await padRun('One meeting has one page', async () => {
  T().command('forget_meeting_slots');
  T().command('forget_meeting');
  await oneTab();
  const datedName = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  const pad = () => $('#pad-inline #pad-card');
  const form = () => pad()?.dataset.form;
  const shown = () => $$('.pane').find((p) => datedName.test(p.querySelector('.title-input')?.value || '') && p.offsetParent);
  const rowsOf = (page) => $$('.block-row', page).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-'))}:${(r.querySelector('.blk') || r).textContent.trim()}`);
  const count = (page, row) => rowsOf(page).filter((r) => r === row).length;
  // Each notes page that the app makes opens in a tab of its own.
  const tabs = () => $$('.tab').length;
  const heard = () => until(() => $('.listen-bar.on') && ($('.pad-latest')?.textContent || '').includes('Friday'), 6000);
  // The sound of the meeting's program comes back, with no microphone in use.
  const back = async () => {
    delete document.body.dataset.meetingBack;
    T().command('meeting_back');
    return until(() => document.body.dataset.meetingBack, 2000);
  };
  T().command('pad_inline');

  // 1. The person listens, stops for a part, and listens again.
  T().command('meeting_started');
  if (!(await until(() => form() === 'ask' && $('#pad-listen'), 4000))) return;
  $('#pad-listen').click();
  const page = await until(shown, 4000);
  if (!page) return;
  const made = tabs();
  const line = () => page.querySelector('.blk')?.textContent || '';
  await heard();
  $('#pad-stop').click();
  await until(() => !$('.listen-bar.on') && rowsOf(page).includes('kind-text:For the weekend.'), 14000);
  check('Stop during the meeting: the notes of what was heard are in the page, the pad stays, and nothing asks yet where to keep the notes', form() === 'open' && $('#pad-listen') && !$('#meeting-place') && !$('#pad-keep-in') && count(page, 'kind-toggle:Transcript') === 1, `${form()} | ${rowsOf(page).join(' | ')}`);
  $('#pad-listen').click();
  await heard();
  await wait(300);
  T().command('meeting_ended');
  check('Listen again, then the end of the meeting: one page, with one Notes part, one Action items part and one transcript', (await until(() => form() === 'end' && $('#pad-status')?.textContent === 'The notes are in the page.', 16000)) && tabs() === made && count(page, 'kind-heading2:Notes') === 1 && count(page, 'kind-heading2:Action items') === 1 && count(page, 'kind-toggle:Transcript') === 1, `${tabs() - made} new tabs | ${$('#pad-status')?.textContent} | ${rowsOf(page).join(' | ')}`);
  check('The date line has the end of the meeting', /\(\d+ min\)/.test(line()), line());

  // 2. The call goes on after all (it only went quiet): it is noticed again.
  T().command('meeting_started');
  check('A call that is noticed again is the same meeting: no second page, the pad is the notepad again, the date line has no end, and nothing asks where to keep the notes', (await until(() => form() === 'open', 4000)) && tabs() === made && (await until(() => !/\(\d+ min\)/.test(line()), 3000)) && !$('#meeting-place') && !$('#pad-keep-in'), `${form()} | ${tabs() - made} new tabs | ${line()}`);
  check('The app does not listen again by itself: the pad says that listening stopped and offers it with one press, and so does the page', !$('.listen-bar.on') && /Listening stopped/.test($('.pad-foot')?.textContent || '') && $('#pad-listen') && (await until(() => $('#listen-start', page), 3000)), $('.pad-foot')?.textContent);
  await wait(150);
  snap('pad-goes-on');

  // 3. Over again; the last card goes by itself. Then the sound of the
  // meeting's program alone (the person is muted) says that it goes on.
  T().command('meeting_ended');
  await until(() => form() === 'end', 4000);
  T().command('pad_old');
  await until(() => !$('#pad-inline'), 4000);
  check('Sound alone after the end is the same meeting too: no new page, the pad comes back in its small form, and the page asks nothing', (await back()) === 'yes' && (await until(() => form() === 'small', 4000)) && tabs() === made && !$('#meeting-place'), `${document.body.dataset.meetingBack} | ${form()} | ${tabs() - made} new tabs`);

  // 4. The person puts the page in the Trash while the app listens to it.
  $('#pad-pill').click();
  await until(() => form() === 'open' && $('#pad-listen'), 3000);
  $('#pad-listen').click();
  await heard();
  $('.pagebar .icon-btn[title="Page options"]', page).click();
  menuItem(await until(() => $('.page-menu')), 'Move to Trash').click();
  check('A page that goes to the Trash is not listened to any more, and its pad goes', (await until(() => !$('.listen-bar.on') && !$('#listening-row') && !$('#pad-inline'), 6000)), `${!!$('.listen-bar.on')} ${!!$('#pad-inline')}`);
  await wait(400);
  const left = tabs();
  T().command('meeting_ended');
  await wait(300);
  T().command('meeting_started');
  await wait(600);
  check('When that meeting ends, and when its call is noticed again, no other page is made, and nothing listens', tabs() === left && !$('#pad-inline') && !$('.listen-bar.on') && !shown(), `${tabs() - left} new tabs`);

  // 5. Half an hour after a meeting went quiet, sound alone makes nothing;
  // a call that starts then is a new meeting.
  T().command('meeting_ended');
  T().command('meeting_old');
  check('Half an hour after the last meeting went quiet, sound alone makes no page', (await back()) === 'no' && tabs() === left && !$('#pad-inline'), `${document.body.dataset.meetingBack} | ${tabs() - left} new tabs`);
  T().command('meeting_started');
  const next = await until(() => tabs() === left && form() === 'ask' && !shown(), 5000);
  check('A call that starts then is a new meeting, with a pad of its own (its page opens on a yes)', next, `${tabs() - left} new tabs | ${form()}`);

  // 6. The app is closed and opened again during the call (an update does that).
  T().command('meeting_restart');
  await until(() => !$('#pad-inline'), 3000);
  T().command('meeting_started');
  check('After the app was closed and opened again, the call is the same meeting: no other page, and its pad comes back', (await until(() => $('#pad-inline') && form() === 'small', 4000)) && tabs() === left, `${form()} | ${tabs() - left} new tabs`);
  // The same, while the person is muted: the app knows the program of the
  // call, and the sound of that program is enough.
  delete document.body.dataset.meetingApp;
  T().command('meeting_restart');
  await until(() => !$('#pad-inline'), 3000);
  check('After the app was closed and opened again, it knows the program of the call, and the sound of that program alone is the same meeting', (await until(() => document.body.dataset.meetingApp === 'Microsoft Teams', 2000)) && (await back()) === 'yes' && (await until(() => $('#pad-inline') && form() === 'small', 4000)) && tabs() === left, `${document.body.dataset.meetingApp} | ${document.body.dataset.meetingBack} | ${form()} | ${tabs() - left} new tabs`);
  T().command('meeting_ended');
  T().command('forget_meeting');
});

// Only when asked for (NOTION2_REAL_PAD=1): opens the meeting pad's real
// window at the corner of the screen for some seconds, and clicks and types
// in it. It proves what the check above cannot: that the window opens, is as
// big as its card, takes no keyboard, and closes again without a fault.
if (window.__n2_real_pad) {
  await padRun('Real meeting pad window', async () => {
    T().command('forget_meeting_slots');
    T().command('forget_meeting');
    await oneTab();
    const datedName = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
    const probe = async () => {
      delete document.body.dataset.padProbe;
      T().command('pad_probe');
      const raw = await until(() => document.body.dataset.padProbe, 1500);
      return raw ? JSON.parse(raw) : {};
    };
    const probed = async (ok, ms = 6000) => {
      const t0 = performance.now();
      let last = {};
      while (performance.now() - t0 < ms) {
        last = await probe();
        if (ok(last)) return last;
        await wait(120);
      }
      return { failed: true, ...last };
    };
    const inPad = (script) => T().command('pad_run:' + script);
    const rows = (page) => $$('.block-row', page).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-'))}:${(r.querySelector('.blk') || r).textContent.trim()}`);
    T().command('pad_window');
    T().command('meeting_started');
    const pageNow = () => $$('.pane').find((p) => datedName.test(p.querySelector('.title-input')?.value || '') && p.offsetParent);
    let p = await probed((p) => p.open && p.shown && p.form === 'ask' && p.wide);
    check('The pad opens in a window of its own, as big as its card', !p.failed && p.wide === 328 && p.high > 110 && p.high < 190 && !$('#pad-inline'), JSON.stringify(p));
    check('The window is a panel that does not make the app the active one, on every desktop and beside a full screen', p.panel && p.nonactivating && p.all_spaces && p.beside_full_screen && p.level === 3, JSON.stringify(p));
    check('It takes no keyboard as it opens, and cannot while it only asks', !p.key && !p.can_key, JSON.stringify(p));
    check('It sits at the corner of the screen, and is kept out of screen shares', Math.abs(p.gap_right - 20) < 1.5 && Math.abs(p.gap_bottom - 20) < 1.5 && p.hidden_from_shares, JSON.stringify(p));
    check('macOS says that it is on the space in view', p.on_space, JSON.stringify(p));
    // A press of the mouse on the pad acts at once, though the pad does not
    // have the keyboard and the app is not the active one (a true mouse
    // event to the window, not a click made by script).
    const press = (selector) => T().command('pad_press:' + selector);
    press('#pad-listen');
    check('A press of the mouse on Listen acts at the first press, and starts the listener of the app', await until(() => $('.listen-bar.on') && $('#listening-row'), 5000));
    const page = await until(pageNow, 4000);
    p = await probed((p) => p.form === 'open' && p.high > 200);
    check('…and the pad still does not have the keyboard, nor is the app the active one', !p.failed && !p.key && !p.app_active, JSON.stringify(p));
    check('As the notepad the pad may take the keyboard, and says that a first press acts', p.can_key && p.first_press && p.hit === 'WryWebView', JSON.stringify(p));
    // (Not proven here, a person must try it: that a press in the notepad
    // gives the pad the keyboard for typing. The window server gives the
    // keyboard on a real click; an event made inside the app does not get it.)
    check('The window grew with the notepad, its bottom right corner kept', !p.failed && p.wide === 328 && p.high > 200 && Math.abs(p.gap_right - 20) < 1.5 && Math.abs(p.gap_bottom - 20) < 1.5, JSON.stringify(p));
    inPad("(async () => { for (let i = 0; i < 40 && !(window.__n2pad && document.querySelector('#pad-notes')?.dataset.ready); i++) await new Promise((r) => setTimeout(r, 50)); window.__n2pad.type(['From the pad window']); })()");
    check('What is typed in the pad window goes into the page', await until(() => rows(page).includes('kind-bulleted:From the pad window'), 5000), rows(page).join(' | '));
    press('#pad-small');
    p = await probed((p) => p.form === 'small' && p.high < 40);
    check('A first press on a button of the notepad acts: made small, the window is a pill at the same corner, and cannot take the keyboard', !p.failed && p.high === 32 && p.wide < 328 && p.radius === 16 && Math.abs(p.gap_right - 20) < 1.5 && Math.abs(p.gap_bottom - 20) < 1.5 && !p.can_key && !p.key, JSON.stringify(p));
    // (What the page gets from the press is written down, to say it if the press does nothing.)
    inPad("window.__saw = []; for (const t of ['mousedown', 'mouseup', 'click']) document.addEventListener(t, (e) => window.__saw.push(t + ':' + (e.target.id || e.target.className || e.target.tagName) + '@' + Math.round(e.clientX) + ',' + Math.round(e.clientY)), true); return 'ready';");
    await probed((p) => p.result === 'ready', 2000);
    press('#pad-pill');
    p = await probed((p) => p.form === 'open' && p.high > 200, 4000);
    if (p.failed) {
      inPad("const r = document.querySelector('#pad-pill')?.getBoundingClientRect(); return { saw: window.__saw, pill: r && [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)], view: [innerWidth, innerHeight] };");
      p = { ...p, ...(await probed((p) => p.result && p.result.saw, 2000)) };
    }
    check('A press of the mouse on the pill opens the notepad again, at the first press', !p.failed, JSON.stringify(p.result) + ' ' + JSON.stringify(p));
    press('#pad-stop');
    await until(() => !$('.listen-bar.on'), 12000);
    p = await probed((p) => p.form === 'open' && !$('.listen-bar.on'));
    check('A press on Stop during the meeting pauses the listening, and the pad stays the notepad', !p.failed && p.wide === 328, JSON.stringify(p));
    T().command('meeting_ended');
    p = await probed((p) => p.form === 'end' && p.high > 120);
    check('When the meeting ends, the pad turns into its last card', !p.failed && p.wide === 328, JSON.stringify(p));
    press('#pad-keep-in');
    const withList = await probed((q) => q.form === 'end' && q.high > p.high + 100);
    check('A press of the mouse on the field opens the list of pages, and the window grows for it', !withList.failed, JSON.stringify(withList));
    press('#pad-done');
    p = await probed((p) => !p.open, 6000);
    check('After a press on "Done" the pad says where the notes are, and its window closes', !p.failed && !p.open, JSON.stringify(p));
    await wait(600);
    check('The app is still there, with its page', !!$('.pane') && !$('#meeting-place'));
    T().command('meeting_ended');
    T().command('pad_window_off');
  });
}

// Only when asked for (NOTION2_PAD_TRY=1): the meeting pad's real window, left
// on the screen for the person to try by hand (a throwaway workspace, the
// stand-in listener, the stand-in AI). It ends when the pad has gone, or
// after five minutes.
if (window.__n2_pad_try) {
  await padRun('Meeting pad to try by hand', async () => {
    T().command('forget_meeting_slots');
    T().command('forget_meeting');
    const open = async () => {
      delete document.body.dataset.padProbe;
      T().command('pad_probe');
      const raw = await until(() => document.body.dataset.padProbe, 1500);
      return raw ? JSON.parse(raw).open : false;
    };
    // Started by the person's own click, this copy is made the program in
    // front: its window steps back, so that only the pad is before them.
    T().command('step_back');
    T().command('pad_window');
    T().command('meeting_started');
    let shown = false;
    const t0 = performance.now();
    while (performance.now() - t0 < 300000) {
      if (await open()) shown = true;
      else if (shown) break;
      if (performance.now() - t0 < 2500) T().command('step_back');
      await wait(500);
    }
    check('The pad was on the screen for a try by hand', shown);
    T().command('meeting_ended');
    T().command('pad_window_off');
  });
}

await run('Notes of a video', async () => {
  await oneTab();
  $('#player-close')?.click();
  const title = await newPage('Lesson');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Lesson');
  typeIn(first, 'https://youtu.be/jNQXAC9IVRw');
  await until(() => $('.video-embed', pane()), 3000);
  // A line of the person's own, after the video.
  T().setSel(first, first.textContent.length);
  key(first, 'Enter');
  const second = await until(() => document.activeElement !== first && document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(second, 'My own line');
  await wait(300);
  const rows = () => $$('.block-row', pane()).map((r) => `${[...r.classList].find((c) => c.startsWith('kind-'))}:${(r.querySelector('.blk') || r).textContent.trim()}`);
  const count = (row) => rows().filter((r) => r === row).length;
  const play = async () => {
    $('.video-embed', pane()).click();
    return until(() => $('#player-dock'), 3000);
  };

  // The option is there when a video plays. Nothing listens by itself.
  let dock = await play();
  const option = await until(() => $('#player-notes', dock), 2000);
  check('A video that plays has the option to take notes of it, and nothing listens by itself', option && option.textContent.trim() === 'Take notes' && !$('.listen-bar.on') && !$('.listen-dot', dock), dock?.textContent);
  check('The option says that only what the Mac plays is heard, not the microphone', /only what the Mac plays, not the microphone/.test(option?.title || ''), option?.title);
  if (!option) return;
  option.click();
  check('"Take notes" starts it: the player shows a dot, the time and Stop, and the page says that it listens', await until(() => $('.listen-dot', dock) && $('#player-notes-stop', dock) && /^\d+:\d\d$/.test($('.player-clock', dock)?.textContent || '') && $('.listen-bar.on', pane()) && !$('#player-notes'), 4000), dock.textContent);
  await wait(700);
  await wait(120);
  snap('video-notes-on');
  $('#player-notes-stop').click();
  const written = () => {
    const r = rows();
    const v = r.findIndex((x) => x.includes('youtu.be'));
    const words = r.indexOf('kind-toggle:Transcript of the video');
    return v >= 0 && r[v + 1] === 'kind-toggle:Notes of the video' && r[v + 2] === 'kind-text:For the weekend.' && words > v + 2 && r[r.length - 1] === 'kind-text:My own line';
  };
  check('Stop puts the notes of the video under its line, its words in a toggle under them, and leaves the person’s own lines', await until(written, 10000), rows().join(' | '));
  check('The words are in a closed toggle (its lines are not drawn), the notes in an open one', count('kind-toggle:Transcript of the video') === 1 && !rows().some((r) => /^kind-text:\d+:\d\d — /.test(r)) && rows().includes('kind-todo:Buy milk'), rows().join(' | '));
  check('The player has its option again, and nothing listens', await until(() => $('#player-notes', dock) && !$('.listen-bar.on') && !$('.listen-dot', dock), 3000), dock.textContent);
  await wait(150);
  snap('video-notes');

  // Closing the player ends it too.
  $('#player-notes').click();
  await until(() => $('#player-notes-stop'));
  await wait(600);
  $('#player-close').click();
  check('Closing the player while it listens stops it, and what was heard is written', (await until(() => !$('#player-dock') && !$('.listen-bar.on') && count('kind-toggle:Transcript of the video') === 2, 8000)), rows().join(' | '));
  await until(() => count('kind-toggle:Notes of the video') === 2, 6000);

  // With no AI tool: the words alone, and no word of notes or of an AI.
  T().command('ai_off_for_test');
  dock = await play();
  const plain = await until(() => $('#player-notes', dock), 2000);
  check('With no AI tool the option is to write the video down, with no word of notes or of an AI', plain && plain.textContent.trim() === 'Write it down' && !/\bnotes\b|\bAI\b/i.test(plain.title), `${plain?.textContent} | ${plain?.title}`);
  plain?.click();
  await until(() => $('#player-notes-stop'));
  check('…and the page’s own Stop says nothing of notes', $('#listen-stop', pane())?.textContent.trim() === 'Stop', $('#listen-stop', pane())?.textContent);
  await wait(600);
  $('#player-notes-stop').click();
  check('Then Stop keeps the words and writes no notes', (await until(() => !$('.listen-bar.on') && count('kind-toggle:Transcript of the video') === 3, 8000)) && count('kind-toggle:Notes of the video') === 2, rows().join(' | '));
  T().command('ai_on_for_test');
  $('#player-close')?.click();
  await until(() => !$('#player-dock'));
});

await run('The voice box with the app’s own listener', async () => {
  await oneTab();
  await openSettings();
  await settingsSection('AI');
  const pick = await until(() => $('#voice-engine'));
  check('Settings offers the app’s own listener for the voice box', pick && [...pick.options].map((o) => o.value).join(',') === 'dictation,builtin' && pick.value === 'dictation', pick && pick.value);
  pick.value = 'builtin';
  pick.dispatchEvent(new Event('change', { bubbles: true }));
  await wait(120);
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
  speakPage();
  const box = await until(() => $('.modal.voice #voice-text'));
  const real = !!window.__n2_real_listener;
  const heardIn = (el) => (real ? el.value.trim().length > 3 : el.value === 'So the budget is agreed.');
  check('With it, the words appear in the box as they are heard', await until(() => heardIn(box), real ? 15000 : 4000) && !$('.voice-help'), box?.value);
  key(document.body, 'Escape');
  $('.modal-backdrop')?.click();
  await until(() => !$('.modal.voice'));
  // The microphones beside text boxes use the same listener.
  $$('.tile').find((t) => t.title === 'Home').click();
  await until(() => $('.home'));
  T().command('ai_anywhere');
  const ask = await until(() => $('#ask-notes-input'));
  ask.value = 'About Friday:';
  ask.dispatchEvent(new Event('input', { bubbles: true }));
  const mic = $('[data-dictate="ask-notes-input"]');
  mic.click();
  check('The microphone beside a text box shows that it listens', await until(() => mic.classList.contains('hearing') && document.body.dataset.dictating === 'ask-notes-input', 4000));
  check('While it listens the button has bars that move and a ring, and the box says so', getComputedStyle(mic, '::before').animationName === 'mic-bar' && getComputedStyle(mic, '::after').animationName === 'mic-bar' && getComputedStyle(mic).animationName === 'mic-ring' && ask.placeholder === 'Listening…', `${getComputedStyle(mic, '::before').animationName} ${getComputedStyle(mic).animationName} "${ask.placeholder}"`);
  const typed = await until(() => (real ? ask.value.length > 'About Friday:'.length + 3 : ask.value === 'About Friday: So the budget is agreed.'), real ? 15000 : 4000);
  check('What is heard goes into that box, after what it held', typed && ask.value.startsWith('About Friday: ') && !$('#ask-notes-go').disabled, ask.value);
  check('After a pause it stops listening by itself', await until(() => !mic.classList.contains('hearing') && !document.body.dataset.dictating, 9000));
  check('…and the box has its own hint again', ask.placeholder !== 'Listening…' && ask.placeholder.length > 5, ask.placeholder);
  // Escape while it listens stops the listening, and the box stays.
  mic.click();
  await until(() => mic.classList.contains('hearing'), 4000);
  const line = $('.modal.ask-notes .ask-listening');
  check('While it listens, a line says so with a clock, in place of the foot', line && getComputedStyle(line).display === 'flex' && getComputedStyle($('.modal.ask-notes .ask-foot')).display === 'none' && /^\d:\d\d$/.test($('[data-hear-clock]').textContent), `${line && getComputedStyle(line).display} "${$('[data-hear-clock]')?.textContent}"`);
  key(ask, 'Escape');
  check('Escape stops the listening and leaves the box open', (await until(() => !mic.classList.contains('hearing'), 2000)) && $('#ask-notes-input') && getComputedStyle(line).display === 'none');
  await until(() => !document.body.dataset.dictating, 6000);
  const kept = ask.value;
  mic.click();
  await until(() => mic.classList.contains('hearing'), 4000);
  mic.click();
  check('Pressed again, the microphone stops', await until(() => !mic.classList.contains('hearing'), 1000) && (await until(() => !document.body.dataset.dictating, 6000)), document.body.dataset.dictating);
  check('The second time adds to the box and loses nothing', ask.value.startsWith(kept), ask.value);
  key(document.body, 'Escape');
  $('.modal-backdrop')?.click();
  await until(() => !$('#ask-notes-input'));
  await openSettings();
  await settingsSection('AI');
  const back = await until(() => $('#voice-engine'));
  back.value = 'dictation';
  back.dispatchEvent(new Event('change', { bubbles: true }));
  await wait(100);
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
});

await run('One list of open to-dos', async () => {
  await oneTab();
  const card = await until(() => $('#home-todos'));
  const rows = () => $$('#home-todos .todo-row');
  check('Home lists the open to-dos from all pages, with the page each is on', card && rows().length >= 2 && rows().every((r) => r.querySelector('.todo-from')?.textContent), rows().map((r) => r.textContent).join(' | '));
  const count = () => Number(($('#todos-all')?.textContent || '').replace(/\D/g, '')) || rows().length;
  const before = count();
  const first = rows()[0];
  const text = first.querySelector('.todo-text').textContent;
  const from = first.querySelector('.todo-from').textContent;
  // (Two to-dos of one page can have the same words: one of them goes.)
  const same = () => rows().filter((r) => r.querySelector('.todo-text').textContent === text && r.querySelector('.todo-from').textContent === from).length;
  const sameBefore = same();
  first.querySelector('.todo-box').click();
  check('Ticking one here takes it off the list', await until(() => count() === before - 1 && same() === sameBefore - 1, 3000), `${before} -> ${count()}, with these words ${sameBefore} -> ${same()}`);
  const all = $('#todos-all');
  if (all) {
    all.click();
    const modal = await until(() => $('.modal.todos'));
    check('"All" shows every open to-do, grouped by page', modal && $$('.todos-group', modal).length >= 2 && $$('.todo-row', modal).length === count(), `${$$('.todo-row', modal || document.body).length} of ${count()}`);
    key(document.body, 'Escape');
    $('.modal-backdrop')?.click();
    await until(() => !$('.modal.todos'));
  }
  const second = rows()[0];
  const where = second.querySelector('.todo-from').textContent;
  second.querySelector('.todo-body').click();
  check('Clicking one opens its page', await until(() => shownPane(where), 3000), where);
});

await run('Voice without an AI tool', async () => {
  await oneTab();
  // As if no AI tool were installed.
  T().command('ai_off_for_test');
  await until(() => document.body.dataset.ai === '0');
  const before = $$('.sidebar .tree-item').length;
  delete document.body.dataset.dictating;
  speakPage();
  const note = await until(() => $$('.pane').find((p) => /^Voice note – /.test(p.querySelector('.title-input')?.value || '') && p.offsetParent), 4000);
  check('With no AI, the microphone opens no AI box: a new page takes your words', note && !$('.modal.voice') && $$('.sidebar .tree-item').length === before + 1, `${$$('.pane .title-input').map((t) => t.value).join(' | ')} modal:${!!$('.modal.voice')}`);
  check('Plain voice starts in its first line', await until(() => document.body.dataset.dictating === 'page' && document.activeElement?.classList.contains('blk') && note?.contains(document.activeElement), 3000), `${document.body.dataset.dictating} ${document.activeElement?.className}`);
  delete document.body.dataset.dictating;
  const mic = await until(() => $('#line-mic.show'));
  mic.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
  mic.click();
  check('The microphone on a line types your words there, with no AI box', await until(() => document.body.dataset.dictating === 'line') && !$('.modal.voice') && mic.title.includes('your words'), `${document.body.dataset.dictating} | ${mic.title}`);
  typeIn(document.activeElement, 'some words to select');
  await wait(150);
  T().setSel(document.activeElement, 0, 4);
  await until(() => $('#fmt-bar.show'));
  check('Speak to edit, which needs the AI, is not offered', $('#fmt-bar.show') && getComputedStyle($('#fmt-bar [data-fmt="speak"]')).display === 'none');
  T().command('meeting_recap');
  await wait(400);
  check('What needs the AI opens nothing and says nothing', !$('.ai-panel') && !$('.modal.voice') && !/AI/.test($('.toast')?.textContent || ''), $('.toast')?.textContent);
  T().command('ai_on_for_test');
  check('With an AI tool again, the AI voice is back', await until(() => document.body.dataset.ai === '1') && getComputedStyle($('#fmt-bar [data-fmt="speak"]')).display !== 'none');
});

// ----- speed measurements (reported, not pass/fail) -----
// Each one counts animation frames while it runs: a frame gap over 25 ms is a
// visible hitch. Needs a long page called "Long page" seeded before the run.
const perf = {};
const measure = async (fn) => {
  const gaps = [];
  let last = performance.now();
  let on = true;
  const tick = (t) => {
    gaps.push(t - last);
    last = t;
    if (on) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  const t0 = performance.now();
  await fn();
  const ms = performance.now() - t0;
  on = false;
  await wait(40);
  return { ms: Math.round(ms), frames: gaps.length, hitches: gaps.filter((g) => g > 25).length, worstFrameMs: Math.round(Math.max(0, ...gaps)) };
};
await run('Dates on to-dos and the day of the calendar', async () => {
  await oneTab();
  const iso = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const dayFrom = (n) => {
    const d = new Date();
    d.setDate(d.getDate() + n);
    return d;
  };
  const title = await newPage('Website plan');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Website plan');
  const rows = () => $$('.block-row.kind-todo', pane());
  // "[] " makes the line a to-do; its words are typed after that.
  typeIn(first, '[] ');
  await until(() => rows().length === 1);
  typeIn(await until(() => document.activeElement?.classList.contains('blk') && document.activeElement), 'Send the update list');
  const chip = (i) => $('.due-chip', rows()[i]);
  check('A to-do line has a place for its day at its end', await until(() => rows().length === 1 && chip(0)?.classList.contains('none')));

  // The first to-do: today, from the menu.
  chip(0).click();
  const menu = await until(() => $('.due-menu.anchored'));
  await wait(260); // it grows a little as it opens
  const at = menu?.getBoundingClientRect();
  check('A click there opens the days to choose from', menu && $$('.menu-item', menu).map((m) => m.firstElementChild.textContent).join('|') === 'Today|Tomorrow|Next week' && $('input[type=date]', menu), menu?.textContent);
  check('The menu is in the window, with its right edge at the day', at && at.left >= 0 && at.right <= innerWidth && at.bottom <= innerHeight && Math.abs(at.right - chip(0).getBoundingClientRect().right) <= 2, at && `${Math.round(at.left)}–${Math.round(at.right)} of ${innerWidth}`);
  snap('due-menu');
  menuItem(menu, 'Today').click();
  check('"Today" puts the day at the end of the line and shuts the menu', await until(() => chip(0)?.textContent.trim() === 'Today' && !$('.due-menu')), chip(0)?.textContent);

  // Return on a to-do with a day: the new one has no day of its own.
  const b0 = $('.blk', rows()[0]);
  b0.focus();
  T().setSel(b0, b0.textContent.length);
  key(b0, 'Enter');
  await until(() => rows().length === 2 && document.activeElement !== b0 && document.activeElement?.classList.contains('blk'));
  typeIn(document.activeElement, 'Book the room');
  check('A new to-do under it starts with no day', await until(() => rows().length === 2 && chip(1)?.classList.contains('none') && chip(0)?.textContent.trim() === 'Today'));

  // The second: three days from now, from the date field.
  const setDay = async (i, day) => {
    chip(i).click();
    const field = await until(() => $('.due-menu input[type=date]'));
    field.value = iso(day);
    field.dispatchEvent(new Event('change', { bubbles: true }));
    await until(() => !$('.due-menu'));
  };
  await setDay(1, dayFrom(3));
  const weekday = dayFrom(3).toLocaleDateString('en-US', { weekday: 'short' });
  check('A day in the week ahead shows as the name of the day', await until(() => chip(1)?.textContent.trim() === weekday), `${chip(1)?.textContent} for ${weekday}`);

  // A reminder at a time of that day: a bell and the time on the chip.
  chip(1).click();
  const remindAt = await until(() => $('.due-menu input[data-remind]'));
  check('The menu of the day has a time for a reminder', remindAt && remindAt.type === 'time');
  remindAt.value = '09:30';
  remindAt.dispatchEvent(new Event('change', { bubbles: true }));
  check('A time puts a bell and the time on the chip', await until(() => chip(1)?.textContent.trim() === `${weekday} 9:30 AM` && chip(1).querySelector('svg, .ico'), 3000), chip(1)?.textContent);
  check('The menu then offers to take the reminder away', await until(() => $('.due-menu [data-remind-off]'), 2000));
  key(document.body, 'Escape');
  await until(() => !$('.due-menu'), 2000);

  // The third: yesterday. It is late.
  const b1 = $('.blk', rows()[1]);
  b1.focus();
  T().setSel(b1, b1.textContent.length);
  key(b1, 'Enter');
  await until(() => rows().length === 3 && document.activeElement !== b1 && document.activeElement?.classList.contains('blk'));
  typeIn(document.activeElement, 'Look at the import story');
  await setDay(2, dayFrom(-1));
  check('A day that is past shows as late', await until(() => chip(2)?.classList.contains('late') && chip(2).textContent.trim() === 'Yesterday'), chip(2)?.textContent);
  snap('due-lines');

  // The day goes with the to-do when the page is copied as Markdown.
  delete document.body.dataset.copied;
  T().command('copy_markdown');
  const copied = await until(() => document.body.dataset.copied);
  check('Markdown keeps the day at the end of the line', copied && copied.includes(`- [ ] Send the update list 📅 ${iso(new Date())}`) && copied.includes(`- [ ] Look at the import story 📅 ${iso(dayFrom(-1))}`), copied);
  check('…and the time of a reminder after the day', copied && copied.includes(`- [ ] Book the room 📅 ${iso(dayFrom(3))} ⏰ 09:30`), copied);

  // Home: those with a day come first.
  await oneTab();
  const home = () => $$('#home-todos .todo-row');
  const said = (r) => `${r.querySelector('.todo-text').textContent} [${r.querySelector('.due-chip')?.textContent || ''}${r.querySelector('.due-chip.late') ? ', late' : ''}]`;
  check('On Home the to-dos with a day come first, the late one at the top', await until(() => home().length >= 3 && said(home()[0]) === 'Look at the import story [Yesterday, late]' && said(home()[1]) === 'Send the update list [Today]' && said(home()[2]) === `Book the room [${weekday}]`, 3000), home().slice(0, 3).map(said).join(' | '));

  // The calendar's day view.
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar'));
  const seg = (name) => $$('.cal-toolbar .segmented button').find((b) => b.textContent === name);
  seg('Day').click();
  await until(() => $('.cal-week.one-day'));
  // An earlier check can have left another day in view.
  $('.cal-nav .secondary-btn').click();
  await until(() => $('.cal-title').textContent.startsWith(new Date().toLocaleDateString('en-US', { weekday: 'long' })));
  check('The calendar has a day view: one day at the left, a place for notes at the right', await until(() => $('.cal-day-split .cal-week.one-day') && $('.day-notes')) && $$('.cal-col').length === 1 && getComputedStyle($('.cal-week-head')).display === 'none');
  check('Its title names the day', $('.cal-title').textContent.startsWith(new Date().toLocaleDateString('en-US', { weekday: 'long' })), $('.cal-title').textContent);
  const due = () => $$('#day-todos .todo-row');
  const dueNames = () => due().map((r) => r.querySelector('.todo-text').textContent).join('|');
  check('What is due today is above the hours, with what is late, and the page each is on', await until(() => dueNames() === 'Look at the import story|Send the update list') && due().every((r) => r.querySelector('.todo-from').textContent === 'Website plan') && due()[0].querySelector('.due-chip.late') && !due()[1].querySelector('.due-chip') && $('.day-todos-label').textContent === 'To do today', dueNames());

  // A new event, by a click on a time.
  const col = () => $('.cal-week.one-day .cal-col');
  const r = col().getBoundingClientRect();
  const where = { bubbles: true, clientX: r.left + 10, clientY: r.top + 13 * 48 + 3 };
  col().dispatchEvent(new MouseEvent('mousedown', where));
  col().dispatchEvent(new MouseEvent('click', where));
  const slot = await until(() => $('#ev-draft'));
  check('A click on a time adds an event, as in the week', slot);
  slot.value = 'Design review';
  slot.dispatchEvent(new Event('input', { bubbles: true }));
  key(slot, 'Enter');
  const ev = await until(() => $$('.cal-event:not(.draft)').find((e) => e.querySelector('.ev-title')?.textContent === 'Design review'));
  ev.click();
  const bare = await until(() => $('.day-notes-title')?.textContent === 'Design review' && $('.day-notes[data-event]'));
  check('A click on an event shows it at the right with its time, and opens no panel', bare && /^1 PM/.test($('.day-notes-time').textContent) && !$('.cal-editor') && ev.classList.contains('on'), $('.day-notes-time')?.textContent);
  snap('day-bare');
  $('#day-notes-start').click();
  const page = await until(() => $('.day-notes-page .title-input')?.value === 'Design review' && $('.day-notes-page'), 4000);
  const line = await until(() => document.activeElement?.classList.contains('blk') && page?.contains(document.activeElement) && document.activeElement, 3000);
  check('"Write the notes here" starts the page of the event in place, with the caret in it', page && line && $$('.tab').length === 1 && $('.calendar'), `${document.activeElement?.className} tabs ${$$('.tab').length}`);
  if (line) typeIn(line, 'Two versions of the price table');
  check('What is typed there is in the page of the event', await until(() => $$('.blk', page).some((b) => b.textContent.includes('Two versions of the price table'))) && treeItem('Design review'));
  await wait(200);
  snap('day');

  // The notes beside the day are a page like any other: "@", a typed
  // shortcut, undo and the copy of the page all work there.
  if (line) {
    const bare = (el) => el.textContent.replace(/​/g, '');
    key(line, 'Enter');
    const second = await until(() => document.activeElement !== line && document.activeElement?.classList.contains('blk') && page.contains(document.activeElement) && document.activeElement, 3000);
    typeIn(second, 'See @Website');
    const list = await until(() => $('.slash-menu.page-links'), 3000);
    check('Beside the day, "@" and a name lists the pages that fit', list && $$('.page-links .menu-item .link-title').map((t) => t.textContent).join('|') === 'Website plan', $$('.page-links .menu-item .link-title').map((t) => t.textContent).join('|'));
    key(second, 'Enter');
    const made = await until(() => $('a[href^="notion2://page/"]', second), 3000);
    check('…and Return puts the link in place of what was typed', made && made.textContent === 'Website plan' && bare(second).startsWith('See Website plan') && !bare(second).includes('@') && !$('.slash-menu.page-links') && document.activeElement === second, bare(second));
    T().command('undo');
    check('Undo there takes the link away again', await until(() => !$('a[href^="notion2://page/"]', page), 3000), $$('.blk', page).map((b) => bare(b)).join('|'));
    T().command('redo');
    check('…and Redo brings it back', await until(() => $('.day-notes-page a[href^="notion2://page/"]')?.textContent === 'Website plan', 3000));
    delete document.body.dataset.copied;
    T().command('copy_markdown');
    check('"Copy as Markdown" copies the notes beside the day', await until(() => (document.body.dataset.copied || '').includes('Two versions of the price table'), 3000) && document.body.dataset.copied.includes('Website plan'), JSON.stringify(document.body.dataset.copied));
  }

  // The panel for its time and repeats is one click away, and closing it keeps the notes.
  $('#day-details').click();
  const panel = await until(() => $('.cal-editor'));
  check('The clock button opens the panel of the event beside its notes', panel && $('.day-notes-page'));
  $('.cal-editor .icon-btn[title="Close"]').click();
  check('Closing the panel leaves the notes where they are', await until(() => !$('.cal-editor')) && $('.day-notes-page .title-input')?.value === 'Design review');

  // A second event at the same time: the two are side by side in the day.
  const again = col().getBoundingClientRect();
  const same = { bubbles: true, clientX: again.left + 10, clientY: again.top + 13 * 48 + 3 };
  col().dispatchEvent(new MouseEvent('mousedown', same));
  col().dispatchEvent(new MouseEvent('click', same));
  const slot2 = await until(() => $('#ev-draft'));
  slot2.value = 'Budget call';
  slot2.dispatchEvent(new Event('input', { bubbles: true }));
  key(slot2, 'Enter');
  const pair = await until(() => {
    const two = ['Design review', 'Budget call'].map((name) => $$('.cal-event:not(.draft)').find((e) => e.querySelector('.ev-title')?.textContent === name));
    return two.every(Boolean) && two;
  }, 3000);
  const [ra, rb] = (pair || []).map((e) => e.getBoundingClientRect());
  check('Two events at one time are side by side in the day, not one over the other', pair && (ra.right <= rb.left + 1 || rb.right <= ra.left + 1) && Math.abs(ra.top - rb.top) < 2 && ra.width > 40 && rb.width > 40, pair && `${Math.round(ra.left)}–${Math.round(ra.right)} and ${Math.round(rb.left)}–${Math.round(rb.right)}`);

  // The arrows go a day at a time; a to-do shows in the day it is due.
  for (let i = 1; i <= 3; i++) {
    const was = $('.cal-title').textContent;
    $('.cal-nav .icon-btn[title="Next"]').click();
    await until(() => $('.cal-title').textContent !== was);
  }
  check('The arrows go one day at a time, and a to-do shows in the day it is due', $('.cal-title').textContent.startsWith(dayFrom(3).toLocaleDateString('en-US', { weekday: 'long' })) && (await until(() => dueNames() === 'Book the room')) && $('.day-todos-label').textContent.startsWith('To do on '), `${$('.cal-title').textContent}: ${dueNames()}`);
  $('.cal-nav .secondary-btn').click();
  await until(() => dueNames().includes('Send the update list'));
  due().find((r) => r.querySelector('.todo-text').textContent === 'Send the update list').querySelector('.todo-box').click();
  check('Ticked in the day, a to-do is done', await until(() => dueNames() === 'Look at the import story', 3000), dueNames());

  // From the week, a click on a day opens that day.
  seg('Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
  const head = $$('.cal-wday')[2];
  const named = head.getAttribute('aria-label');
  head.click();
  check('In the week, a click on a day opens that day', await until(() => $('.cal-week.one-day')) && named.startsWith(`Open ${$('.cal-title').firstChild.textContent.split(' ')[0]}`), `${named} / ${$('.cal-title').textContent}`);
  seg('Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
  $('.cal-nav .secondary-btn').click();

  // On Home, the two at one time are on lines of their own.
  await oneTab();
  const onLine = (name) => $$('#day-line .day-event').find((e) => e.title.startsWith(name));
  const da = await until(() => onLine('Design review'), 3000);
  const db = onLine('Budget call');
  const boxOf = (e, part) => e.querySelector(part).getBoundingClientRect();
  const clear = (p, q) => p.right <= q.left + 0.5 || q.right <= p.left + 0.5 || p.bottom <= q.top + 0.5 || q.bottom <= p.top + 0.5;
  check('On Home, the day stays one line: two events at one time have their bars on it', da && db && $$('#day-line .day-rail').length === 1 && Math.abs(boxOf(da, '.day-bar').top - boxOf(db, '.day-bar').top) < 0.5 && Math.abs(boxOf(da, '.day-bar').top - $('.day-rail').getBoundingClientRect().top) < 0.5 && $('.day-track').getBoundingClientRect().height <= 84, da && db && `rails ${$$('#day-line .day-rail').length}, height ${$('.day-track').getBoundingClientRect().height}`);
  check('…and their names are one under the other, each with its time on its line', da && db && da.dataset.row !== db.dataset.row && da.classList.contains('together') && db.classList.contains('together') && clear(boxOf(da, '.day-name'), boxOf(db, '.day-name')) && clear(boxOf(da, '.day-when'), boxOf(db, '.day-name')) && clear(boxOf(db, '.day-when'), boxOf(da, '.day-name')) && Math.abs(boxOf(da, '.day-name').left - boxOf(db, '.day-name').left) < 1 && boxOf(da, '.day-when').top < boxOf(da, '.day-name').bottom, da && db && `rows ${da.dataset.row} and ${db.dataset.row}`);
  const every = $$('#day-line .day-event:not(.unnamed)');
  let over = '';
  for (const a of every) for (const b of every) if (a !== b && !clear(boxOf(a, '.day-name'), boxOf(b, '.day-name'))) over = `${a.title} / ${b.title}`;
  check('No name on the line of the day lies over another', !over, over);
  check('The "now" mark and the hours stay with the line', $('.day-hours').getBoundingClientRect().top >= $('.day-track').getBoundingClientRect().bottom - 1);
  await wait(120);
  snap('home-clash');
});

await run('A page with character', async () => {
  await oneTab();
  const title = await newPage('Field notes');
  const pane = () => shownPane('Field notes');
  const page = () => $('.page', pane());
  check('A page with nothing written has no line under its title', page() && !$('.page-meta', pane()));
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(first, 'The first paragraph says what the page is for.');
  const meta = await until(() => $('.page-meta', pane()));
  check('Once something is written, a line under the title says when the page was changed', meta && /^Edited today, \d/.test($('.meta-edited', meta).textContent) && !$('.meta-read', meta) && !$('.meta-links', meta), meta?.textContent);

  // A new line under the line with the caret: with a shortcut for its kind, then its words.
  const line = async (shortcut, text) => {
    const a = document.activeElement;
    T().setSel(a, a.textContent.replace(/​/g, '').length);
    key(a, 'Enter');
    let b = await until(() => document.activeElement !== a && document.activeElement?.classList.contains('blk') && document.activeElement);
    if (!b) throw new Error(`no new line after "${a.textContent.slice(0, 30)}" (active: ${document.activeElement?.className})`);
    if (shortcut) {
      const was = b.dataset.kind;
      typeIn(b, shortcut);
      b = await until(() => {
        const x = document.activeElement;
        return x?.classList.contains('blk') && x.dataset.kind !== was && x;
      });
      if (!b) throw new Error(`"${shortcut}" did not change the line (active: ${document.activeElement?.className} ${document.activeElement?.dataset?.kind})`);
    }
    typeIn(b, text);
    await wait(40);
    return b;
  };
  const h1 = await line('## ', 'What we decided');
  const plain = await line('', 'One line at the top, no slider.');
  for (let i = 1; i <= 26; i++) await line('', `A line to make the page long, number ${i}.`);
  const h2 = await line('## ', 'Open questions');
  await line('', 'Who has the pictures?');
  const quote = await line('" ', 'If a visitor reads only the first line, that line must be enough.');
  check('The lines for the check are there', h1.dataset.kind === 'heading2' && h2.dataset.kind === 'heading2' && quote.dataset.kind === 'quote' && plain.dataset.kind === 'text', `${h1.dataset.kind} ${h2.dataset.kind} ${quote.dataset.kind}`);

  // The face of the title and the headings.
  const face = (f) => $(`.face[data-face="${f}"]`, pane());
  const fontOf = (el) => getComputedStyle(el).fontFamily;
  const titleBox = () => $('.title-input', pane());
  check('Three faces for the title are with the quiet controls over it, the plain one chosen', $$('.face', pane()).length === 3 && face('').classList.contains('on') && face('').getAttribute('aria-pressed') === 'true');
  const sans = fontOf(titleBox());
  const quoteBox = () => quote.closest('.block-inner');
  check('On a plain page a quote has its line at the left', getComputedStyle(quoteBox()).borderLeftWidth !== '0px' && getComputedStyle(quote).fontStyle === 'normal');
  face('serif').click();
  check('"Serif" gives the title and the headings a serif face', (await until(() => page().classList.contains('font-serif') && face('serif').classList.contains('on'))) && /serif/i.test(fontOf(titleBox())) && fontOf(titleBox()) !== sans && fontOf(h1) === fontOf(titleBox()) && fontOf(plain) === sans, `${fontOf(titleBox())} | ${fontOf(h1)} | ${fontOf(plain)}`);
  check('…its first paragraph is a little larger than the others', parseFloat(getComputedStyle(first).fontSize) > parseFloat(getComputedStyle(plain).fontSize) + 1, `${getComputedStyle(first).fontSize} and ${getComputedStyle(plain).fontSize}`);
  check('…and a quote is set large, in italics, with a rule over it', getComputedStyle(quote).fontStyle === 'italic' && getComputedStyle(quoteBox()).borderLeftWidth === '0px' && getComputedStyle(quoteBox()).borderTopWidth === '1px' && parseFloat(getComputedStyle(quote).fontSize) > parseFloat(getComputedStyle(plain).fontSize) + 4, `${getComputedStyle(quote).fontStyle} ${getComputedStyle(quote).fontSize}`);
  await wait(150);
  snap('page-serif');
  face('mono').click();
  check('"Mono" gives them a typewriter face, and the quote is plain again', (await until(() => page().classList.contains('font-mono') && !page().classList.contains('font-serif'))) && /mono|menlo/i.test(fontOf(titleBox())) && fontOf(h2) === fontOf(titleBox()) && getComputedStyle(quote).fontStyle === 'normal', fontOf(titleBox()));
  face('serif').click();
  await until(() => page().classList.contains('font-serif'));

  // The outline at the right edge.
  const outline = () => $('.page-outline', pane());
  const items = () => $$('.outline-item', outline() || document.body);
  check('Two headings or more give the page an outline at its right edge', (await until(() => items().length === 2)) && items().map((i) => i.title).join('|') === 'What we decided|Open questions', items().map((i) => i.title).join('|'));
  const list = $('.page-outline-list', outline());
  const edge = pane().getBoundingClientRect().right;
  check('It is short lines at the edge, clear of the text; the names are not shown', list && getComputedStyle($('.outline-name', list)).display === 'none' && edge - list.getBoundingClientRect().right < 24 && list.getBoundingClientRect().left > $('.blocks', pane()).getBoundingClientRect().right, list && `${Math.round(list.getBoundingClientRect().left)} after ${Math.round($('.blocks', pane()).getBoundingClientRect().right)}`);
  const scroller = pane().querySelector('.scroller');
  scroller.scrollTop = 0;
  await wait(120);
  items()[1].click();
  const there = await until(() => {
    const top = h2.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
    // At the top of the view, or as far up as the end of the page lets it come.
    const atEnd = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2;
    return scroller.scrollTop > 100 && top > 40 && (top < 110 || atEnd);
  }, 4000);
  check('A click on a line goes to its heading', there, `scrolled ${Math.round(scroller.scrollTop)}, heading at ${Math.round(h2.getBoundingClientRect().top - scroller.getBoundingClientRect().top)}`);
  check('…and the heading the reader is in is marked', await until(() => items()[1].classList.contains('on') && !items()[0].classList.contains('on'), 2000));
  await wait(150);
  snap('page-outline');

  // A picture and its caption.
  const end = await line('', '');
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), (c) => c.charCodeAt(0));
  const clip = new DataTransfer();
  clip.items.add(new File([png], 'Screenshot.png', { type: 'image/png' }));
  end.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const box = await until(() => $('.image-block[data-image]', pane()), 4000);
  check('A picture has no caption line until one is asked for', box && !$('.image-caption', pane()));
  $('[data-image-op="caption"]', box).click();
  const cap = await until(() => document.activeElement?.classList.contains('image-caption') && document.activeElement, 3000);
  check('"Add a caption" puts a line under the picture, with the caret in it', cap && cap.getBoundingClientRect().top >= box.getBoundingClientRect().bottom - 1 && cap.closest('.block-row') === box.closest('.block-row'));
  if (cap) {
    cap.value = 'The first screen, on a phone';
    cap.dispatchEvent(new Event('input', { bubbles: true }));
    key(cap, 'Enter');
  }
  check('The caption stays under the picture, and its button goes', (await until(() => document.activeElement !== cap && $('.image-caption', pane())?.value === 'The first screen, on a phone')) && !$('[data-image-op="caption"]', pane()), $('.image-caption', pane())?.value);
  delete document.body.dataset.copied;
  T().command('copy_markdown');
  const copied = await until(() => document.body.dataset.copied);
  check('The caption goes out with the page as text', copied && copied.includes('[Image: The first screen, on a phone]'), copied?.slice(-160));
  // Emptied, the line goes away again.
  const again = $('.image-caption', pane());
  again.focus();
  again.value = '';
  again.dispatchEvent(new Event('input', { bubbles: true }));
  again.blur();
  check('An emptied caption takes its line away', await until(() => !$('.image-caption', pane()) && $('[data-image-op="caption"]', pane())));

  // The face is kept with the page.
  await oneTab();
  treeItem('Field notes').click();
  check('The page keeps its face', await until(() => shownPane('Field notes') && $('.page.font-serif', shownPane('Field notes'))));
});

await run('The graph of pages', async () => {
  await oneTab();
  // One pane, the full width (an earlier check can leave pages side by side).
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  // Two pages with a link from one to the other, and a page inside one of them.
  await newPage('Graph B');
  const title = await newPage('Graph A');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  typeIn(first, 'See @Graph B');
  await until(() => $('.slash-menu.page-links'), 3000);
  key(first, 'Enter');
  await until(() => $('a[href^="notion2://page/"]', first), 3000);
  await oneTab();

  const tile = $$('.tile').find((t) => t.title.startsWith('Graph'));
  check('A third button beside Home and Calendar opens the graph', tile && $$('.tile').length === 3 && $$('.tile').every((t) => t.getBoundingClientRect().width >= 36), $$('.tile').map((t) => Math.round(t.getBoundingClientRect().width)).join(' '));
  tile.click();
  const el = await until(() => $('.graph[data-graph]'));
  const g = await until(() => el?._graph?.fitted && el._graph.nodes.length && el._graph, 4000);
  check('The graph opens in the pane, and its button shows as the current one', el && g && tile.classList.contains('current') && $('.pagebar .crumb.current')?.textContent.includes('Graph'));
  const data = JSON.parse(el.dataset.graph);
  const at = (name) => data.nodes.findIndex((n) => n.t === name);
  const idOf = (name) => data.nodes[at(name)]?.id;
  const [a, b] = [at('Graph A'), at('Graph B')];
  check('Each page of the space is a dot', a >= 0 && b >= 0 && at('Getting Started') >= 0 && g.nodes.length === data.nodes.length && data.nodes.length === new Set(data.nodes.map((n) => n.id)).size, `${data.nodes.length} dots`);
  check('A link from one page to another is a line between their dots', data.edges.some((e) => e[0] === Math.min(a, b) && e[1] === Math.max(a, b) && e[2] === 0), JSON.stringify(data.edges));
  const links = data.edges.filter((e) => e[2] === 0).length;
  const nested = data.edges.filter((e) => e[2] === 1).length;
  const counted = `${data.nodes.length} pages${nested ? ` · ${nested} ${nested === 1 ? 'sub-page' : 'sub-pages'}` : ''} · ${links} ${links === 1 ? 'link' : 'links'}`;
  check('The corner says how many pages, sub-pages and links there are', $('.graph-count').textContent === counted, `${$('.graph-count').textContent} / ${counted}`);
  check('Every dot has a colour: its icon’s, or its group’s', data.nodes.every((n) => n.c), data.nodes.filter((n) => !n.c).map((n) => n.t).join(', '));
  check('The same is there as words for a screen reader', $$('.graph-words li').some((li) => li.textContent === 'Graph A: joined to Graph B') && $('.graph-canvas').getAttribute('role') === 'img');

  // It comes to rest, in view, with no two dots in one place.
  check('The dots come to rest', await until(() => g.alpha <= 0.02, 8000), String(g.alpha));
  const box = el.getBoundingClientRect();
  const spot = (name) => g.screen(idOf(name));
  const inside = (p) => p && p.x > box.left + 8 && p.x < box.right - 8 && p.y > box.top + 8 && p.y < box.bottom - 8;
  const apart = (p, q) => Math.hypot(p.x - q.x, p.y - q.y);
  const spots = data.nodes.map((n) => g.screen(n.id));
  let closest = Infinity;
  for (let i = 0; i < spots.length; i++) for (let j = i + 1; j < spots.length; j++) closest = Math.min(closest, apart(spots[i], spots[j]));
  check('All of them are in view, no two in one place, the linked two near each other', spots.every(inside) && closest > 5 && apart(spot('Graph A'), spot('Graph B')) < 220 && $$('.pane').length === 1, `closest ${Math.round(closest)}, A–B ${Math.round(apart(spot('Graph A'), spot('Graph B')))}`);
  const canvas = $('.graph-canvas');
  const dpr = window.devicePixelRatio || 1;
  const inkAt = (p) => canvas.getContext('2d').getImageData(Math.round((p.x - box.left) * dpr), Math.round((p.y - box.top) * dpr), 1, 1).data[3];
  check('The dots are drawn', inkAt(spot('Graph A')) > 100 && inkAt(spot('Graph B')) > 100, `${inkAt(spot('Graph A'))} ${inkAt(spot('Graph B'))}`);
  // A picture of the canvas for the snapshot (a snapshot keeps no canvas).
  const still = new Image();
  still.src = canvas.toDataURL();
  still.style.cssText = 'position:absolute;inset:0;width:100%;height:100%';
  el.appendChild(still);
  await still.decode();
  snap('graph');
  still.remove();

  // The pointer on a dot picks it and the dots joined to it.
  const move = (p, type = 'mousemove', target = canvas) => target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: p.x, clientY: p.y }));
  move(spot('Graph A'));
  check('The pointer on a dot picks it', g.hover === a && canvas.style.cursor === 'pointer', `${g.hover} for ${a}`);
  move({ x: box.left + 3, y: box.bottom - 3 });
  check('…and off it, nothing is picked', g.hover === -1);

  // Find.
  const find = $('#graph-find');
  find.value = 'graph b';
  find.dispatchEvent(new Event('input', { bubbles: true }));
  check('"Find a page" marks the pages with those letters', g.find === 'graph b');
  find.value = '';
  find.dispatchEvent(new Event('input', { bubbles: true }));

  // Zoom: what is under the pointer stays under it.
  const before = spot('Graph A');
  const k0 = g.view.k;
  canvas.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -160, clientX: before.x, clientY: before.y }));
  check('A scroll zooms, and what is under the pointer stays under it', g.view.k > k0 * 1.2 && apart(spot('Graph A'), before) < 1.5, `${k0.toFixed(2)} to ${g.view.k.toFixed(2)}, moved ${apart(spot('Graph A'), before).toFixed(1)}`);

  // Drag the background: the picture moves. Drag a dot: the dot moves.
  const bSpot = spot('Graph B');
  const empty = { x: box.left + 6, y: box.top + 60 };
  move(empty, 'mousedown');
  move({ x: empty.x + 40, y: empty.y + 30 }, 'mousemove', window);
  move({ x: empty.x + 40, y: empty.y + 30 }, 'mouseup', window);
  check('A drag on the background moves the picture', apart(spot('Graph B'), { x: bSpot.x + 40, y: bSpot.y + 30 }) < 2, `${Math.round(spot('Graph B').x - bSpot.x)}, ${Math.round(spot('Graph B').y - bSpot.y)}`);
  const from = spot('Graph A');
  move(from, 'mousedown');
  move({ x: from.x + 70, y: from.y + 50 }, 'mousemove', window);
  check('A drag on a dot moves that dot', apart(spot('Graph A'), { x: from.x + 70, y: from.y + 50 }) < 2, `${Math.round(spot('Graph A').x - from.x)}, ${Math.round(spot('Graph A').y - from.y)}`);
  move({ x: from.x + 70, y: from.y + 50 }, 'mouseup', window);
  await wait(200);
  check('…and opens no page', $('.graph') && !shownPane('Graph A'));

  // A click on a dot opens its page.
  await until(() => g.alpha <= 0.02, 8000);
  const target = spot('Graph B');
  move(target, 'mousedown');
  move(target, 'mouseup', window);
  check('A click on a dot opens its page', await until(() => shownPane('Graph B'), 3000));

  // A page made now is a dot when the graph is looked at again.
  await newPage('Graph C');
  await oneTab();
  $$('.tile').find((t) => t.title.startsWith('Graph')).click();
  const again = await until(() => $('.graph[data-graph]')?._graph?.fitted && $('.graph[data-graph]'));
  check('A new page is a dot the next time', again && JSON.parse(again.dataset.graph).nodes.some((n) => n.t === 'Graph C') && again._graph.nodes.length === data.nodes.length + 1);
  await oneTab();
  // The graph has one tab at most: opening it again from another tab goes to it.
  $$('.tile').find((t) => t.title.startsWith('Graph')).click();
  await until(() => $('.tab.active .tab-title')?.textContent === 'Graph', 3000);
  treeItem('Graph A').click();
  await until(() => $('.tab.active .tab-title')?.textContent === 'Graph A', 3000);
  $$('.tile').find((t) => t.title.startsWith('Graph')).click();
  const titles = () => $$('.tab .tab-title').map((t) => t.textContent);
  check('Opening the graph again goes to its tab, and the page keeps its own', await until(() => $('.tab.active .tab-title')?.textContent === 'Graph' && titles().filter((t) => t === 'Graph').length === 1 && titles().includes('Graph A'), 3000), titles().join(' | '));
  await oneTab();
});

await run('A web address is a link', async () => {
  await oneTab();
  const title = await newPage('Web links');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const text = (el) => el.textContent.replace(/​/g, '');
  typeIn(first, 'See https://example.com/a. ');
  const typed = await until(() => $('a[href="https://example.com/a"]', first), 2000);
  check('A web address becomes a link when a space is typed after it, without the full stop', typed && typed.textContent === 'https://example.com/a' && text(first) === 'See https://example.com/a. ', first.innerHTML);
  check('A link to the web is blue', typed && getComputedStyle(typed).color !== getComputedStyle(first).color && getComputedStyle(typed).textDecorationLine.includes('underline'), typed && getComputedStyle(typed).color);
  typeIn(first, 'more');
  check('Typing goes on after the link, outside it', $('a[href="https://example.com/a"]', first)?.textContent === 'https://example.com/a' && text(first).endsWith('. more'), first.innerHTML);

  // Pasted by itself, an address goes in as a link.
  T().setSel(first, text(first).length);
  key(first, 'Enter');
  const second = await until(() => document.activeElement !== first && document.activeElement?.classList.contains('blk') && document.activeElement);
  const clip = new DataTransfer();
  clip.setData('text/plain', 'https://example.org/page?x=1');
  second.dispatchEvent(new ClipboardEvent('paste', { clipboardData: clip, bubbles: true, cancelable: true }));
  const pasted = await until(() => $('a[href="https://example.org/page?x=1"]', second), 2000);
  check('A pasted web address is a link', pasted && text(second) === 'https://example.org/page?x=1', second.innerHTML);
  typeIn(second, ' and words');
  check('…and what is typed after it is not part of the link', pasted && $('a', second).textContent === 'https://example.org/page?x=1' && text(second).endsWith(' and words'), second.innerHTML);
  delete document.body.dataset.copied;
  T().command('copy_markdown');
  const copied = await until(() => document.body.dataset.copied);
  check('The page keeps them as links', copied && copied.includes('[https://example.com/a](https://example.com/a)') && copied.includes('[https://example.org/page?x=1](https://example.org/page?x=1)'), copied);

  // The page menu in the sidebar: one line for each of its lines.
  const row = treeItem('Web links');
  row.querySelector('.tree-actions .row-btn').click();
  const menu = await until(() => $('.sidebar-menu'), 2000);
  if (menu) {
    await wait(260);
    const item = menuItem(menu, 'Open in split view');
    const label = item.querySelector('span:not(.menu-icon):not(.menu-shortcut)');
    const hint = item.querySelector('.menu-shortcut');
    const lineHeight = parseFloat(getComputedStyle(label).lineHeight) || 22;
    check('In the page menu of the sidebar, a line and its hint do not break in two', label.getBoundingClientRect().height < lineHeight * 1.6 && hint.getBoundingClientRect().height < lineHeight * 1.6 && label.scrollWidth <= label.clientWidth + 1, `${Math.round(label.getBoundingClientRect().height)} and ${Math.round(hint.getBoundingClientRect().height)}`);
    key(document.body, 'Escape');
    $('.backdrop')?.click();
    await until(() => !$('.sidebar-menu'));
  } else {
    check('In the page menu of the sidebar, a line and its hint do not break in two', false, 'the menu did not open');
  }
});

// Only when asked for (NOTION2_REAL_VIDEO=1): plays a real video from YouTube,
// without sound, to prove that YouTube accepts the app's player.
if (window.__n2_real_video) {
  await run('Real YouTube probe', async () => {
    await oneTab();
    const title = await newPage('Real video');
    key(title, 'Enter');
    const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
    typeIn(first, 'https://youtu.be/jNQXAC9IVRw');
    const card = await until(() => $('.video-embed', shownPane('Real video')), 3000);
    card.click();
    const dock = await until(() => $('#player-dock'), 3000);
    const end = await until(() => dock.dataset.ready === 'true' || $('.player-problem', dock), 25000, 100);
    check('YouTube accepts the player of the app, and the video comes up', dock.dataset.ready === 'true' && !$('.player-problem', dock), $('.player-problem', dock)?.textContent || `ready: ${dock.dataset.ready}`);
    await wait(2500);
    check('…and it is still up a moment later', $('#player-dock')?.dataset.ready === 'true' && !$('.player-problem'), $('.player-problem')?.textContent || '');
    $('#player-close')?.click();
    await until(() => !$('#player-dock'), 2000);
  });
}

await run('The calendars of this Mac are an option', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  const closeSettings = async () => {
    key(document.body, 'Escape');
    await until(() => !$('.settings-view'));
  };
  const onHome = (name) => $$('#day-line .day-event, #day-line .day-all-item').some((e) => (e.title || e.textContent).startsWith(name));
  check('Before the option is on, Home shows no event of the Mac', !onHome('Budget review') && !onHome('Sam on leave'));

  await openSettings();
  check('Settings has a part for the calendar', await settingsSection('Calendar'));
  const main = () => $('#mac-calendars-switch');
  check('The calendars of this Mac are off until they are turned on', main() && main().getAttribute('aria-checked') === 'false' && !$('[data-calendar]'));
  await wait(300);
  snap('settings-calendar-off');
  main().click();
  const listed = await until(() => $$('[data-calendar]').length === 2 && $$('[data-calendar]'), 4000);
  check('Turned on, it lists the calendars, each with its account and a switch of its own', listed && listed.map((r) => r.dataset.calendar).join('|') === 'Work|Home' && listed.every((r) => r.querySelector('.switch.on') && r.querySelector('.set-dot')) && listed[0].textContent.includes('Google'), listed && listed.map((r) => r.textContent).join(' | '));
  await wait(300);
  snap('settings-calendar');
  for (const part of ['Appearance', 'Space', 'AI', 'Notifications', 'Data']) {
    await settingsSection(part);
    await wait(280);
    snap('settings-' + part.toLowerCase());
  }
  await closeSettings();

  check('Their events are on Home with the app’s own: on the line of the day, and all day', await until(() => onHome('Budget review') && onHome('Sam on leave'), 4000));

  // In the calendar: in the colour of its calendar, and read only.
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar'));
  const seg = (name) => $$('.cal-toolbar .segmented button').find((b) => b.textContent === name);
  seg('Day').click();
  await until(() => $('.cal-week.one-day'));
  $('.cal-nav .secondary-btn').click();
  const ev = await until(() => $$('.cal-event').find((e) => e.querySelector('.ev-title')?.textContent === 'Budget review'), 4000);
  check('An event of the Mac is in the day, in the colour of its calendar', ev && ev.getAttribute('style').includes('#b9a07f') && $$('.cal-chip').some((c) => c.textContent.includes('Sam on leave')), ev?.getAttribute('style'));
  ev.click();
  await until(() => $('.day-notes-title')?.textContent === 'Budget review');
  $('#day-details').click();
  const panel = await until(() => $('.cal-editor.of-the-mac'));
  check('Its panel only shows it: the calendar it is from, the place, and nothing to change', panel && !$('input, select, textarea', panel) && panel.textContent.includes('Work · Google') && panel.textContent.includes('Room 2') && panel.textContent.includes('Calendar app') && $('#ev-join', panel) && !panel.textContent.includes('https://'), panel?.textContent);
  await wait(250);
  snap('mac-event');
  $('.icon-btn[title="Close"]', panel).click();
  await until(() => !$('.cal-editor'));
  $('#day-notes-start').click();
  const page = await until(() => $('.day-notes-page .title-input')?.value === 'Budget review' && $('.day-notes-page'), 4000);
  check('A notes page can be made for it, in place', page && treeItem('Budget review') && $$('.tab').length === 1);
  seg('Week').click();
  await until(() => $('.cal-week:not(.one-day)'));

  // One calendar hidden: its events go; the other stays.
  await openSettings();
  await settingsSection('Calendar');
  $('[data-calendar="Work"] .switch').click();
  await until(() => !$('[data-calendar="Work"] .switch.on'));
  await closeSettings();
  await oneTab();
  check('A calendar that is switched off shows no events; the other one still does', await until(() => !onHome('Budget review') && onHome('Sam on leave'), 3000));

  // The option off again: none of them.
  await openSettings();
  await settingsSection('Calendar');
  main().click();
  check('Turned off, the list goes', await until(() => main().getAttribute('aria-checked') === 'false' && !$('[data-calendar]'), 3000));
  await closeSettings();
  check('…and so do the events', await until(() => !onHome('Sam on leave') && !onHome('Budget review'), 3000));
});

await run('Drag an event in the calendar', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar'));
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
  $('.cal-nav .secondary-btn').click();
  const cols = () => $$('.cal-col');
  // An event at 10:00 in the third day of the week.
  const day = cols()[2].getBoundingClientRect();
  const at = { bubbles: true, clientX: day.left + 10, clientY: day.top + 10 * 48 + 3 };
  cols()[2].dispatchEvent(new MouseEvent('mousedown', at));
  cols()[2].dispatchEvent(new MouseEvent('mouseup', at));
  cols()[2].dispatchEvent(new MouseEvent('click', at));
  const slot = await until(() => $('#ev-draft'));
  slot.value = 'Move me';
  slot.dispatchEvent(new Event('input', { bubbles: true }));
  key(slot, 'Enter');
  const ev = () => $$('.cal-event[data-event]').find((e) => e.querySelector('.ev-title')?.textContent === 'Move me');
  await until(ev);
  const where = () => ({ col: cols().indexOf(ev().closest('.cal-col')), top: parseFloat(ev().style.top), height: parseFloat(ev().style.height) });
  const start = where();
  check('The event starts at 10 in the third day', start.col === 2 && start.top === 480, JSON.stringify(start));
  const press = (type, x, y, target) => target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, buttons: type === 'mouseup' ? 0 : 1, clientX: x, clientY: y }));

  // One day to the right and one hour down.
  let box = ev().getBoundingClientRect();
  let from = { x: box.left + box.width / 2, y: box.top + 5 };
  press('mousedown', from.x, from.y, ev());
  press('mousemove', from.x + day.width, from.y + 50, window);
  check('While it is dragged, the event is lifted and goes with the pointer in steps of a quarter hour', ev().classList.contains('dragging') && ev().style.transform.replace(/\s/g, '') === `translate(${day.width}px,48px)`, ev().style.transform);
  press('mouseup', from.x + day.width, from.y + 50, window);
  check('Let go, it is on the next day, one hour later', await until(() => where().col === 3 && where().top === 528, 3000), JSON.stringify(where()));
  await wait(450);
  check('…in its new place with no lift left on it, and no panel opened', !ev().style.transform && !ev().classList.contains('dragging') && !$('.cal-editor'), ev().style.transform);

  // Its lower edge pulled down: longer.
  box = ev().getBoundingClientRect();
  from = { x: box.left + box.width / 2, y: box.bottom - 3 };
  ev().dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: from.x, clientY: from.y }));
  check('The pointer on its lower edge shows that the edge can be pulled', ev().classList.contains('at-edge'));
  press('mousedown', from.x, from.y, ev());
  press('mousemove', from.x, from.y + 36, window);
  press('mouseup', from.x, from.y + 36, window);
  check('The lower edge pulled down by three quarters makes it an hour long, at the same start', await until(() => where().height === 48 && where().top === 528 && where().col === 3, 3000), JSON.stringify(where()));

  // A small slip of the hand is a click: it picks the event and moves nothing.
  box = ev().getBoundingClientRect();
  press('mousedown', box.left + 20, box.top + 8, ev());
  press('mousemove', box.left + 22, box.top + 9, window);
  press('mouseup', box.left + 22, box.top + 9, window);
  ev().click();
  check('A press that does not move is a click: the event is picked, and stays where it is', (await until(() => $('.cal-editor'), 2000)) && where().col === 3 && where().top === 528, JSON.stringify(where()));
  $('.cal-editor .icon-btn[title="Close"]').click();
  await until(() => !$('.cal-editor'));

  // Back one day and up, past the left edge of the week: it stops at the first day.
  box = ev().getBoundingClientRect();
  from = { x: box.left + box.width / 2, y: box.top + 5 };
  press('mousedown', from.x, from.y, ev());
  press('mousemove', from.x - day.width * 9, from.y - 96, window);
  press('mouseup', from.x - day.width * 9, from.y - 96, window);
  check('Dragged past the edge of the week, it stops at the first day', await until(() => where().col === 0 && where().top === 432, 3000), JSON.stringify(where()));

  // A press whose release the page did not see (outside the window) arms nothing.
  box = ev().getBoundingClientRect();
  press('mousedown', box.left + 20, box.top + 6, ev());
  window.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, buttons: 0, clientX: box.left + 300, clientY: box.top + 200 }));
  window.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, buttons: 0, clientX: box.left + 320, clientY: box.top + 240 }));
  await wait(200);
  check('A press that was let go outside the window moves nothing afterwards', !ev().classList.contains('dragging') && !ev().style.transform && where().col === 0 && where().top === 432 && !$('.cal-event.drawing') && !$('#ev-draft'), JSON.stringify(where()));

  // A drag over empty time draws a new event of that length.
  const space = cols()[4].getBoundingClientRect();
  press('mousedown', space.left + 12, space.top + 14 * 48 + 2, cols()[4]);
  press('mousemove', space.left + 12, space.top + 15.5 * 48 - 2, window);
  const ghost = $('.cal-event.drawing', cols()[4]);
  check('While empty time is dragged over, the new event shows in its length', ghost && ghost.style.top === '672px' && ghost.style.height === '72px', ghost && `${ghost.style.top} ${ghost.style.height}`);
  press('mouseup', space.left + 12, space.top + 15.5 * 48 - 2, window);
  const drawn = await until(() => $('#ev-draft'), 3000);
  check('Let go, it is a slot of that length to type the name in', drawn && !$('.cal-event.drawing') && drawn.parentElement.style.top === '672px' && drawn.parentElement.style.height === '72px' && drawn.placeholder.includes('2 PM') && drawn.placeholder.includes('3:30 PM'), drawn && `${drawn.parentElement.style.top} ${drawn.parentElement.style.height} ${drawn.placeholder}`);
  drawn.value = 'Drawn';
  drawn.dispatchEvent(new Event('input', { bubbles: true }));
  key(drawn, 'Enter');
  const made = await until(() => $$('.cal-event[data-event]').find((e) => e.querySelector('.ev-title')?.textContent === 'Drawn'), 3000);
  check('With a name, it is an event of an hour and a half', made && made.style.top === '672px' && made.style.height === '72px' && made.closest('.cal-col') === cols()[4], made && `${made.style.top} ${made.style.height}`);

  // An all-day event is a chip: it goes to another day by a drag.
  const cells = () => $$('.cal-allday-cell');
  cells()[1].dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  const name = await until(() => $('.cal-editor .ed-title'), 3000);
  name.value = 'Day off';
  name.dispatchEvent(new Event('input', { bubbles: true }));
  $('.cal-editor .icon-btn[title="Close"]').click();
  await until(() => !$('.cal-editor'));
  const chip = () => $$('.cal-chip[data-event]').find((c) => c.textContent.includes('Day off'));
  await until(chip);
  const carry = async (el, target) => {
    const a = el.getBoundingClientRect();
    const b = target.getBoundingClientRect();
    press('mousedown', a.left + 8, a.top + 5, el);
    press('mousemove', b.left + b.width / 2, b.top + b.height / 2, window);
    const lifted = !!$('.chip-ghost');
    press('mouseup', b.left + b.width / 2, b.top + b.height / 2, window);
    return lifted;
  };
  const lifted = await carry(chip(), cells()[3]);
  check('An all-day event is carried to another day by a drag', lifted && (await until(() => chip()?.closest('.cal-allday-cell') === cells()[3], 3000)) && !$('.chip-ghost') && !$('.cal-editor'), chip()?.closest('[data-day]')?.dataset.day);

  // In the month too.
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Month').click();
  await until(() => $('.cal-month'));
  const inMonth = () => $$('.cal-month .cal-chip[data-event]').find((c) => c.textContent.includes('Day off'));
  const first = await until(inMonth, 3000);
  const fromDay = first.closest('.cal-day').dataset.day;
  const days = $$('.cal-month .cal-day');
  const target = days[days.indexOf(first.closest('.cal-day')) + 1];
  await carry(first, target);
  check('In the month, an event is carried to the next day the same way', await until(() => inMonth()?.closest('.cal-day')?.dataset.day === target.dataset.day && target.dataset.day !== fromDay, 3000), `${fromDay} to ${inMonth()?.closest('.cal-day')?.dataset.day}`);
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
});

await run('Join a meeting from its event', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  const toCalendar = async (view) => {
    $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
    await until(() => $('.calendar'));
    $$('.cal-toolbar .segmented button').find((b) => b.textContent === view).click();
    await until(() => (view === 'Day' ? $('.cal-week.one-day') : $('.cal-week:not(.one-day)')));
    $('.cal-nav .secondary-btn').click();
    await wait(150);
  };
  await toCalendar('Week');
  // An event today at 4 PM.
  const col = $('.cal-col.today');
  const r = col.getBoundingClientRect();
  const at = { bubbles: true, clientX: r.left + 10, clientY: r.top + 16 * 48 + 3 };
  col.dispatchEvent(new MouseEvent('mousedown', at));
  col.dispatchEvent(new MouseEvent('mouseup', at));
  col.dispatchEvent(new MouseEvent('click', at));
  const slot = await until(() => $('#ev-draft'));
  slot.value = 'Sync call';
  slot.dispatchEvent(new Event('input', { bubbles: true }));
  key(slot, 'Enter');
  const ev = () => $$('.cal-event[data-event]').find((e) => e.querySelector('.ev-title')?.textContent === 'Sync call');
  (await until(ev)).click();
  await until(() => $('.cal-editor #ev-notes'));
  check('An event has a field of its own for the meeting link, empty at first, and no Join button', $('#ev-link') && $('#ev-link').value === '' && !$('#ev-join') && $('#ev-link').getBoundingClientRect().bottom <= $('#ev-notes').getBoundingClientRect().top);
  const link = 'https://us02web.zoom.us/j/8123456789?pwd=abc';
  // The field: a link typed there gives the Join button beside it.
  const field = $('#ev-link');
  field.value = 'teams.example.com/meet/standup';
  field.dispatchEvent(new Event('input', { bubbles: true }));
  const save = await until(() => $('.ed-link-row #ev-link-save'), 3000);
  check('A link typed in that field gives Save beside it, and not Join before it is saved', save && !$('#ev-join'));
  save?.click();
  const beside = await until(() => !$('#ev-link-save') && $('.ed-link-row #ev-join'), 3000);
  delete document.body.dataset.opened;
  beside?.click();
  check('A link in that field gives Join beside it, and Join opens it (with https when it was left out)', beside && (await until(() => document.body.dataset.opened === 'https://teams.example.com/meet/standup', 3000)), document.body.dataset.opened);
  $('#listen-no')?.click();
  await oneTab();
  await toCalendar('Week');
  (await until(ev)).click();
  await until(() => $('.cal-editor #ev-link')?.value === 'teams.example.com/meet/standup');
  check('The link stays in its field, not in the notes', $('#ev-notes').value === '');
  const again = $('#ev-link');
  again.value = '';
  again.dispatchEvent(new Event('input', { bubbles: true }));
  await until(() => $('#ev-link-save'));
  key(again, 'Enter');
  check('Return in the field saves too: an emptied field takes the link away', await until(() => !$('#ev-join') && !$('#ev-link-save'), 3000));
  // An older event has its link in its notes: it still joins, and the field shows it.
  const notes = $('#ev-notes');
  notes.value = `Agenda in the doc.\nJoin: <${link}>`;
  notes.dispatchEvent(new Event('input', { bubbles: true }));
  const join = await until(() => $('#ev-join'), 3000);
  check('With the address of a meeting in its notes, it has one', join);
  delete document.body.dataset.opened;
  join?.click();
  check('Join opens the meeting by that address', await until(() => document.body.dataset.opened === link, 3000), document.body.dataset.opened);
  check('…and the notes page of the event, which asks whether to listen', await until(() => shownPane('Sync call') && $('#listen-start'), 4000) && !$('.listen-bar.on'), $('.listen-bar')?.textContent);

  // In the day view the button is over the notes, and the notes stay at the right of the day.
  $('#listen-no')?.click();
  await oneTab();
  await toCalendar('Day');
  (await until(ev)).click();
  const dayJoin = await until(() => $('#day-join'), 3000);
  const tabs = $$('.tab').length;
  delete document.body.dataset.opened;
  dayJoin?.click();
  check('In the day view, Join is over the notes of the event; the notes stay beside the day', dayJoin && (await until(() => document.body.dataset.opened === link, 3000)) && $('.day-notes-page .title-input')?.value === 'Sync call' && $$('.tab').length === tabs && $('.calendar'), document.body.dataset.opened);
  $('#listen-no')?.click();
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
});

await run('The meeting link of a repeating event', async () => {
  await oneTab();
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar .cal-week'));
  // (The panel of an event from a check before this one may still be open.)
  $('.cal-editor .icon-btn[title="Close"]')?.click();
  await until(() => !$('.cal-editor'));
  $('.cal-toolbar .primary-btn').click();
  key(await until(() => $('#cal-quick')), 'Enter');
  const title = await until(() => $('.cal-editor .ed-title')?.value === '' && $('.cal-editor .ed-title'));
  title.value = 'Daily standup';
  title.dispatchEvent(new Event('input', { bubbles: true }));
  const rule = $('#ev-repeat');
  rule.value = 'weekday';
  rule.dispatchEvent(new Event('change', { bubbles: true }));
  await until(() => $$('.cal-editor .ed-label').some((l) => l.textContent === 'Ends'));
  const type = (text) => {
    const field = $('.cal-editor #ev-link');
    field.value = text;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const chip = () => $$('.cal-event').find((e) => e.querySelector('.ev-title')?.textContent.includes('Daily standup'));
  const shut = async () => {
    $('.cal-editor .icon-btn[title="Close"]').click();
    await until(() => !$('.cal-editor'));
  };
  const open = async () => {
    const found = await until(chip, 3000);
    if (!found) {
      check('The repeating event is on the calendar', false, `events: ${$$('.cal-event').map((e) => e.textContent.trim().slice(0, 40)).join(' | ')} || chips: ${$$('.cal-chip, .ev-chip').length} || view: ${$('.cal-toolbar .segmented .on, .cal-toolbar .segmented [aria-pressed="true"]')?.textContent}`);
      return null;
    }
    found.click();
    return until(() => $('.cal-editor #ev-link'), 3000);
  };
  const first = 'https://teams.example.com/meet/daily';
  type(first);
  const save = await until(() => $('#ev-link-save'), 3000);
  check('A link typed into an event that repeats gives the Save button', save && !$('#ev-join'));
  save?.click();
  check('Save keeps it: the button becomes Join', await until(() => !$('#ev-link-save') && $('.ed-link-row #ev-join'), 3000));
  await shut();
  check('Opened again, the repeating event still has its link, and Join', (await open())?.value === first && $('#ev-join'), $('.cal-editor #ev-link')?.value);
  // A link that still waits for Save is kept when the panel is shut.
  const second = 'https://teams.example.com/meet/daily-2';
  type(second);
  await until(() => $('#ev-link-save'));
  await shut();
  check('A link that was typed and not saved is kept when the panel is shut', (await open())?.value === second && $('#ev-join') && !$('#ev-link-save'), $('.cal-editor #ev-link')?.value);
  delete document.body.dataset.opened;
  $('#ev-join').click();
  check('Join opens the link that is kept', await until(() => document.body.dataset.opened === second, 3000), document.body.dataset.opened);
  $('#listen-no')?.click();
});

await run('To-dos on the calendar', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  const iso = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const dayFrom = (n) => {
    const d = new Date();
    d.setDate(d.getDate() + n);
    return d;
  };
  // Two to-dos: one due today, one in two days.
  const title = await newPage('Plan the week');
  key(title, 'Enter');
  const first = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const pane = () => shownPane('Plan the week');
  const rows = () => $$('.block-row.kind-todo', pane());
  typeIn(first, '[] ');
  await until(() => rows().length === 1);
  typeIn(await until(() => document.activeElement?.classList.contains('blk') && document.activeElement), 'Write the report');
  $('.due-chip', rows()[0]).click();
  menuItem(await until(() => $('.due-menu')), 'Today').click();
  await until(() => !$('.due-menu'));
  const b0 = $('.blk', rows()[0]);
  b0.focus();
  T().setSel(b0, b0.textContent.length);
  key(b0, 'Enter');
  await until(() => rows().length === 2 && document.activeElement !== b0 && document.activeElement?.classList.contains('blk'));
  typeIn(document.activeElement, 'Call the bank');
  $('.due-chip', rows()[1]).click();
  const field = await until(() => $('.due-menu input[type=date]'));
  field.value = iso(dayFrom(2));
  field.dispatchEvent(new Event('change', { bubbles: true }));
  await until(() => !$('.due-menu'));

  const view = async (name) => {
    $$('.cal-toolbar .segmented button').find((b) => b.textContent === name).click();
    await until(() => (name === 'Month' ? $('.cal-month') : name === 'Day' ? $('.cal-week.one-day') : $('.cal-week:not(.one-day)')));
  };
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar'));
  await view('Week');
  $('.cal-nav .secondary-btn').click();
  const cellOf = (d) => $(`.cal-allday-cell[data-day="${iso(d)}"]`);
  const chip = (d, words) => $$('.cal-todo', cellOf(d) || document.createElement('div')).find((c) => c.querySelector('.chip-title').textContent === words);
  check('In the week, a to-do shows on the day it is due, with the all-day events', await until(() => chip(new Date(), 'Write the report'), 3000) && (!cellOf(dayFrom(2)) || chip(dayFrom(2), 'Call the bank')) && !chip(new Date(), 'Call the bank'));

  // The month: how many are due on a day; a click opens that day.
  await view('Month');
  const count = await until(() => $(`.cal-day[data-day="${iso(dayFrom(2))}"] .cal-todo-count`), 3000);
  check('In the month, a day says how many to-dos are due on it', count && /^\d+ to do$/.test(count.textContent) && Number(count.dataset.todos) >= 1 && $(`.cal-day[data-day="${iso(new Date())}"] .cal-todo-count`), count?.textContent);
  count?.click();
  check('…and a click on that opens the day, with them above the hours', (await until(() => $('.cal-week.one-day'), 3000)) && (await until(() => $$('#day-todos .todo-text').some((t) => t.textContent === 'Call the bank'), 3000)), $('.cal-title')?.textContent);

  // A to-do dragged onto a time: half an hour for it there.
  $('.cal-nav .secondary-btn').click();
  const row = await until(() => $$('#day-todos .todo-row').find((r) => r.querySelector('.todo-text').textContent === 'Write the report'), 3000);
  const scroller = $('.cal-scroll');
  scroller.scrollTop = 12 * 48;
  await wait(120);
  const col = $('.cal-week.one-day .cal-col');
  const c = col.getBoundingClientRect();
  const to = { x: c.left + c.width / 2, y: c.top + 15 * 48 + 4 };
  const from = row.querySelector('.todo-text').getBoundingClientRect();
  const press = (type, x, y, target, down = 1) => target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, buttons: down, clientX: x, clientY: y }));
  press('mousedown', from.left + 6, from.top + 6, row.querySelector('.todo-text'));
  press('mousemove', to.x, to.y, window);
  const slot = $('.cal-event.drawing', col);
  check('While a to-do is dragged over the hours, its half hour shows where it will go', $('.todo-ghost')?.textContent === 'Write the report' && slot && slot.style.top === '720px' && slot.style.height === '24px', slot ? `${slot.style.top} ${slot.style.height}` : `under: ${document.elementFromPoint(to.x, to.y)?.className}`);
  press('mouseup', to.x, to.y, window, 0);
  const planned = await until(() => $$('.cal-event[data-event]').find((e) => e.querySelector('.ev-title')?.textContent === 'Write the report'), 3000);
  check('Let go, there is an event for it at that time, half an hour long', planned && planned.style.top === '720px' && planned.style.height === '24px' && !$('.todo-ghost') && !$('.cal-event.drawing'), planned && `${planned.style.top} ${planned.style.height}`);
  check('…and the to-do itself is still to do, on that day', $$('#day-todos .todo-text').some((t) => t.textContent === 'Write the report'));

  // Ticked in the week, it is done.
  await view('Week');
  const tick = await until(() => chip(new Date(), 'Write the report'), 3000);
  tick.querySelector('.todo-box').click();
  check('Ticked in the week, a to-do is done and goes from the day', await until(() => !chip(new Date(), 'Write the report'), 3000));
});

await run('An event typed in words', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
  await until(() => $('.calendar'));
  $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
  await until(() => $('.cal-week:not(.one-day)'));
  $('.cal-nav .secondary-btn').click();
  const iso = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  const type = async (text) => {
    $('.cal-toolbar .primary-btn').click();
    const box = await until(() => document.activeElement?.id === 'cal-quick' && document.activeElement, 3000);
    if (!box) return null;
    box.value = text;
    box.dispatchEvent(new Event('input', { bubbles: true }));
    key(box, 'Enter');
    await until(() => !$('#cal-quick'));
    return box;
  };
  const box = await type('Lunch with Sam tomorrow 1pm for 90 min');
  check('"New event" opens a box to type the event in words, with the caret in it', box);
  const view = (name) => $$('.cal-toolbar .segmented button').find((b) => b.textContent === name).click();
  view('Day');
  const lunch = await until(() => $('.cal-week.one-day') && $$('.cal-event[data-event]').find((e) => e.querySelector('.ev-title')?.textContent === 'Lunch with Sam'), 4000);
  check('The words give the event its name, its day and its time, and the calendar goes to that day', lunch && lunch.closest('.cal-col').dataset.day === iso(tomorrow) && lunch.style.top === '624px' && lunch.style.height === '72px', lunch ? `${lunch.closest('.cal-col').dataset.day} ${lunch.style.top} ${lunch.style.height}` : $('.cal-title')?.textContent);
  check('…and no panel opens', !$('.cal-editor'));

  // A day and no time: all day.
  await type('Trip to Lisbon tomorrow');
  check('A day with no time makes an all-day event', await until(() => $$('.cal-chip.all-day').some((c) => c.textContent.includes('Trip to Lisbon')), 3000));

  // Words with no day and no time: a new event of that name, with its panel open.
  await type('Think about the offsite');
  const panel = await until(() => $('.cal-editor .ed-title'), 3000);
  check('Words with no day and no time make a new event of that name, with its panel open', panel && panel.value === 'Think about the offsite', panel?.value);
  $('.cal-editor .icon-btn[title="Close"]')?.click();
  await until(() => !$('.cal-editor'));

  // Escape puts the box away and makes nothing.
  const before = $$('.cal-event[data-event]').length;
  $('.cal-toolbar .primary-btn').click();
  const again = await until(() => $('#cal-quick'));
  again.value = 'Not this 5pm';
  again.dispatchEvent(new Event('input', { bubbles: true }));
  key(again, 'Escape');
  check('Escape puts the box away and makes nothing', (await until(() => !$('#cal-quick') && $('.cal-toolbar .primary-btn'), 2000)) && $$('.cal-event[data-event]').length === before && $('.calendar'));
  view('Week');
  await until(() => $('.cal-week:not(.one-day)'));
  $('.cal-nav .secondary-btn').click();
});

await run('Changes to the calendars of this Mac have a switch of their own', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  const settings = async () => {
    await openSettings();
    await settingsSection('Calendar');
  };
  const closeSettings = async () => {
    key(document.body, 'Escape');
    await until(() => !$('.settings-view'));
  };
  const week = async () => {
    $$('.tile').find((t) => t.title.startsWith('Calendar')).click();
    await until(() => $('.calendar'));
    $$('.cal-toolbar .segmented button').find((b) => b.textContent === 'Week').click();
    await until(() => $('.cal-week:not(.one-day)'));
    $('.cal-nav .secondary-btn').click();
    await wait(150);
  };
  const press = (type, x, y, target, down = 1) => target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, buttons: down, clientX: x, clientY: y }));
  const drag = async (el, dx, dy) => {
    const box = el.getBoundingClientRect();
    const from = { x: box.left + box.width / 2, y: box.top + 5 };
    press('mousedown', from.x, from.y, el);
    press('mousemove', from.x + dx, from.y + dy, window);
    press('mouseup', from.x + dx, from.y + dy, window, 0);
    await wait(350);
  };

  await settings();
  $('#mac-calendars-switch').click();
  await until(() => $$('[data-calendar]').length === 2, 4000);
  // (A check before this one can leave a calendar hidden.)
  for (const off of $$('[data-calendar] .switch:not(.on)')) off.click();
  await until(() => !$('[data-calendar] .switch:not(.on)'), 2000);
  const write = () => $('#mac-write-switch');
  check('The switch that lets the app change the calendars of the Mac is there, and off', write() && write().getAttribute('aria-checked') === 'false' && !$('#mac-new-in'));
  await wait(250);
  snap('settings-calendar-write');
  await closeSettings();

  await week();
  const mac = () => $$('.cal-event[data-event^="mac:"]').find((e) => e.querySelector('.ev-title')?.textContent === 'Budget review');
  await until(mac, 4000);
  check('An event of the Mac is at 3 PM, and the calendar says the app may not change it', mac()?.style.top === '720px' && $('.calendar').dataset.macWrite === '0', mac()?.style.top);
  await drag(mac(), 0, 50);
  check('With that switch off, a drag does not move an event of the Mac', mac().style.top === '720px' && !mac().style.transform && !mac().classList.contains('dragging'), mac().style.top);

  await settings();
  write().click();
  const newIn = await until(() => $('#mac-new-in'), 3000);
  check('Turned on, it asks where new events go: the app, or a calendar of the Mac', write().getAttribute('aria-checked') === 'true' && newIn && [...newIn.options].map((o) => o.textContent).join('|') === 'Rune|Work|Home' && newIn.value === '', newIn && [...newIn.options].map((o) => o.textContent).join('|'));
  await closeSettings();
  await week();
  await until(mac, 4000);
  await drag(mac(), 0, 50);
  check('With the switch on, an event of the Mac is dragged to another time', await until(() => mac()?.style.top === '768px', 3000), mac()?.style.top);

  // New events go to a calendar of the Mac when one is chosen.
  await settings();
  const pick = $('#mac-new-in');
  pick.value = [...pick.options].find((o) => o.textContent === 'Work').value;
  pick.dispatchEvent(new Event('change', { bubbles: true }));
  await closeSettings();
  await week();
  $('.cal-toolbar .primary-btn').click();
  const box = await until(() => document.activeElement?.id === 'cal-quick' && document.activeElement, 3000);
  box.value = 'Vendor call today 5pm';
  box.dispatchEvent(new Event('input', { bubbles: true }));
  key(box, 'Enter');
  const made = await until(() => $$('.cal-event[data-event^="mac:"]').find((e) => e.querySelector('.ev-title')?.textContent === 'Vendor call'), 4000);
  check('With a calendar of the Mac chosen for new events, a typed event goes there, in its colour', made && made.getAttribute('style').includes('#b9a07f') && made.style.top === '816px', made?.getAttribute('style'));

  // Back as it was: new events in the app, the switch off, the calendars off.
  await settings();
  $('#mac-new-in').value = '';
  $('#mac-new-in').dispatchEvent(new Event('change', { bubbles: true }));
  write().click();
  await until(() => write().getAttribute('aria-checked') === 'false' && !$('#mac-new-in'));
  $('#mac-calendars-switch').click();
  check('Turned off again, nothing of it is left', await until(() => !$('[data-calendar]') && !$('#mac-write-switch'), 3000));
  await closeSettings();
});

await run('For other people: input methods and no AI tool', async () => {
  await oneTab();
  for (let i = 0; i < 8 && $$('.pane').length > 1; i++) {
    const n = $$('.pane').length;
    T().command('close_split');
    await until(() => $$('.pane').length < n, 1500);
  }
  await oneTab();
  // Return that only confirms a word of an input method does not save the quick note.
  $$('.tile').find((t) => t.title === 'Home').click();
  const note = await until(() => $('.home input[type="text"], .home .jot input, .home .quick-note input'), 3000);
  check('Home has its box for a quick note', note);
  if (note) {
    const pages = () => $$('.sidebar > .side-scroll .tree-item[data-fav="0"]').length;
    const before = pages();
    note.focus();
    note.value = 'にほんご';
    note.dispatchEvent(new Event('input', { bubbles: true }));
    note.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', keyCode: 229, isComposing: true, bubbles: true, cancelable: true }));
    await wait(350);
    check('Return that confirms a word of an input method does not save the note', pages() === before && note.value === 'にほんご', `${before} then ${pages()}`);
    note.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    check('The next Return saves it', await until(() => pages() === before + 1, 3000), `${before} then ${pages()}`);
  }

  // With no AI tool connected, nothing of the AI shows: no button, no menu rows, no box, no line about it.
  const dockButtons = () => $$('.ai-dock-btn').filter((b) => b.offsetParent).length;
  check('With an AI tool, Home has the AI button at its foot', dockButtons() === 1, String(dockButtons()));
  T().command('ai_off_for_test');
  await until(() => document.body.dataset.ai === '0');
  check('With no AI tool, Home has no AI button', await until(() => dockButtons() === 0 && !$('.ai-dock'), 2000), String(dockButtons()));
  const plainTitle = await newPage('A page with no AI');
  key(plainTitle, 'Enter');
  const firstLine = await until(() => document.activeElement?.classList.contains('blk') && document.activeElement);
  const started = shownPane('A page with no AI');
  check('…and a page has none', started && firstLine && dockButtons() === 0 && !$('.ai-dock'));
  // The "/" menu has no AI rows.
  typeIn(firstLine, 'Some words on a line of the page');
  await wait(150);
  key(firstLine, 'Enter');
  const fresh = await until(() => {
    const a = document.activeElement;
    return a?.classList.contains('blk') && a !== firstLine && a;
  });
  typeIn(fresh, '/');
  const slash = await until(() => $('.slash-menu'));
  const aiRows = () => $$('.slash-menu .menu-item').filter((i) => /Ask AI|Continue writing|Summarize|action items|Improve writing|Fix spelling|Translate/i.test(i.textContent)).length;
  check('The "/" menu has no AI rows', slash && $$('.slash-menu .menu-item').length > 8 && aiRows() === 0, `${$$('.slash-menu .menu-item').length} rows, ${aiRows()} of the AI`);
  key(fresh, 'Escape');
  await until(() => !$('.slash-menu'));
  // The menu of a line has no AI part.
  const withText = $$('.block-row', started).find((r) => r.querySelector('.blk')?.textContent.trim().length > 10 && r.querySelector('.handle'));
  withText.querySelector('.handle').click();
  const lineMenu = await until(() => $('.block-menu'));
  check('The menu of a line has no AI part', lineMenu && menuItem(lineMenu, 'Duplicate') && !$$('.menu-section', lineMenu).some((x) => x.textContent === 'AI') && !menuItem(lineMenu, 'Improve'), lineMenu?.textContent.slice(0, 120));
  key(document.body, 'Escape');
  await until(() => !$('.block-menu'));
  // The key of the AI opens no AI box. It shows where a tool is connected.
  T().command('ai');
  const settings = await until(() => $('.settings-view') && $$('.settings-view h2').some((h) => h.textContent === 'Connect AI tools'), 3000);
  check('The key of the AI opens no AI box: it shows Settings, where a tool is connected', settings && !$('.ai-panel') && !$('#ask-notes-input'));
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
  T().command('ask_notes');
  check('…and so does the key that asks the notes', await until(() => $('.settings-view'), 3000) && !$('#ask-notes-input'));
  key(document.body, 'Escape');
  await until(() => !$('.settings-view'));
  check('No line about the AI is left on the page', !$('#ai-setup') && !$('.ai-setup') && !$('.ai-panel') && !$('.toast')?.textContent.includes('AI'), $('.toast')?.textContent);
  // With a tool connected again, the AI is back at once.
  T().command('ai_on_for_test');
  await until(() => document.body.dataset.ai === '1');
  check('With an AI tool again, the button is back', await until(() => dockButtons() === 1, 2000), String(dockButtons()));
  // An empty line again (the "/" from before is still in it).
  fresh.focus();
  T().setSel(fresh, 0, fresh.textContent.length);
  document.execCommand('delete');
  await wait(100);
  typeIn(fresh, '/');
  await until(() => $('.slash-menu'));
  check('…and the "/" menu has its AI rows', await until(() => aiRows() >= 3, 2000), `${aiRows()} of ${$$('.slash-menu .menu-item').length} rows, line "${fresh.textContent}"`);
  key(fresh, 'Escape');
  await until(() => !$('.slash-menu'));
  askAi();
  check('…and it opens the AI bar, with no line about set-up', await until(() => $('.ai-dock #ai-input'), 3000) && !$('#ai-setup'));
  key($('#ai-input') || document.body, 'Escape');
  await wait(200);
});

await run('Space switch speed', async () => {
  // Only when it is asked for by name: it is a measurement, not a check.
  if (!ONLY.length) return;
  await oneTab();
  $('.ws-name').click();
  menuItem(await until(() => $('.space-menu')), 'New space').click();
  const input = await until(() => $('.space-new input'));
  input.value = 'Speed space';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(50);
  key(input, 'Enter');
  await until(() => $('.ws-label')?.textContent === 'Speed space');
  for (let i = 0; i < 8; i++) await newPage('Speed page ' + i);
  T().command('space_prev');
  await until(() => $('.ws-label')?.textContent === 'My Space');
  await wait(500);
  treeItem('Getting Started').click();
  await until(() => shownPane('Getting Started'));
  await wait(400);
  // Every frame while the space changes six times.
  const gaps = [];
  let on = true;
  let before = performance.now();
  const tick = (t) => {
    gaps.push(t - before);
    before = t;
    if (on) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  await wait(300);
  const calm = gaps.length;
  const toList = [];
  for (let i = 0; i < 6; i++) {
    const want = i % 2 === 0 ? 'Speed space' : 'My Space';
    const t0 = performance.now();
    T().command(i % 2 === 0 ? 'space_next' : 'space_prev');
    await until(() => $('.ws-label')?.textContent === want, 3000, 2);
    toList.push(Math.round(performance.now() - t0));
    await wait(700);
  }
  on = false;
  const during = gaps.slice(calm).sort((a, b) => b - a);
  perf.spaceSwitch = { toListMs: toList, longestFrameGapsMs: during.slice(0, 10).map((g) => Math.round(g)), frames: during.length, usualGapMs: Math.round(during[Math.floor(during.length / 2)] || 0) };
  check('The change of space was measured', toList.length === 6, JSON.stringify(perf.spaceSwitch));
});

await run('Speed measurements', async () => {
  if (!treeItem('Long page')) {
    perf.skipped = 'no "Long page" in this database';
    return;
  }
  await oneTab();
  perf.openLongPage = await measure(async () => {
    treeItem('Long page').click();
    await until(() => $$('.pane .block-row').length >= 400, 15000);
  });
  perf.blocks = $$('.pane .block-row').length;
  await wait(300);
  // Typing in the middle of the long page, about 60 keys a second.
  const blk = $$('.pane .blk')[200];
  blk.focus();
  T().setSel(blk, 0);
  perf.typing = await measure(async () => {
    for (let i = 0; i < 60; i++) {
      document.execCommand('insertText', false, 'a');
      await wait(16);
    }
  });
  // Round trip through the app: rename the page and wait for its tab to follow.
  const title = $('.pane .title-input');
  const trips = [];
  for (let i = 0; i < 15; i++) {
    const want = 'Long page ' + i;
    const t0 = performance.now();
    title.value = want;
    title.dispatchEvent(new Event('input', { bubbles: true }));
    await until(() => $('.tab.active')?.textContent.includes(want), 3000, 2);
    trips.push(performance.now() - t0);
  }
  trips.sort((a, b) => a - b);
  perf.titleToTabMs = { median: Math.round(trips[7]), worst: Math.round(trips[14]) };
  title.value = 'Long page';
  title.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(200);
  // Scrolling the long page.
  const scroller = $('.pane .scroller');
  perf.scroll = await measure(async () => {
    for (let i = 0; i < 90; i++) {
      scroller.scrollTop += 90;
      await new Promise((r) => requestAnimationFrame(r));
    }
  });
  // Switching between Home and the long page (clicking the page opened it in a second tab).
  const homeTab = $$('.tab').findIndex((t) => !t.textContent.includes('Long page'));
  const longTab = $$('.tab').findIndex((t) => t.textContent.includes('Long page'));
  perf.tabs = $$('.tab').length;
  const switches = [];
  perf.tabSwitch = await measure(async () => {
    for (let i = 0; i < 12; i++) {
      const toHome = i % 2 === 0;
      const target = toHome ? homeTab : longTab;
      const t0 = performance.now();
      $$('.tab')[target].click();
      await until(() => $$('.tab')[target]?.classList.contains('active') && (toHome ? $('.home-hero') : $$('.pane .block-row').length >= 400), 5000, 2);
      switches.push(performance.now() - t0);
    }
  });
  switches.sort((a, b) => a - b);
  perf.tabSwitchMs = { median: Math.round(switches[6]), worst: Math.round(switches[11]) };
  perf.domNodes = document.getElementsByTagName('*').length;
  await oneTab();
});

dioxus.send({ results, snaps, perf, views, title: document.title });
