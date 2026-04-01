import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  late Directory tmpDir;
  late MobileclawAgentImpl agent;

  // Same dev key as engine_provider.dart — not secure, test-only.
  const devKey = <int>[
    0x6d, 0x63, 0x6c, 0x61, 0x77, 0x2d, 0x64, 0x65,
    0x76, 0x2d, 0x6b, 0x65, 0x79, 0x2d, 0x33, 0x32,
    0x62, 0x79, 0x74, 0x65, 0x73, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  ];

  setUp(() async {
    tmpDir = Directory.systemTemp.createTempSync('mobileclaw_test_');
    agent = await MobileclawAgentImpl.create(
      apiKey: '',
      dbPath: '${tmpDir.path}/mem.db',
      secretsDbPath: '${tmpDir.path}/secrets.db',
      encryptionKey: devKey,
      sandboxDir: tmpDir.path,
      httpAllowlist: [],
    );
  });

  tearDown(() {
    agent.dispose();
    tmpDir.deleteSync(recursive: true);
  });

  group('email account round-trip (Android FFI)', () {
    const dto = EmailAccountDto(
      id: 'work',
      smtpHost: 'smtp.example.com',
      smtpPort: 587,
      imapHost: 'imap.example.com',
      imapPort: 993,
      username: 'alice@example.com',
    );
    const password = 'hunter2';

    testWidgets('save then load returns correct dto', (tester) async {
      await agent.emailAccountSave(dto: dto, password: password);

      final loaded = await agent.emailAccountLoad(id: 'work');

      expect(loaded, isNotNull);
      expect(loaded!.id, 'work');
      expect(loaded.smtpHost, 'smtp.example.com');
      expect(loaded.smtpPort, 587);
      expect(loaded.imapHost, 'imap.example.com');
      expect(loaded.imapPort, 993);
      expect(loaded.username, 'alice@example.com');
    });

    testWidgets('password not exposed after save', (tester) async {
      await agent.emailAccountSave(dto: dto, password: password);

      final loaded = await agent.emailAccountLoad(id: 'work');

      expect(loaded.toString(), isNot(contains(password)));
    });

    testWidgets('load returns null for unknown id', (tester) async {
      final result = await agent.emailAccountLoad(id: 'nonexistent');
      expect(result, isNull);
    });

    testWidgets('delete removes account', (tester) async {
      await agent.emailAccountSave(dto: dto, password: password);
      await agent.emailAccountDelete(id: 'work');

      final result = await agent.emailAccountLoad(id: 'work');
      expect(result, isNull);
    });

    testWidgets('delete is idempotent', (tester) async {
      await expectLater(
        agent.emailAccountDelete(id: 'nonexistent'),
        completes,
      );
    });

    testWidgets('overwrite existing account', (tester) async {
      await agent.emailAccountSave(dto: dto, password: password);

      const updated = EmailAccountDto(
        id: 'work',
        smtpHost: 'smtp.updated.com',
        smtpPort: 465,
        imapHost: 'imap.updated.com',
        imapPort: 993,
        username: 'alice@example.com',
      );
      await agent.emailAccountSave(dto: updated, password: 'newpass');

      final loaded = await agent.emailAccountLoad(id: 'work');
      expect(loaded!.smtpHost, 'smtp.updated.com');
      expect(loaded.smtpPort, 465);
    });

    testWidgets('multiple accounts stored independently', (tester) async {
      const personal = EmailAccountDto(
        id: 'personal',
        smtpHost: 'smtp.personal.com',
        smtpPort: 587,
        imapHost: 'imap.personal.com',
        imapPort: 993,
        username: 'bob@personal.com',
      );

      await agent.emailAccountSave(dto: dto, password: 'pass1');
      await agent.emailAccountSave(dto: personal, password: 'pass2');

      final work = await agent.emailAccountLoad(id: 'work');
      final pers = await agent.emailAccountLoad(id: 'personal');

      expect(work!.username, 'alice@example.com');
      expect(pers!.username, 'bob@personal.com');
    });

    testWidgets('deleting one account does not affect others', (tester) async {
      const personal = EmailAccountDto(
        id: 'personal',
        smtpHost: 'smtp.personal.com',
        smtpPort: 587,
        imapHost: 'imap.personal.com',
        imapPort: 993,
        username: 'bob@personal.com',
      );

      await agent.emailAccountSave(dto: dto, password: 'pass1');
      await agent.emailAccountSave(dto: personal, password: 'pass2');
      await agent.emailAccountDelete(id: 'work');

      expect(await agent.emailAccountLoad(id: 'work'), isNull);
      expect((await agent.emailAccountLoad(id: 'personal'))!.username,
          'bob@personal.com');
    });
  });
}
