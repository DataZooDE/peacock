// peacock-web — the MCP-Apps host bridge (FR-M-1, FR-M-3, FR-X-3).
//
// peacock's interactive surface is this Flutter-web app. It runs in two modes:
//
//   * **Embedded** — loaded inside an MCP host's sandboxed iframe (directly, or
//     via the `ui://` shim that nests `/app/`; see
//     `doc/flutter-iframe-runtime-proposal.md`). The host fulfils server-tool
//     calls; the app never touches peacock's HTTP origin itself. It talks to
//     its parent over `postMessage` using the MCP-Apps verbs that
//     `crates/peacock-server/assets/iframe.html` established:
//       - request : `{ type: "mcp:callServerTool", reqId, name, arguments }`
//       - result  : `{ type: "mcp:callServerTool:result", reqId, result }`
//       - context : `{ type: "mcp:updateModelContext", record }`
//
//   * **Standalone** — opened directly at peacock's `/app` (no parent frame).
//     Then it calls peacock's same-origin `POST /v1/render_report` itself, so
//     the bundle stays independently verifiable (NFR-S-5).
//
// This file is the platform-neutral *interface*. The real, web-only transport
// lives in `mcp_bridge_web.dart` (uses `dart:js_interop` + `package:web`); a VM
// stub in `mcp_bridge_stub.dart` keeps the widget smoke test compiling under
// `flutter test` (which runs on the Dart VM, where `package:web`'s helpers do
// not compile). The conditional import below selects the right one.

import 'mcp_bridge_stub.dart'
    if (dart.library.js_interop) 'mcp_bridge_web.dart';

/// A render request/response transport. Implementations: a host bridge
/// (postMessage to the MCP host) and an HTTP fallback (same-origin fetch).
abstract class Mcp {
  /// Pick the transport for the current embedding context: the host bridge when
  /// embedded in an MCP host's iframe, the same-origin HTTP fallback when
  /// standalone. On the VM (tests) this returns an inert stub.
  static Mcp detect() => detectMcp();

  /// The report id this surface should render. The shim passes it on the URL
  /// fragment (`#mcp&report=<id>`) so the embedded app renders the same report
  /// the host's `ui://peacock/<report>` link named. Falls back to [fallback].
  static String reportIdFromUrl(String fallback) => reportIdFromUrlImpl(fallback);

  /// True when running inside an MCP host (drives UI affordances/labels).
  bool get embedded;

  /// Run a server tool and return its decoded JSON result (the render
  /// artifact). Mirrors MCP-Apps `callServerTool(name, arguments)`.
  Future<Map<String, Object?>> callServerTool(
    String name,
    Map<String, Object?> arguments,
  );

  /// Publish a compact, committed view-state record to the model's context
  /// (FR-X-3). No-op in standalone mode. Never carries row data.
  void updateModelContext(Map<String, Object?> record);
}
