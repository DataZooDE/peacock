// peacock-web — the web (browser) implementation of the MCP-Apps bridge.
//
// Selected by the conditional import in `mcp_bridge.dart` when compiling for the
// web (`dart.library.js_interop` present). It uses `dart:js_interop` +
// `package:web` for the host `postMessage` channel and the standalone `fetch`.

import 'dart:async';
import 'dart:convert';
import 'dart:js_interop';
import 'dart:js_interop_unsafe';

import 'package:web/web.dart' as web;

import 'mcp_bridge.dart';

/// Pick the transport for the current embedding context (see [Mcp.detect]).
Mcp detectMcp() {
  // `window.parent === window` ⇒ top-level ⇒ standalone. A null parent or any
  // access error (cross-origin parent) means we *are* embedded.
  var embedded = false;
  try {
    final parent = web.window.parent as JSObject?;
    final self = web.window as JSObject;
    embedded = parent == null || !parent.strictEquals(self).toDart;
  } catch (_) {
    embedded = true;
  }
  return embedded ? _HostBridge() : _HttpFallback();
}

/// Read the report id from the URL fragment (see [Mcp.reportIdFromUrl]).
String reportIdFromUrlImpl(String fallback) {
  var hash = web.window.location.hash;
  if (hash.startsWith('#')) hash = hash.substring(1);
  for (final part in hash.split('&')) {
    final kv = part.split('=');
    if (kv.length == 2 && kv[0] == 'report' && kv[1].isNotEmpty) {
      return Uri.decodeComponent(kv[1]);
    }
  }
  return fallback;
}

/// Embedded transport: relay `callServerTool` / `updateModelContext` to the
/// parent frame (the MCP host, or the `ui://` shim that proxies to it) and
/// await the matching `mcp:callServerTool:result`.
class _HostBridge implements Mcp {
  _HostBridge() {
    _listener = ((web.MessageEvent e) {
      final data = e.data;
      if (data == null || !data.isA<JSObject>()) return;
      final obj = data as JSObject;
      final type = obj.getProperty<JSAny?>('type'.toJS).dartify();
      if (type != 'mcp:callServerTool:result') return;
      final reqId = obj.getProperty<JSAny?>('reqId'.toJS).dartify()?.toString();
      final pending = reqId == null ? null : _pending.remove(reqId);
      if (pending == null) return;
      final result = obj.getProperty<JSAny?>('result'.toJS).dartify();
      pending.complete(_asStringMap(result));
    }).toJS;
    web.window.addEventListener('message', _listener);
  }

  late final JSFunction _listener;
  final _pending = <String, Completer<Map<String, Object?>>>{};
  var _seq = 0;

  @override
  bool get embedded => true;

  @override
  Future<Map<String, Object?>> callServerTool(
    String name,
    Map<String, Object?> arguments,
  ) {
    final reqId = 'r${DateTime.now().millisecondsSinceEpoch}_${_seq++}';
    final completer = Completer<Map<String, Object?>>();
    _pending[reqId] = completer;
    final msg = <String, Object?>{
      'type': 'mcp:callServerTool',
      'reqId': reqId,
      'name': name,
      'arguments': arguments,
    };
    (web.window.parent ?? web.window).postMessage(msg.jsify(), '*'.toJS);
    // Defensive timeout so a missing host can't hang the UI forever.
    return completer.future.timeout(
      const Duration(seconds: 30),
      onTimeout: () {
        _pending.remove(reqId);
        throw TimeoutException('MCP host did not answer callServerTool($name)');
      },
    );
  }

  @override
  void updateModelContext(Map<String, Object?> record) {
    final msg = <String, Object?>{
      'type': 'mcp:updateModelContext',
      'record': record,
    };
    (web.window.parent ?? web.window).postMessage(msg.jsify(), '*'.toJS);
  }
}

/// Standalone transport: peacock serves this bundle and the render endpoint, so
/// a `callServerTool('render_report', …)` is a same-origin POST. Mirrors the
/// HTML runtime's standalone branch.
class _HttpFallback implements Mcp {
  @override
  bool get embedded => false;

  @override
  Future<Map<String, Object?>> callServerTool(
    String name,
    Map<String, Object?> arguments,
  ) async {
    if (name != 'render_report') {
      throw StateError('standalone peacock only serves render_report, got $name');
    }
    final body = jsonEncode(<String, Object?>{
      'report_id': arguments['report_id'],
      'params': arguments['params'],
      'png': true,
    });
    final init = web.RequestInit(
      method: 'POST',
      headers: {'content-type': 'application/json'}.jsify() as web.HeadersInit,
      body: body.toJS,
    );
    final resp = await web.window.fetch('/v1/render_report'.toJS, init).toDart;
    if (!resp.ok) {
      throw StateError('render failed (${resp.status})');
    }
    final text = (await resp.text().toDart).toDart;
    return _asStringMap(jsonDecode(text));
  }

  @override
  void updateModelContext(Map<String, Object?> record) {
    // Standalone: no model to update. Intentionally a no-op.
  }
}

Map<String, Object?> _asStringMap(Object? v) {
  if (v is Map) {
    return v.map((k, value) => MapEntry(k.toString(), value));
  }
  return <String, Object?>{};
}
