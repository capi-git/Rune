// Developer self-test, run inside the real app window when NOTION2_SELFTEST is
// set (always with a throwaway workspace via NOTION2_DB). It clicks and types
// through the interface like a person would and reports what worked.

const results = [];
// Saved copies of the screen at a few moments, for checking the look.
const snaps = {};
const snap = (name) => {
  snaps[name] = document.documentElement.outerHTML;
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
const treeItem = (title) => $$('.sidebar .tree-item').find((t) => t.querySelector('.tree-title')?.textContent === title);
const typeIn = (el, text) => {
  el.focus();
  for (const ch of text) document.execCommand('insertText', false, ch);
};
// Whether the window was in view at the start and end of each check (macOS
// pauses drawing in a window out of view, which stalls the checks).
const views = {};
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
  $('.new-tab').click();
  await until(() => $$('.tab').length === 2);
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
  $$('.tile').find((t) => t.title.startsWith('Ask AI')).click();
  const panel = await until(() => $('.ai-panel'));
  check('Ask AI opens the panel', panel);
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
  $('.new-tab').click();
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
  dots()[1].click();
  const list = await until(() => $('.ws-label')?.textContent === 'Second space' && $('.side-scroll'));
  check('A click on a dot goes to its space, and its list comes in from that side', list && current() === 1 && treeItem('Only in space two') && list.classList.contains('from-next'), list?.className);
  check('…and the mark of the move goes away when it is over', await until(() => $('.side-scroll').className === 'side-scroll', 2000), $('.side-scroll').className);

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
  $$('.tile').find((t) => t.title.startsWith('Ask AI')).click();
  const pick = await until(() => $('#ai-model'));
  check('Ask AI has a picker that shows the strength as four dots and the model’s name', pick && $$('.ai-pips i', pick).length === 4 && $('.ai-model-name', pick)?.textContent.length > 2 && !$('.ai-panel select'), pick?.textContent);
  pick.click();
  const list = await until(() => $('.ai-menu'));
  const models = () => $$('.ai-menu-row[data-model]');
  const efforts = () => $$('.ai-menu-row[data-effort]');
  check('It opens a menu of the models, grouped by tool, and of five strengths', list && models().length > 1 && $$('.ai-menu-label').length >= 2 && efforts().length === 5, `${models().length} models, ${efforts().length} strengths`);
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
  const remind = $('.cal-editor select.select[title^="A Mac notification"]');
  check('New events get a 10-minute reminder', remind && remind.selectedOptions[0]?.textContent === '10 min before', remind && remind.selectedOptions[0]?.textContent);
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
  const times = $$('.cal-editor .ed-times input').map((i) => i.value).join(' to ');
  check('The panel shows the 15 minutes', /6:15.*6:30|06:15.*06:30/.test(times), times);
  $('.cal-editor .ghost-btn.danger-text').click();
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
  check('Events start out not repeating', rule && rule.value === 'none', rule && rule.value);
  rule.value = 'week';
  rule.dispatchEvent(new Event('change', { bubbles: true }));
  check('Choosing a repeat offers an end', await until(() => $$('.cal-editor .ed-label').some((l) => l.textContent === 'Ends')));
  const shown = () => $$('.cal-event').find((e) => e.querySelector('.ev-title')?.textContent.includes('Weekly review'));
  const next = () => $('.cal-nav .icon-btn[title="Next"]').click();
  next();
  await wait(120);
  const ev = await until(shown);
  check('A weekly event shows the next week too, marked as repeating', ev && ev.querySelector('.repeat-mark'));
  // The panel still shows this week's repeat; wait for the one clicked.
  const kicker = $('.cal-editor .ed-kicker')?.textContent;
  ev.click();
  await until(() => $('.cal-editor .ed-kicker') && $('.cal-editor .ed-kicker').textContent !== kicker);
  check('A repeat can be changed on its own', await until(() => $$('.cal-editor .link-btn').some((b) => b.textContent === 'Change only this one')));
  const del = $$('.cal-editor .ghost-btn.danger-text').find((b) => b.textContent.includes('Delete'));
  del.click();
  const only = await until(() => $$('.ed-delete-choice button').find((b) => b.textContent === 'Only this one'));
  check('Deleting a repeat asks: only this one, or all', only, `${del.textContent} | ${$('.cal-editor')?.outerHTML.slice(-900)}`);
  if (!only) return;
  only.click();
  check('Deleting only that repeat removes it from its week', await until(() => !shown()));
  next();
  await wait(120);
  const later = await until(shown);
  check('The other repeats stay', later);
  later.click();
  await until(() => $$('.cal-editor .ghost-btn.danger-text').length);
  $$('.cal-editor .ghost-btn.danger-text').find((b) => b.textContent.includes('Delete')).click();
  (await until(() => $$('.ed-delete-choice button').find((b) => b.textContent === 'All repeats'))).click();
  check('Deleting all repeats removes the event', await until(() => !shown()));
  $('.cal-nav .secondary-btn').click();
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
  $('.cal-editor .ghost-btn.danger-text').click();
  await until(() => !$('.cal-editor'));
  cal.style.width = '';
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
  check('A mark shows where now is (between 8 AM and 6 PM)', hour < 8 || hour > 18 ? !mark : mark && Math.abs(parseFloat(mark.style.left) - ((hour - 8) / 10) * 100) < 3, mark ? mark.style.left : 'no mark');
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
  T().command('meeting_ended');
  check('When it ends, the line gets the end time and length', await until(() => /\(\d+ min\)/.test(line())), line());
  snap('meeting-notes');
});

