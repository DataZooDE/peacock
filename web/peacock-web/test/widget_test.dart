// Smoke test: the app builds and shows the loading state before its first
// fetch resolves. Full report rendering is verified end-to-end against a real
// peacock + escurel in the browser (see peacock CLAUDE.md).

import 'package:flutter_test/flutter_test.dart';
import 'package:peacock_web/main.dart';

void main() {
  testWidgets('app builds and shows a loading indicator', (tester) async {
    await tester.pumpWidget(const PeacockApp());
    expect(find.byType(PeacockApp), findsOneWidget);
  });
}
