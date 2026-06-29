// peacock-web — the Flutter-web iframe runtime (FR-M-1, C-4).
//
// A declarative A2UI v0.9 renderer: it fetches a peacock report artifact
// (structuredContent + a2ui + the chart PNG) and renders KPI tiles, the chart,
// and a data table as Flutter widgets. A committed category drill issues a
// fresh render (FR-M-3) — peacock stays stateless; the view state is the
// absolute parameter vector. The chart is peacock's own pure-Rust PNG, so the
// runtime needs no Node/vega CDN (NFR-S-5).
//
// Renders are routed through [Mcp] (see `lib/mcp_bridge.dart`): when embedded
// in an MCP host the app uses `callServerTool('render_report', …)` over the
// host's `postMessage` bridge and publishes committed drills via
// `updateModelContext`; standalone (opened at `/app`) it falls back to
// peacock's same-origin `POST /v1/render_report`.
//
// Built with `flutter build web` and served as static assets; no Flutter or
// Node runtime ships in the peacock alloc (NFR-O-2).

import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';

import 'mcp_bridge.dart';

/// Default report when none is named on the URL (standalone `/app`).
const defaultReportId = 'northwind-monthly-revenue';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  // Force-enable the semantics tree so the CanvasKit-rendered widgets are
  // reachable for accessibility and browser-driven tests (no CSS-selectable
  // DOM otherwise — see peacock CLAUDE.md "Browser verification").
  SemanticsBinding.instance.ensureSemantics();
  runApp(const PeacockApp());
}

class PeacockApp extends StatelessWidget {
  const PeacockApp({super.key});
  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'peacock report',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        colorSchemeSeed: const Color(0xFF0F6CBD),
        scaffoldBackgroundColor: Colors.white,
      ),
      home: const ReportView(),
    );
  }
}

class ReportView extends StatefulWidget {
  const ReportView({super.key});
  @override
  State<ReportView> createState() => _ReportViewState();
}

class _ReportViewState extends State<ReportView> {
  // The MCP-Apps transport: host bridge when embedded, same-origin fetch when
  // standalone. Chosen once for the lifetime of the app.
  final Mcp _mcp = Mcp.detect();
  // The report to render — named on the URL fragment by the shim, else default.
  final String _reportId = Mcp.reportIdFromUrl(defaultReportId);

  Map<String, dynamic>? _artifact;
  String _category = 'ALL';
  String? _error;

  @override
  void initState() {
    super.initState();
    _render(
      {'from': '1997-01-01', 'to': '1997-12-31', 'category': 'ALL'},
      commit: false,
    );
  }

  /// Render with the **absolute** parameter vector (peacock is stateless). When
  /// [commit] is set (a user-committed drill) the new view-state is also pushed
  /// to the model via `updateModelContext` (FR-X-3).
  Future<void> _render(Map<String, dynamic> params, {bool commit = false}) async {
    setState(() => _error = null);
    try {
      final data = await _mcp.callServerTool('render_report', {
        'report_id': _reportId,
        'params': params,
      });
      // MCP `tools/call` nests the artifact under structuredContent + _meta;
      // the standalone HTTP shape is flat. Normalise so the view sees one map.
      final artifact = _normalize(data);
      final sc = (artifact['structuredContent'] as Map?) ?? const {};
      final current =
          (sc['current_params'] as Map?)?['category']?.toString() ?? 'ALL';
      setState(() {
        _artifact = artifact;
        _category = current;
      });
      if (commit) {
        final rows = (sc['rows'] as List?)?.cast<Map<String, dynamic>>() ??
            const <Map<String, dynamic>>[];
        final total = rows.fold<double>(
            0, (a, r) => a + ((r['revenue'] as num?)?.toDouble() ?? 0));
        final scope = current == 'ALL' ? 'All categories' : current;
        _mcp.updateModelContext({
          'report_id': _reportId,
          'params': sc['current_params'] ?? params,
          'salient_summary': '$scope: \$${total.round()} (1997)',
        });
      }
    } catch (e) {
      setState(() => _error = '$e');
    }
  }

  /// Coerce the two result shapes into a flat artifact with `structuredContent`
  /// and a top-level `png_base64`:
  ///   * standalone HTTP : `{ structuredContent, png_base64, … }`
  ///   * MCP tools/call  : `{ structuredContent, _meta: { png_base64 } }`
  Map<String, dynamic> _normalize(Map<String, Object?> data) {
    final out = Map<String, dynamic>.from(data);
    if (out['png_base64'] == null) {
      final meta = out['_meta'];
      if (meta is Map && meta['png_base64'] != null) {
        out['png_base64'] = meta['png_base64'];
      }
    }
    return out;
  }

  void _drill(String cat) {
    _render(
      {'from': '1997-01-01', 'to': '1997-12-31', 'category': cat},
      commit: true,
    );
  }

