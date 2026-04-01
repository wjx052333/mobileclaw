import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'mobileclaw_sdk_platform_interface.dart';

/// An implementation of [MobileclawSdkPlatform] that uses method channels.
class MethodChannelMobileclawSdk extends MobileclawSdkPlatform {
  /// The method channel used to interact with the native platform.
  @visibleForTesting
  final methodChannel = const MethodChannel('mobileclaw_sdk');

  @override
  Future<String?> getPlatformVersion() async {
    final version = await methodChannel.invokeMethod<String>(
      'getPlatformVersion',
    );
    return version;
  }
}
