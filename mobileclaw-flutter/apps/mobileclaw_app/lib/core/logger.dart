import 'dart:io';

/// Writes log entries to a file. Thread-safe via sync write.
class FileLogger {
  FileLogger._(this._file);

  final File _file;

  static Future<FileLogger> init(String path) async {
    final file = File(path);
    await file.parent.create(recursive: true);
    // Truncate on each app start to keep log manageable.
    await file.writeAsString('', mode: FileMode.write);
    return FileLogger._(file);
  }

  void info(String msg) => _write('INFO', msg);
  void warn(String msg) => _write('WARN', msg);
  void error(String msg) => _write('ERROR', msg);

  void _write(String level, String msg) {
    final line = '${DateTime.now().toIso8601String()} [$level] $msg\n';
    _file.writeAsStringSync(line, mode: FileMode.append);
  }
}
