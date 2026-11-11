# Bundled diagram library

Code blocks of the language Mermaid are drawn as diagrams by
[Mermaid](https://github.com/mermaid-js/mermaid). `mermaid.min.js` is copied
unchanged from the npm package `mermaid` 11.16.1 (`dist/mermaid.min.js`), so
diagrams draw offline. Licence: MIT, Copyright (c) 2014 - 2022 Knut Sveidqvist
(`LICENSE`). Notices of the libraries bundled inside it are in the file itself.

The app serves it from `n2draw://app/mermaid.min.js` (see `src/drawing.rs`) and
loads it only when a page shows a diagram (see `drawDiagrams` in
`assets/bridge.js`).
