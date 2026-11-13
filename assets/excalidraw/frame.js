// One Excalidraw canvas, shown inside a Drawing block. It runs in a frame of
// its own so its styles, keyboard shortcuts and scrolling stay apart from the
// page. It talks to the page around it with postMessage:
//   frame → page: ready · save {json} · expand
//   page → frame: load {json} · expanded {on}
// The scene is saved as Excalidraw's own JSON (the same as a .excalidraw file).

(() => {
  const params = new URLSearchParams(location.search);
  const testing = params.get('test') === '1';
  const { Excalidraw, getSceneVersion, serializeAsJSON, restoreElements } = window.ExcalidrawLib;
  const h = React.createElement;
  const tell = (kind, extra) => parent.postMessage(Object.assign({ n2draw: kind }, extra), '*');
  const dark = matchMedia('(prefers-color-scheme: dark)');

  let api = null;
  let loaded = false; // has the first scene arrived from the page?
  let ready = false; // is it on the canvas? Nothing is saved before this.
  let later = null; // a newer scene that came while the first was still loading
  let shownVersion = -1; // scene version that is already saved (or came from the page)
  let shownBg = null;
  let timer = 0;
  let resolveFirst;
  const firstScene = new Promise((r) => (resolveFirst = r));

  const parse = (json) => {
    try {
      const d = JSON.parse(json);
      return { elements: d.elements || [], appState: d.appState || {}, files: d.files || {} };
    } catch {
      return { elements: [], appState: {}, files: {} };
    }
  };

  const remember = () => {
    shownVersion = getSceneVersion(api.getSceneElementsIncludingDeleted());
    shownBg = api.getAppState().viewBackgroundColor;
  };

  // A scene from the page: the first one, or a change made elsewhere
  // (another window, an undo, an agent).
  const load = (json) => {
    const scene = json ? parse(json) : { elements: [], appState: {}, files: {} };
    if (!loaded) {
      loaded = true;
      resolveFirst(Object.assign({}, scene, { scrollToContent: true }));
      return;
    }
    if (!api || !ready) {
      later = json;
      return;
    }
    clearTimeout(timer);
    timer = 0;
    const files = Object.values(scene.files);
    if (files.length) api.addFiles(files);
    const appState = scene.appState.viewBackgroundColor ? { viewBackgroundColor: scene.appState.viewBackgroundColor } : undefined;
    api.updateScene({ elements: restoreElements(scene.elements, null, { refreshDimensions: false, repairBindings: true }), appState });
    remember();
  };

  const save = () => {
    clearTimeout(timer);
    timer = 0;
    if (!api || !ready) return;
    remember();
    tell('save', { json: serializeAsJSON(api.getSceneElements(), api.getAppState(), api.getFiles(), 'local') });
  };

  // Called a lot (every pointer move); only real changes to the drawing count.
  const onChange = (elements, appState) => {
    if (!api || !loaded || appState.isLoading) return;
    if (!ready) {
      // The first scene is on the canvas now; it is what's saved already.
      ready = true;
      remember();
      if (later !== null) {
        const json = later;
        later = null;
        load(json);
      }
      return;
    }
    const v = getSceneVersion(api.getSceneElementsIncludingDeleted());
    if (v === shownVersion && appState.viewBackgroundColor === shownBg) return;
    clearTimeout(timer);
    timer = setTimeout(save, 400);
  };
  // Finishing a stroke or a drag saves right away; typing waits for a pause.
  addEventListener('pointerup', () => timer && setTimeout(() => timer && save(), 0), true);
  addEventListener('pagehide', () => timer && save());

  const icon = (expanded) =>
    h(
      'svg',
      { viewBox: '0 0 16 16', fill: 'none', stroke: 'currentColor', strokeWidth: 1.5, strokeLinecap: 'round', strokeLinejoin: 'round' },
      expanded
        ? h('path', { d: 'M6.5 2.5v4h-4M9.5 13.5v-4h4M6.5 6.5 2 2M9.5 9.5 14 14' })
        : h('path', { d: 'M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5 9 7M2.5 13.5 7 9' }),
    );

  let setExpandedState = () => {};
  function App() {
    const [theme, setTheme] = React.useState(dark.matches ? 'dark' : 'light');
    const [expanded, setExpanded] = React.useState(false);
    React.useEffect(() => {
      const follow = () => setTheme(dark.matches ? 'dark' : 'light');
      dark.addEventListener('change', follow);
      setExpandedState = setExpanded;
      return () => dark.removeEventListener('change', follow);
    }, []);
    return h(Excalidraw, {
      excalidrawAPI: (a) => (api = a),
      initialData: firstScene,
      theme,
      onChange,
      UIOptions: { canvasActions: { loadScene: false, saveToActiveFile: false, toggleTheme: false, export: { saveFileToDisk: true } } },
      renderTopRightUI: () =>
        h('button', { className: 'n2-expand', title: expanded ? 'Back to the page' : 'Expand', onClick: () => tell('expand') }, icon(expanded)),
    });
  }

  addEventListener('message', (e) => {
    if (e.source !== parent || !e.data || typeof e.data !== 'object') return;
    const d = e.data;
    if (d.n2draw === 'load') load(typeof d.json === 'string' ? d.json : '');
    else if (d.n2draw === 'expanded') setExpandedState(!!d.on);
    // (A press outside the drawing: see the page script.)
    else if (d.n2draw === 'flush') {
      if (timer) save();
    }
    else if (testing && d.n2draw === 'probe') probe();
    else if (testing && d.n2draw === 'test-add') testAdd();
    else if (testing && d.n2draw === 'test-expand') document.querySelector('.n2-expand')?.click();
  });

  // ----- only in the app's self-test -----
  async function probe() {
    // "ready" is sent before the canvas mounts; give it a moment.
    for (let i = 0; i < 100 && !(api && document.querySelector('.excalidraw canvas')); i++) await new Promise((r) => setTimeout(r, 50));
    const fonts = await document.fonts.load('20px Virgil').catch(() => []);
    tell('probe', {
      info: {
        canvas: !!document.querySelector('.excalidraw canvas'),
        elements: api ? api.getSceneElements().length : -1,
        dark: !!document.querySelector('.excalidraw.theme--dark'),
        font: fonts.length > 0,
        expandButton: !!document.querySelector('.n2-expand'),
      },
    });
  }
  function testAdd() {
    if (!api) return;
    const added = window.ExcalidrawLib.convertToExcalidrawElements([{ type: 'rectangle', x: 40, y: 40, width: 160, height: 90 }]);
    api.updateScene({ elements: api.getSceneElementsIncludingDeleted().concat(added) });
  }

  ReactDOM.createRoot(document.getElementById('root')).render(h(App));
  tell('ready');
})();
