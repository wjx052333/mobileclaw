import 'package:flutter_test/flutter_test.dart';

import 'package:mobileclaw_sdk_example/main.dart';

void main() {
  testWidgets('PlaceholderApp smoke test', (WidgetTester tester) async {
    await tester.pumpWidget(const PlaceholderApp());
    expect(find.text('mobileclaw_sdk plugin example'), findsOneWidget);
  });
}
