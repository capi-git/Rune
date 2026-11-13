// A Mermaid diagram: a code block of the language "mermaid", drawn as the
// Mac draws it (`drawDiagram` in assets/bridge.js), with the same library
// (assets/mermaid, linked into the app as Vendor/mermaid.min.js) and the
// colours of the app. Read-only; nothing is fetched from the network.

import SwiftUI
import WebKit

struct DiagramView: View {
    let source: String
    @Environment(\.colorScheme) private var scheme
    @State private var height: CGFloat = 120
    @State private var failure: String?

    var body: some View {
        if let failure {
            VStack(alignment: .leading, spacing: 6) {
                Text("This diagram cannot be drawn: \(failure)")
                    .font(.system(size: 13))
                    .foregroundStyle(Theme.text2)
                Text(source)
                    .font(.system(size: 12.5, design: .monospaced))
                    .foregroundStyle(Theme.text2)
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.field, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        } else {
            DiagramWeb(source: source, dark: scheme == .dark, height: $height, failure: $failure)
                .frame(height: height)
                .frame(maxWidth: .infinity)
                .accessibilityLabel("Diagram")
        }
    }
}

private struct DiagramWeb: UIViewRepresentable {
    let source: String
    let dark: Bool
    @Binding var height: CGFloat
    @Binding var failure: String?

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.userContentController.add(context.coordinator, name: "diagram")
        // Only the page and the library in the app: no network at all.
        config.websiteDataStore = .nonPersistent()
        let web = WKWebView(frame: .zero, configuration: config)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.scrollView.isScrollEnabled = false
        web.navigationDelegate = context.coordinator
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {
        context.coordinator.parent = self
        let key = "\(dark)\n\(source)"
        guard context.coordinator.drawn != key else { return }
        context.coordinator.drawn = key
        web.loadHTMLString(Self.page(source: source, dark: dark), baseURL: Bundle.main.resourceURL)
    }

    static func dismantleUIView(_ web: WKWebView, coordinator: Coordinator) {
        web.configuration.userContentController.removeScriptMessageHandler(forName: "diagram")
    }

    final class Coordinator: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
        var parent: DiagramWeb
        var drawn = ""

        init(_ parent: DiagramWeb) {
            self.parent = parent
        }

        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            guard let body = message.body as? [String: Any] else { return }
            if let height = body["height"] as? Double {
                parent.height = max(CGFloat(height), 40)
            } else if let error = body["error"] as? String {
                parent.failure = error
            }
        }

        // A link in a diagram opens nothing inside it.
        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            action.navigationType == .other ? .allow : .cancel
        }
    }

    /// The colours of `drawDiagram` on the Mac, as opaque hex values.
    private static func look(dark: Bool) -> [String: String] {
        dark
            ? ["text": "#f2f2f3", "soft": "#a1a1a6", "fill": "#2a2a2d", "fill2": "#232325", "edge": "#3a3a3d", "ground": "#1e1e20"]
            : ["text": "#15171a", "soft": "#4f5660", "fill": "#eeeef1", "fill2": "#f5f5f7", "edge": "#d9d9de", "ground": "#ffffff"]
    }

    private static func page(source: String, dark: Bool) -> String {
        let look = look(dark: dark)
        let json = (try? JSONSerialization.data(withJSONObject: ["source": source, "dark": dark, "look": look]))
            .map { String(decoding: $0, as: UTF8.self) } ?? "{}"
        return """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1">
        <style>
          html,body{margin:0;padding:0;background:transparent;-webkit-text-size-adjust:100%}
          #d{display:flex;justify-content:center}
          #d svg{max-width:100%;height:auto}
        </style>
        <script src="mermaid.min.js"></script></head>
        <body><div id="d"></div><script>
        (async () => {
          const it = \(json);
          const post = (m) => window.webkit.messageHandlers.diagram.postMessage(m);
          try {
            const L = it.look;
            mermaid.initialize({
              startOnLoad: false, securityLevel: 'strict', theme: 'base',
              fontFamily: '-apple-system, "Helvetica Neue", Helvetica, Arial, sans-serif',
              themeVariables: {
                darkMode: it.dark, background: L.ground, fontSize: '14px',
                primaryColor: L.fill, primaryTextColor: L.text, primaryBorderColor: L.edge,
                secondaryColor: L.fill2, secondaryTextColor: L.text, secondaryBorderColor: L.edge,
                tertiaryColor: L.fill2, tertiaryTextColor: L.text, tertiaryBorderColor: L.edge,
                lineColor: L.soft, textColor: L.text, mainBkg: L.fill, nodeBorder: L.edge,
                clusterBkg: L.fill2, clusterBorder: L.edge, edgeLabelBackground: L.ground,
                actorBkg: L.fill, actorBorder: L.edge, actorTextColor: L.text,
                signalColor: L.soft, signalTextColor: L.text,
                noteBkgColor: L.fill2, noteTextColor: L.text, noteBorderColor: L.edge,
              },
              flowchart: { curve: 'basis', padding: 14, htmlLabels: true },
            });
            await mermaid.parse(it.source);
            const { svg } = await mermaid.render('rune-diagram', it.source);
            const el = document.getElementById('d');
            el.innerHTML = svg;
            el.querySelectorAll('.node rect.label-container, .node rect.basic').forEach((r) => {
              if (!r.getAttribute('rx') || r.getAttribute('rx') === '0') { r.setAttribute('rx', '6'); r.setAttribute('ry', '6'); }
            });
            requestAnimationFrame(() => post({ height: el.getBoundingClientRect().height }));
          } catch (e) {
            const line = String((e && e.message) || e).split('\\n').filter((l) => l.trim()).slice(0, 2).join(' ');
            post({ error: line || 'unknown fault' });
          }
        })();
        </script></body></html>
        """
    }
}
