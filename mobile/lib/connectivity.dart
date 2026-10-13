import 'dart:async';
import 'dart:io';
import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

class StealthInternetChecker {
  final Duration checkInterval;
  final Duration timeout;
  final String? heartbeatUrl;
  final List<String> dnsTargets;
  final int dnsPort;

  late final Dio _dio;
  late final StreamController<bool> _controller;
  bool _lastStatus = false;
  Timer? _timer;

  StealthInternetChecker({
    this.heartbeatUrl,
    this.checkInterval = const Duration(seconds: 4),
    this.timeout = const Duration(seconds: 2),
    this.dnsTargets = const ['8.8.8.8', '1.1.1.1'],
    this.dnsPort = 443,
  }) {
    _dio = Dio(
      BaseOptions(
        connectTimeout: timeout,
        receiveTimeout: timeout,
        validateStatus: (status) => status != null && status < 500,
      ),
    );

    _controller = StreamController<bool>.broadcast(
      onListen: _start,
      onCancel: _stop,
    );
  }

  Stream<bool> get onStatusChange => _controller.stream;

  void _start() {
    _timer = Timer.periodic(checkInterval, (_) => _checkInternet());
    _checkInternet();
  }

  void _stop() {
    _timer?.cancel();
    _timer = null;
  }

  Future<void> _checkInternet() async {
    bool isConnected = await hasInternet();
    if (isConnected != _lastStatus) {
      _lastStatus = isConnected;
      _controller.add(isConnected);
    }
  }

  bool getLastStatus() {
    return _lastStatus;
  }

  Future<bool> hasInternet() async {
    if (heartbeatUrl != null) {
      try {
        final response = await _dio.head(heartbeatUrl!);
        if (response.statusCode != null) return true;
      } catch (e) {
        debugPrint("Error $e");
      }
    }

    return await _checkDnsConnection();
  }

  Future<bool> _checkDnsConnection() async {
    try {
      final checks = dnsTargets.map((ip) => _trySocket(ip)).toList();
      final results = await Future.wait(checks);
      return results.any((success) => success);
    } catch (_) {
      return false;
    }
  }

  Future<bool> _trySocket(String ip) async {
    try {
      final socket = await Socket.connect(ip, dnsPort, timeout: timeout);
      socket.destroy();
      return true;
    } catch (_) {
      return false;
    }
  }

  void dispose() {
    _stop();
    _controller.close();
  }
}

final connectivityProvider = StreamProvider<bool>((ref) {
  ref.onDispose(globalConnectivity.dispose);
  return globalConnectivity.onStatusChange;
});

class ConnectivityNotifier extends AsyncNotifier<bool> {
  @override
  Future<bool> build() {
    ref.listen(connectivityProvider, (_, next) {
      state = AsyncData(next.value ?? false);
    });
    return globalConnectivity.hasInternet();
  }

  Future<void> recheck() async {
    state = const AsyncLoading();
    state = AsyncData(await globalConnectivity.hasInternet());
  }
}

final connectivityNotifier = AsyncNotifierProvider<ConnectivityNotifier, bool>(
  ConnectivityNotifier.new,
);

final globalConnectivity = StealthInternetChecker(
  checkInterval: Duration(seconds: 5),
);
