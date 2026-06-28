// peacock-web — VM stub of the MCP-Apps bridge.
//
// Selected by the conditional import in `mcp_bridge.dart` when `dart.library.js_interop`
// is absent (i.e. the Dart VM, where `flutter test` runs the widget smoke test).
// `package:web` is web-only, so we provide an inert transport that never fetches
// or postMessages — the smoke test only verifies the widget tree builds.

import 'dart:async';

import 'mcp_bridge.dart';

/// VM: there is no browser embedding; return an inert transport.
Mcp detectMcp() => _StubMcp();

/// VM: no URL fragment; always the fallback report id.
String reportIdFromUrlImpl(String fallback) => fallback;

class _StubMcp implements Mcp {
  @override
  bool get embedded => false;

  @override
  Future<Map<String, Object?>> callServerTool(
    String name,
    Map<String, Object?> arguments,
  ) async {
    // The widget smoke test pumps once and asserts the loading state; it never
    // awaits a render. Stay pending so no fake data leaks into the tree.
    final completer = Completer<Map<String, Object?>>();
    return completer.future;
  }

  @override
  void updateModelContext(Map<String, Object?> record) {}
}