await run('Speak a page', async () => {
  await oneTab();
  const made = () => $$('.sidebar .tree-item .tree-title').filter((t) => t.textContent === 'Shop and call list').length;
  check('The sidebar has one AI tile and no second microphone', $$('.sidebar .tile').map((t) => t.title.replace(/ \(.*/, '')).join(', ') === 'Home, Calendar, Graph: how your pages connect, Ask AI', $$('.sidebar .tile').map((t) => t.title).join(', '));
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
  cells()[2].focus();
  await until(() => table.dataset.c === '2');
  $('[data-table-op="remove-column"]', table).click();
  check('The buttons act on the cell with the caret: its column goes', await until(() => grid() === 'Item|Amount / Rent|$2,000 / | / |', 3000), grid());
  cells()[1].focus();
  await until(() => table.dataset.c === '1' && table.dataset.r === '0');
  $('[data-table-op="add-column"]', table).click();
  check('…and a column is added after it', await until(() => grid() === 'Item|Amount| / Rent|$2,000| / || / ||', 3000), grid());
  cells()[9].focus();
  await until(() => table.dataset.r === '3');
  $('[data-table-op="remove-row"]', table).click();
  check('…and its row goes', await until(() => $$('tr', table).length === 3, 3000), grid());
  cells()[3].focus();
  key(document.activeElement, 'Enter');
  check('Return goes to the cell below', await until(() => document.activeElement === cells()[6]));
  await wait(200);
  check('The buttons show while a cell has the caret', table.classList.contains('editing') && getComputedStyle($('.table-tools', table)).opacity === '1', getComputedStyle($('.table-tools', table)).opacity);
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
  check('It has the shape of a video and sits under its line', Math.abs(r.width / r.height - 16 / 9) < 0.02 && r.width >= 300 && r.top >= first.getBoundingClientRect().bottom - 1, `${r.width} x ${r.height}`);
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
  $('#player-close', dock).click();
  check('Its close button puts it away', await until(() => !$('#player-dock'), 2000));
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
  $$('.tile').find((t) => t.title.startsWith('Ask AI')).click();
  const ask = await until(() => $('.modal.ask-notes #ask-notes-input'));
  check('With no page open, Ask AI asks all your notes', ask);
  check('The question can be spoken', $('[data-dictate="ask-notes-input"]'));
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
});

await run('Voice to calendar and to-dos', async () => {
  await oneTab();
  const d = new Date(Date.now() + 86400000);
  const day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const pagesNow = $$('.sidebar .tree-item').length;
  speakPage();
  await say(`lunch with Sam tomorrow at one, every week [[reply: @event {"title": "Lunch with Sam", "start": "${day}T13:00", "repeat": "weekly"}]]`);
  check('Asked for the calendar, it makes the event and shows it', await until(() => $('.cal-editor .ed-title')?.value === 'Lunch with Sam', 6000), `${$('.cal-editor .ed-title')?.value} | ${$('.toast')?.textContent}`);
  check('The event is at the time that was said, and repeats', $('.cal-editor .ed-kicker')?.textContent.includes('1 PM') && $('#ev-repeat')?.value === 'week', `${$('.cal-editor .ed-kicker')?.textContent} | ${$('#ev-repeat')?.value}`);
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
  check('The page menu holds only what belongs to this page', (await items()).join(', ') === 'Copy link, Copy for Aven, Page history, Move to Trash', (await (async () => $$('.page-menu .menu-item').map((i) => i.textContent).join(', '))()));
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
  const choose = (place, name) => {
    place.value = [...place.options].find((o) => o.textContent.trim() === name).value;
    place.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const place = made && (await ask(made));
  const names = () => [...$('#meeting-place').options].map((o) => o.textContent.trim());
  check('"Move to…" in the menu of the page asks which page to keep it in', place && $('.place-bar .listen-what', made).textContent === 'Keep these notes in' && place.selectedOptions[0]?.textContent.trim() === 'Meeting notes' && names().includes('Reading List') && !names().some((n) => datedName.test(n)), place && names().join(' | '));
  if (!place) return;
  choose(place, 'Reading List');
  check('A choice moves the page there at once', await until(() => crumbs().includes('Reading List') && $('#meeting-place')?.selectedOptions[0]?.textContent.trim() === 'Reading List', 3000), crumbs().join(' / '));
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
  check('On any other page it asks the same, with "no page" chosen for a page at the top', where && $('.place-bar .listen-what', loose).textContent === 'Keep this page in' && where.selectedOptions[0]?.value === '', where && where.selectedOptions[0]?.textContent);
  if (!where) return;
  choose(where, 'Getting Started');
  const looseCrumbs = () => $$('.pagebar .crumb', loose).map((c) => c.textContent.trim());
  check('…and a choice puts the page inside that page', await until(() => looseCrumbs().includes('Getting Started'), 3000), looseCrumbs().join(' / '));
  choose($('#meeting-place', loose), 'No page (top of the list)');
  check('"No page" puts it back at the top of the list', await until(() => !looseCrumbs().includes('Getting Started') && treeItem('Loose page'), 3000), looseCrumbs().join(' / '));
  $('#meeting-place-done').click();
  await until(() => !$('#meeting-place'));
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
  place.value = [...place.options].find((o) => o.textContent.trim() === 'Reading List').value;
  place.dispatchEvent(new Event('change', { bubbles: true }));
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
});

await run('Meeting recap by voice', async () => {
  const dated = / – [A-Z][a-z]{2} \d{1,2}, \d{4}$/;
  const row = $$('.sidebar .tree-item').find((r) => {
    const t = r.querySelector('.tree-title')?.textContent || '';
    return dated.test(t) && !t.startsWith('Voice note');
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
  const places = () => [...$('#meeting-place').options].map((o) => o.textContent.trim());
  check('When the meeting is over, its page asks which page to keep it in', place && place.selectedOptions[0]?.textContent.trim() === 'Meeting notes' && places().includes('Getting Started') && !places().some((o) => datedName.test(o)), place && places().join(' | '));
  await wait(120);
  snap('meeting-place');
  if (place) {
    place.value = [...place.options].find((o) => o.textContent.trim() === 'Getting Started').value;
    place.dispatchEvent(new Event('change', { bubbles: true }));
  }
  check('A choice moves the page there at once', await until(() => $('.crumbs', page)?.textContent.includes('Getting Started') && $('#meeting-place')?.selectedOptions[0]?.textContent.trim() === 'Getting Started', 3000), $('.crumbs', page)?.textContent);
  $('#meeting-place-done')?.click();
  check('"Done" puts the question away', await until(() => !$('#meeting-place'), 2000));
  // A page that is not a meeting's has none of this.
  treeItem('Typing test').click();
  await until(() => shownPane('Typing test'));
  check('Other pages show no listening line, and no band of voices', !$('.listen-bar') && !$('.meeting-band', shownPane('Typing test')));
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
  const typed = await until(() => (real ? ask.value.length > 'About Friday:'.length + 3 : ask.value === 'About Friday: So the budget is agreed.'), real ? 15000 : 4000);
  check('What is heard goes into that box, after what it held', typed && ask.value.startsWith('About Friday: ') && !$('#ask-notes-go').disabled, ask.value);
  check('After a pause it stops listening by itself', await until(() => !mic.classList.contains('hearing') && !document.body.dataset.dictating, 9000));
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
  first.querySelector('.todo-box').click();
  check('Ticking one here takes it off the list', await until(() => count() === before - 1 && !rows().some((r) => r.querySelector('.todo-text').textContent === text && r.querySelector('.todo-from').textContent === from), 3000), `${before} -> ${count()}`);
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
  check('What needs the AI says so', await until(() => ($('.toast')?.textContent || '').includes('needs an AI tool')) && !$('.modal.voice'), $('.toast')?.textContent);
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
  check('A fourth button beside Home, Calendar and Ask AI opens the graph', tile && $$('.tile').length === 4 && $$('.tile').every((t) => t.getBoundingClientRect().width >= 36), $$('.tile').map((t) => Math.round(t.getBoundingClientRect().width)).join(' '));
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
  check('The corner says how many pages and links there are', $('.graph-count').textContent === `${data.nodes.length} pages · ${links} ${links === 1 ? 'link' : 'links'}`, $('.graph-count').textContent);
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
  check('An event with no meeting address has no Join button', !$('#ev-join'));
  const link = 'https://us02web.zoom.us/j/8123456789?pwd=abc';
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
