# Bundled drawing libraries

Drawing blocks run [Excalidraw](https://github.com/excalidraw/excalidraw) in a
frame of their own. These files are copied unchanged from the official npm
packages so drawing works offline:

| Files in `vendor/` | Package | Licence |
| --- | --- | --- |
| `excalidraw.production.min.js`, `excalidraw-assets/*` | `@excalidraw/excalidraw` 0.17.6 | MIT, Copyright (c) 2020 Excalidraw |
| `react.production.min.js` | `react` 18.3.1 | MIT, Copyright (c) Facebook, Inc. and its affiliates (`LICENSE-react.txt`) |
| `react-dom.production.min.js` | `react-dom` 18.3.1 | MIT, same as React |

Third-party notices bundled inside Excalidraw are in the `*.LICENSE.txt` files
next to its scripts. The language files are left out; English is built in.

`index.html` and `frame.js` are this app's own: they load the libraries and pass
the scene to and from the page.

Version 0.17.6 is the last release that ships ready-to-load browser files
(newer releases need a JavaScript build step).