  @override
  Widget build(BuildContext context) {
    final a = _artifact;
    return Scaffold(
      body: SafeArea(
        child: _error != null
            ? Center(child: Text('Could not load: $_error'))
            : a == null
                ? const Center(child: CircularProgressIndicator())
                : _ReportCard(artifact: a, selected: _category, onDrill: _drill),
      ),
    );
  }
}

class _ReportCard extends StatelessWidget {
  final Map<String, dynamic> artifact;
  final String selected;
  final void Function(String) onDrill;
  const _ReportCard({
    required this.artifact,
    required this.selected,
    required this.onDrill,
  });

  String _money(num n) {
    final s = n.round().toString();
    final b = StringBuffer();
    for (var i = 0; i < s.length; i++) {
      if (i > 0 && (s.length - i) % 3 == 0) b.write(',');
      b.write(s[i]);
    }
    return '\$$b';
  }

  @override
  Widget build(BuildContext context) {
    final sc = artifact['structuredContent'] as Map<String, dynamic>;
    final rows = (sc['rows'] as List).cast<Map<String, dynamic>>();
    final total = rows.fold<double>(
        0, (a, r) => a + ((r['revenue'] as num?)?.toDouble() ?? 0));
    final cats = {for (final r in rows) r['category'] as String}.toList();
    final pngB64 = artifact['png_base64'] as String?;
    final scope = selected == 'ALL' ? 'All categories' : selected;
    const allCats = [
      'ALL', 'Beverages', 'Condiments', 'Dairy Products', 'Produce', 'Seafood'
    ];

    return ListView(
      padding: const EdgeInsets.all(18),
      children: [
        Row(children: [
          Container(
            width: 26,
            height: 26,
            decoration: const BoxDecoration(
              shape: BoxShape.circle,
              gradient: SweepGradient(colors: [
                Color(0xFF1F8DF0), Color(0xFF22D3C5), Color(0xFF38D39F),
                Color(0xFFF2C14E), Color(0xFFE85AAD), Color(0xFF1F8DF0),
              ]),
            ),
          ),
          const SizedBox(width: 10),
          const Expanded(
            child: Text('Northwind revenue by category',
                style: TextStyle(fontWeight: FontWeight.w600, fontSize: 16)),
          ),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
            decoration: BoxDecoration(
              color: const Color(0xFFEEF7E8),
              borderRadius: BorderRadius.circular(999),
            ),
            child: const Text('MCP App · live',
                style: TextStyle(fontSize: 11, color: Color(0xFF3B6E22))),
          ),
        ]),
        Padding(
          padding: const EdgeInsets.only(left: 36, top: 2, bottom: 14),
          child: Text('$scope · ${rows.length} rows · live from escurel',
              style: TextStyle(color: Colors.grey[600], fontSize: 12)),
        ),
        Row(children: [
          _kpi('Total revenue', _money(total), accent: true),
          const SizedBox(width: 12),
          _kpi('Categories', '${cats.length}'),
        ]),
        const SizedBox(height: 14),
        if (pngB64 != null)
          ClipRRect(
            borderRadius: BorderRadius.circular(8),
            child: Image.memory(
                Uint8List.fromList(base64Decode(pngB64)), fit: BoxFit.fitWidth),
          ),
        const SizedBox(height: 14),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            for (final c in allCats)
              ChoiceChip(
                label: Text(c == 'ALL' ? 'All' : c),
                selected: selected == c,
                onSelected: (_) => onDrill(c),
              ),
          ],
        ),
        const SizedBox(height: 14),
        _table(rows),
      ],
    );
  }

  Widget _kpi(String label, String value, {bool accent = false}) {
    return Expanded(
      child: Container(
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(
          color: const Color(0xFFF4F9FE),
          border: Border.all(color: const Color(0xFFE4EEF8)),
          borderRadius: BorderRadius.circular(10),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label.toUpperCase(),
                style: TextStyle(
                    fontSize: 11, color: Colors.grey[600], letterSpacing: .4)),
            const SizedBox(height: 4),
            Text(value,
                style: TextStyle(
                    fontSize: 24,
                    fontWeight: FontWeight.bold,
                    color:
                        accent ? const Color(0xFF0F6CBD) : Colors.black87)),
          ],
        ),
      ),
    );
  }

  Widget _table(List<Map<String, dynamic>> rows) {
    return Container(
      decoration: BoxDecoration(
        border: Border.all(color: const Color(0xFFE1DFDD)),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        children: [
          for (final r in rows.take(40))
            Padding(
              padding:
                  const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
              child: Row(children: [
                Expanded(
                    child: Text('${r['month']}',
                        style: const TextStyle(fontSize: 13))),
                Expanded(
                    child: Text('${r['category']}',
                        style: const TextStyle(fontSize: 13))),
                Text(_money((r['revenue'] as num).toDouble()),
                    style: const TextStyle(fontSize: 13)),
              ]),
            ),
        ],
      ),
    );
  }
}
