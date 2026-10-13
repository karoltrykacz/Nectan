import 'dart:async';
import 'package:nectan_mobile/connectivity.dart';
import 'dart:core';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:nectan_mobile/add_remote.dart';
import 'package:nectan_mobile/elements.dart';
import 'package:nectan_mobile/main.dart';
import 'package:nectan_mobile/media_picker.dart';
import 'package:nectan_mobile/src/rust/api/simple.dart';
import 'package:file_picker/file_picker.dart';
import 'package:nectan_mobile/transfers.dart';
import 'package:permission_handler/permission_handler.dart';
import 'package:photo_manager/photo_manager.dart';
import 'package:device_info_plus/device_info_plus.dart';

final transferHistoryProvider = Provider.family<List<TransferMetaDto>, String>(
  (ref, deviceId) => ref
      .watch(transfersPoolProvider)
      .where((t) => t.remoteDeviceId == deviceId)
      .toList(),
);

class DevicePoolNotifier extends Notifier<List<DeviceDto>> {
  StreamSubscription<List<DeviceDto>>? _sub;

  @override
  List<DeviceDto> build() {
    ref.onDispose(() {
      _sub?.cancel();
    });
    return [];
  }

  void setStream(Stream<List<DeviceDto>> stream) async {
    debugPrint('💙 Devices stream sink was set.');
    _sub?.cancel();
    _listenTo(stream);
    state = await getDtoDevices();
  }

  void _listenTo(Stream<List<DeviceDto>> stream) {
    _sub = stream.listen((devices) => state = devices);
  }

  Future<void> removeDevice(String id) async {
    debugPrint('🟦 Deleted device.');
    state = state.where((d) => d.id != id).toList();
    await deleteDevice(deviceId: id);
  }

  Future<void> refreshDevices() async {
    debugPrint('🟦 Refreshing devices.');

    final devices = await getDtoDevices();
    state = devices;
    debugPrint('🟦 Got ${devices.length} device(s).');
  }
}

final devicePoolProvider =
    NotifierProvider<DevicePoolNotifier, List<DeviceDto>>(
      DevicePoolNotifier.new,
    );

class DevicesScreen extends StatefulWidget {
  final GlobalKey<NavigatorState> navigatorKey;
  const DevicesScreen({super.key, required this.navigatorKey});

  @override
  State<DevicesScreen> createState() => _DevicesScreenState();
}

class _DevicesScreenState extends State<DevicesScreen> {
  late final _observer = _RouteObserver(onChanged: () => setState(() {}));

  @override
  Widget build(BuildContext context) {
    final canNestedPop = widget.navigatorKey.currentState?.canPop() ?? false;
    return PopScope(
      canPop: !canNestedPop,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop && canNestedPop) widget.navigatorKey.currentState?.pop();
      },
      child: Navigator(
        key: widget.navigatorKey,
        observers: [_observer],
        onGenerateRoute: (settings) =>
            FastRoute(builder: (_) => const _DevicesListContent()),
      ),
    );
  }
}

class _RouteObserver extends NavigatorObserver {
  final VoidCallback onChanged;
  _RouteObserver({required this.onChanged});

  void _notify() =>
      WidgetsBinding.instance.addPostFrameCallback((_) => onChanged());

  @override
  void didPush(Route r, Route? p) => _notify();
  @override
  void didPop(Route r, Route? p) => _notify();
  @override
  void didRemove(Route r, Route? p) => _notify();
}

class RippleBg extends StatefulWidget {
  final Widget child;
  final Duration interval;
  final int initialRings;
  final Color color;
  final Duration duration;

  const RippleBg({
    super.key,
    required this.child,
    this.interval = const Duration(milliseconds: 6000),
    this.initialRings = 5,
    this.color = const Color(0x37787878),
    this.duration = const Duration(milliseconds: 6000),
  });

  @override
  State<RippleBg> createState() => _RippleBgState();
}

class _RippleBgState extends State<RippleBg> with TickerProviderStateMixin {
  final List<AnimationController> _controllers = [];
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    for (int i = 0; i < widget.initialRings; i++) {
      Future.delayed(widget.interval * i, () {
        if (mounted) _spawnRing();
      });
    }
    _timer = Timer.periodic(widget.interval, (_) {
      if (mounted) _spawnRing();
    });
  }

  void _spawnRing() {
    final ctrl = AnimationController(vsync: this, duration: widget.duration);
    _controllers.add(ctrl);
    ctrl.forward();
    ctrl.addStatusListener((status) {
      if (status == AnimationStatus.completed) {
        ctrl.dispose();
        _controllers.remove(ctrl);
      }
    });
    setState(() {});
  }

  @override
  void dispose() {
    _timer?.cancel();
    for (final c in _controllers) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: Listenable.merge(_controllers),
      builder: (context, child) {
        return CustomPaint(
          painter: _RipplePainter(_controllers, widget.color),
          child: child,
        );
      },
      child: widget.child,
    );
  }
}

class _RipplePainter extends CustomPainter {
  final List<AnimationController> controllers;
  final Color color;

  _RipplePainter(this.controllers, this.color);

  @override
  void paint(Canvas canvas, Size size) {
    final center = Offset(size.width / 2, size.height / 2);
    final maxRadius = size.longestSide * 0.8;

    for (final ctrl in controllers) {
      final t = ctrl.value;
      final radius = maxRadius * t;
      final opacity = t < 0.3 ? (t / 0.3) : (1.0 - (t - 0.3) / 0.7);
      final paint = Paint()
        ..color = color.withOpacity(color.opacity * opacity)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.5;

      canvas.drawCircle(center, radius, paint);
    }
  }

  @override
  bool shouldRepaint(_RipplePainter old) => true;
}

class FoundNearbyScreen extends StatefulWidget {
  final DeviceDto device;
  const FoundNearbyScreen({super.key, required this.device});

  @override
  State<FoundNearbyScreen> createState() => _FoundNearbyScreenState();
}

class _FoundNearbyScreenState extends State<FoundNearbyScreen>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller;

  bool _dontShowAgain = false;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 200),
    )..forward();
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final deviceName = widget.device.username;
    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            children: [
              const Spacer(flex: 3),
              FadeTransition(
                opacity: _controller,
                child: SlideTransition(
                  position:
                      Tween<Offset>(
                        begin: const Offset(0, 0.05),
                        end: Offset.zero,
                      ).animate(
                        CurvedAnimation(
                          parent: _controller,
                          curve: Curves.easeOut,
                        ),
                      ),
                  child: Column(
                    children: [
                      ShaderMask(
                        shaderCallback: (bounds) => LinearGradient(
                          colors: [
                            context.colors.primaryFont,
                            context.colors.primaryFont,
                          ],
                        ).createShader(bounds),
                        child: Text(
                          "$deviceName\n found nearby",
                          textAlign: TextAlign.center,
                          style: TextStyle(
                            color: context.colors.primaryFont,
                            fontSize: 30,
                            fontWeight: FontWeight.w600,
                            fontFamily: "Rubik",
                          ),
                        ),
                      ),
                      const SizedBox(height: 14),
                      Text(
                        "Found nearby device with Nectan.\nYou can now send files.",
                        textAlign: TextAlign.center,
                        style: TextStyle(
                          fontFamily: "Rubik",
                          color: context.colors.secondaryFont,
                          fontSize: 15,
                          height: 1.5,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              const Spacer(flex: 3),
              SizedBox(
                width: double.infinity,
                child: PrimaryWideButton(
                  label: "Continue",
                  onPressed: () => Navigator.of(context).pop(true),
                ),
              ),
              const SizedBox(height: 42),
              GestureDetector(
                onTap: () {
                  setShowConnectedToNearby(showAgain: !_dontShowAgain);
                  setState(() => _dontShowAgain = !_dontShowAgain);
                },
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    AnimatedContainer(
                      duration: const Duration(milliseconds: 100),
                      width: 20,
                      height: 20,
                      decoration: BoxDecoration(
                        color: _dontShowAgain
                            ? context.colors.primaryFont
                            : Colors.transparent,
                        border: Border.all(
                          color: _dontShowAgain
                              ? context.colors.primaryCol
                              : context.colors.classicBorderActive,
                          width: 1,
                        ),
                        borderRadius: BorderRadius.circular(6),
                      ),
                      child: _dontShowAgain
                          ? Icon(
                              Icons.check,
                              size: 14,
                              color: context.colors.primaryCol,
                            )
                          : null,
                    ),
                    const SizedBox(width: 10),
                    Text(
                      "Don't show again",
                      style: TextStyle(
                        fontFamily: "Rubik",
                        color: context.colors.secondaryFont,
                        fontSize: 14,
                        fontWeight: FontWeight(400),
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 24),
            ],
          ),
        ),
      ),
    );
  }
}

class _DevicesListContent extends ConsumerStatefulWidget {
  const _DevicesListContent();

  @override
  ConsumerState<_DevicesListContent> createState() =>
      _DevicesListContentState();
}

class _DevicesListContentState extends ConsumerState<_DevicesListContent> {
  @override
  void initState() {
    super.initState();
  }

  @override
  void dispose() {
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final devices = ref.watch(devicePoolProvider);
    final isOnline = globalConnectivity.getLastStatus();

    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 24, 20, 16),
              child: SizedBox(
                height: 48,
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    GestureDetector(
                      onTap: () {},
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.center,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            'Devices',
                            style: TextStyle(
                              fontFamily: 'Rubik',
                              fontSize: 30,
                              fontWeight: FontWeight.w600,
                              color: context.colors.primaryFont,
                              height: 0.9,
                            ),
                          ),
                          Text(
                            isOnline ? 'Online Mode' : 'Local Mode',
                            style: TextStyle(
                              fontFamily: 'Rubik',
                              fontSize: 13,
                              fontWeight: FontWeight.w500,
                              color: context.colors.secondaryFont,
                            ),
                          ),
                        ],
                      ),
                    ),
                    Row(
                      children: [
                        IconButton(
                          onPressed: () => Navigator.of(
                            context,
                            rootNavigator: true,
                          ).push(FastRoute(builder: (_) => const CodeScreen())),
                          icon: Icon(
                            Icons.add_rounded,
                            color: context.colors.primaryCol,
                            size: 24,
                          ),
                          style: ButtonStyle(
                            backgroundColor: WidgetStateProperty.all(
                              context.colors.primaryFont,
                            ),
                            shape: WidgetStateProperty.all(
                              RoundedRectangleBorder(
                                borderRadius: BorderRadius.circular(12),
                              ),
                            ),
                          ),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 12),

            Expanded(
              child: devices.isEmpty
                  ? Stack(
                      children: [
                        const RippleBg(child: SizedBox.expand()),
                        Center(
                          child: Column(
                            mainAxisAlignment: MainAxisAlignment.center,
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Text(
                                'No devices',
                                style: TextStyle(
                                  color: context.colors.secondaryFont,
                                  fontFamily: "Rubik",
                                ),
                              ),
                            ],
                          ),
                        ),
                      ],
                    )
                  : Align(
                      alignment: Alignment.topCenter,
                      child: ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 800),
                        child: RefreshIndicator(
                          color: context.colors.primaryFont,
                          onRefresh: () async {
                            await ref
                                .read(devicePoolProvider.notifier)
                                .refreshDevices();
                          },
                          child: CustomScrollView(
                            slivers: [
                              SliverPadding(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 20,
                                ),
                                sliver: SliverList.separated(
                                  itemCount: devices.length,
                                  separatorBuilder: (_, _) =>
                                      const SizedBox(height: 8),
                                  itemBuilder: (context, i) {
                                    final device = devices[i];
                                    return _DeviceTile(device: device);
                                  },
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

class _OfflineSheet extends StatelessWidget {
  final DeviceDto device;
  const _OfflineSheet({required this.device});

  @override
  Widget build(BuildContext context) {
    return Material(
      color: context.colors.primaryCol,
      borderRadius: const BorderRadius.vertical(top: Radius.circular(8)),
      child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(64, 64, 64, 64),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const SizedBox(height: 24),
              Icon(
                Icons.wifi_off_rounded,
                size: 40,
                color: context.colors.primaryFont,
              ),
              const SizedBox(height: 12),
              Text(
                '${device.username} is offline',
                style: TextStyle(
                  fontFamily: 'Rubik',
                  fontWeight: FontWeight.w600,
                  fontSize: 18,
                  color: context.colors.primaryFont,
                ),
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 12),
              Text(
                'This device is not reachable right now.',
                textAlign: TextAlign.center,
                style: TextStyle(
                  color: context.colors.secondaryFont,
                  fontSize: 14,
                  fontFamily: 'Rubik',
                ),
              ),
              const SizedBox(height: 24),
            ],
          ),
        ),
      ),
    );
  }
}

class ExpandableTip extends StatefulWidget {
  final String title;
  final String tip;
  final bool expandable;

  const ExpandableTip({
    super.key,
    required this.title,
    required this.tip,
    this.expandable = false,
  });

  @override
  State<ExpandableTip> createState() => _ExpandableTipState();
}

class _ExpandableTipState extends State<ExpandableTip> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    // if not expandable, tip always show (old behavior)
    final bool showTip = !widget.expandable || _expanded;

    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: widget.expandable
          ? () => setState(() => _expanded = !_expanded)
          : null,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: BoxDecoration(
          color: context.colors.secondaryCol,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: context.colors.classicBorder),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.only(top: 2),
              child: Icon(
                Icons.info_outline_rounded,
                size: 17,
                color: context.colors.thirdFont,
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          widget.title,
                          style: TextStyle(
                            fontFamily: 'Rubik',
                            fontSize: 14,
                            fontWeight: FontWeight.w500,
                            color: context.colors.primaryFont,
                          ),
                        ),
                      ),
                      if (widget.expandable) ...[
                        const SizedBox(width: 10, height: 0),
                        Icon(
                          _expanded
                              ? Icons.keyboard_arrow_up_rounded
                              : Icons.keyboard_arrow_down_rounded,
                          size: 18,
                          color: context.colors.thirdFont,
                        ),
                      ],
                    ],
                  ),
                  AnimatedSize(
                    duration: const Duration(milliseconds: 100),
                    curve: Curves.easeOut,
                    alignment: Alignment.topLeft,
                    clipBehavior: Clip.hardEdge,
                    child: ClipRect(
                      child: Align(
                        alignment: Alignment.topLeft,
                        heightFactor: 1,
                        child: showTip
                            ? Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  const SizedBox(height: 3),
                                  Text(
                                    widget.tip,
                                    style: TextStyle(
                                      fontFamily: 'Rubik',
                                      fontSize: 13,
                                      height: 1.3,
                                      color: context.colors.secondaryFont,
                                    ),
                                  ),
                                ],
                              )
                            : const SizedBox(width: double.infinity, height: 0),
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

Widget buildTip(String title, String tip, [bool expandable = false]) {
  return ExpandableTip(title: title, tip: tip, expandable: expandable);
}

class _DeviceTile extends ConsumerWidget {
  final DeviceDto device;
  // final VoidCallback? onTap;
  const _DeviceTile({required this.device});

  String get _statusLabel => switch (device.status) {
    DeviceStatus.online => 'Online',
    DeviceStatus.localNetwork => 'Nearby',
    DeviceStatus.offline => 'Offline',
  };

  Color get _statusColor => switch (device.status) {
    DeviceStatus.online => const Color(0xFF23a3ec),
    DeviceStatus.localNetwork => const Color(0xFF23a3ec),
    DeviceStatus.offline => const Color(0xFF9E9E9E),
  };

  void _showSendOptions(BuildContext context, WidgetRef ref) {
    debugPrint("Clicked on ${device.username}");
    if (device.status == DeviceStatus.offline) {
      showModalBottomSheet(
        context: context,
        backgroundColor: Colors.transparent,
        useRootNavigator: true,
        sheetAnimationStyle: AnimationStyle(
          duration: const Duration(milliseconds: 150),
        ),
        builder: (_) => _OfflineSheet(device: device),
      );
      return;
    }

    if (device.status == DeviceStatus.online ||
        device.status == DeviceStatus.localNetwork) {
      _openSendSheet(context, ref);
    }
  }

  void _openSendSheet(BuildContext context, WidgetRef ref) async {
    bool granted = true;

    if (!Platform.isLinux) {
      final status = await Permission.manageExternalStorage.status;
      if (!context.mounted) return;
      granted = status == PermissionStatus.granted;
      if (!granted) {
        granted = await showStoragePermissionDialog(context, status);
      }
    }

    if (granted && context.mounted) {
      showModalBottomSheet(
        context: context,
        backgroundColor: Colors.transparent,
        useRootNavigator: true,
        sheetAnimationStyle: AnimationStyle(
          duration: const Duration(milliseconds: 150),
        ),
        builder: (_) => SendOptionsSheet(device: device, ref: ref),
      );
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return SizedBox(
      height: 72,
      child: Material(
        borderRadius: BorderRadius.circular(12),
        color: context.colors.secondaryCol,
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onLongPress: () => _showDeviceOptions(context, device),
          onTap: () {
            _showSendOptions(context, ref);
          },
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        device.username,
                        style: TextStyle(
                          color: context.colors.primaryFont,
                          fontWeight: FontWeight.w600,
                          fontSize: 14,
                          fontFamily: "Rubik",
                        ),
                      ),
                      const SizedBox(height: 3),
                      Row(
                        children: [
                          Text(
                            _statusLabel,
                            style: TextStyle(
                              color: _statusColor,
                              fontSize: 12,
                              fontWeight: FontWeight.w600,
                              fontFamily: "Rubik",
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
                Icon(
                  Icons.chevron_right,
                  color: context.colors.thirdFont,
                  size: 20,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

void _showDeviceOptions(BuildContext context, DeviceDto device) {
  showModalBottomSheet(
    context: context,
    useRootNavigator: true,
    backgroundColor: Colors.transparent,
    builder: (s) => _DeviceOptionsSheet(device: device),
  );
}

class _DeviceOptionsSheet extends ConsumerWidget {
  final DeviceDto device;
  const _DeviceOptionsSheet({required this.device});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Material(
      color: context.colors.primaryCol,
      borderRadius: const BorderRadius.vertical(top: Radius.circular(20)),
      child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 36,
                height: 4,
                margin: const EdgeInsets.only(bottom: 16),
                decoration: BoxDecoration(
                  color: context.colors.primaryFont,
                  borderRadius: BorderRadius.circular(2),
                ),
              ),
              Text(
                device.username,
                style: TextStyle(
                  fontFamily: 'Rubik',
                  fontWeight: FontWeight.w600,
                  fontSize: 16,
                  color: context.colors.primaryFont,
                ),
              ),
              const SizedBox(height: 16),
              ListTile(
                title: const Text(
                  'Remove device',
                  style: TextStyle(
                    fontWeight: FontWeight(500),
                    fontFamily: "Rubik",
                    color: Colors.redAccent,
                  ),
                ),
                onTap: () {
                  Navigator.pop(context);
                  ref.read(devicePoolProvider.notifier).removeDevice(device.id);
                },
              ),
            ],
          ),
        ),
      ),
    );
  }
}

Future<void> requestStoragePermissions() async {
  if (!Platform.isAndroid) return;
  debugPrint("🟧 Requesting storage permission.");

  final androidInfo = await DeviceInfoPlugin().androidInfo;
  final permission = androidInfo.version.sdkInt >= 30
      ? Permission.manageExternalStorage
      : Permission.storage;

  final status = await permission.status;

  if (status.isGranted) {
    debugPrint("🟧 GRANTED!");
    return;
  }

  if (status.isPermanentlyDenied) {
    debugPrint("🟧 PERMANENTLY DENIED - open settings");
    await openAppSettings();
    return;
  }

  // Just request! No shouldShowRvalueequestRationale check needed here
  final result = await permission.request();

  if (result.isGranted) {
    debugPrint("🟧 GRANTED!");
  } else if (result.isPermanentlyDenied) {
    debugPrint("🟧 PERMANENTLY DENIED - open settings");
    await openAppSettings();
  } else {
    debugPrint("🟧 DENIED");
  }
}

class SendOptionsSheet extends StatelessWidget {
  final DeviceDto device;
  final WidgetRef ref;
  const SendOptionsSheet({super.key, required this.device, required this.ref});

  static const List<_SendOption> _options = [
    _SendOption(icon: Icons.videocam_outlined, label: 'Videos'),
    _SendOption(icon: Icons.photo_library_outlined, label: 'Photos'),
    _SendOption(icon: Icons.insert_drive_file_outlined, label: 'Files'),
    _SendOption(icon: Icons.insert_drive_file_outlined, label: 'Audio'),
    _SendOption(icon: Icons.folder_outlined, label: 'Folder'),
  ];

  Future<void> _handleOption(BuildContext context, String label) async {
    await requestStoragePermissions();
    switch (label) {
      case 'Videos':
      case 'Photos':
        if (!context.mounted) return;

        final targetType = (label == 'Videos')
            ? RequestType.video
            : RequestType.image;

        final List<AssetEntity>? selectedAssets =
            await Navigator.push<List<AssetEntity>>(
              context,
              FastRoute(
                builder: (_) => MediaPickerScreen(
                  mediaType: targetType,
                  onDone: (assets) => Navigator.pop(context, assets),
                ),
              ),
            );

        if (selectedAssets == null || selectedAssets.isEmpty) return;
        if (!context.mounted) return;

        // TODO

        // Navigator.push(
        //   context,
        //   FastRoute(
        //     builder: (_) => PhotosVideosFilesOfferScreen(
        //       assets: selectedAssets,
        //       remoteDevice: device,
        //       files: null,
        //     ),
        //   ),
        // );
        break;
      case 'Audio':
        if (!context.mounted) return;
        final List<AssetEntity>? selectedAudio =
            await Navigator.push<List<AssetEntity>>(
              context,
              FastRoute(
                builder: (_) => MediaPickerScreen(
                  mediaType: RequestType.audio,
                  onDone: (assets) => Navigator.pop(context, assets),
                ),
              ),
            );
        if (selectedAudio == null || selectedAudio.isEmpty) return;
        if (!context.mounted) return;
        // Navigator.push(
        //   context,
        //   FastRoute(
        //     builder: (_) => PhotosVideosFilesOfferScreen(
        //       assets: selectedAudio,
        //       remoteDevice: device,
        //       files: null,
        //     ),
        //   ),
        // );
        throw Exception("Todo");
        break;

      case 'Files':
        try {
          final result = await FilePicker.platform.pickFiles(
            allowMultiple: true,
          );
          if (result == null || result.files.isEmpty) return;
          // path empry on web platrom but builiding for anoridn andios:w
          final files = result.files.where((f) => f.path != null).toList();
          if (files.isEmpty) return;
          if (!context.mounted) return;

          throw Exception("Todo");
          // Navigator.push(
          //   context,
          //   FastRoute(
          //     builder: (_) => PhotosVideosFilesOfferScreen(
          //       assets: null,
          //       files: files,
          //       remoteDevice: device,
          //     ),
          //   ),
          // );
        } catch (e) {
          debugPrint('🟥 Files error: $e');
        }
        break;

      case 'Folder':
        debugPrint("🟦Selecting folder");

        final result = await FilePicker.platform.getDirectoryPath();
        if (result == null) return;
        debugPrint("🟦 Selected folder $result");

        final transferName = await genTransferName();

        if (!context.mounted) return;
        Navigator.of(context).push(
          FastRoute(
            builder: (_) => WalkDirScreen(
              paths: [result],
              deviceId: device.id,
              transferName: transferName,
            ),
          ),
        );
        break;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Material(
      color: context.colors.primaryCol,
      borderRadius: const BorderRadius.vertical(top: Radius.circular(8)),
      child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const SizedBox(height: 16),
              Text(
                'Send',
                style: TextStyle(
                  fontFamily: 'Rubik',
                  fontWeight: FontWeight.w500,
                  fontSize: 18,
                  color: context.colors.primaryFont,
                ),
              ),
              const SizedBox(height: 16),
              ..._options.map(
                (opt) => _OptionTile(
                  option: opt,
                  onTap: () => _handleOption(context, opt.label),
                ),
              ),
              const SizedBox(height: 16),
            ],
          ),
        ),
      ),
    );
  }
}

class _SendOption {
  final IconData icon;
  final String label;
  const _SendOption({required this.icon, required this.label});
}

class _OptionTile extends StatelessWidget {
  final _SendOption option;
  final VoidCallback onTap;
  const _OptionTile({required this.option, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ListTile(
      title: Text(
        option.label,
        style: TextStyle(
          fontWeight: FontWeight.w500,
          fontSize: 15,
          color: scheme.onSurface,
          fontFamily: "Inter",
        ),
      ),
      trailing: Icon(
        Icons.chevron_right,
        color: scheme.onSurface.withAlpha(100),
        size: 18,
      ),
      onTap: onTap,
    );
  }
}

class WalkDirScreen extends StatefulWidget {
  const WalkDirScreen({
    super.key,
    required this.deviceId,
    required this.transferName,
    required this.paths,
  });

  final String deviceId;
  final String transferName;
  final List<String> paths;

  @override
  State<WalkDirScreen> createState() => _WalkDirScreenState();
}

class _WalkDirScreenState extends State<WalkDirScreen> {
  StreamSubscription<WalkInfo>? _sub;
  WalkInfo? _info;
  Object? _streamError;

  @override
  void initState() {
    super.initState();
    _start();
  }

  void _start() {
    _sub?.cancel();
    setState(() {
      _info = null;
      _streamError = null;
    });
    _sub =
        send(
          remoteDeviceId: widget.deviceId,
          transferName: widget.transferName,
          paths: widget.paths,
        ).listen(
          (info) => setState(() => _info = info),
          onError: (e) => setState(() => _streamError = e),
        );
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = _streamError != null ? SendState.failed : _info?.state;

    return Scaffold(
      appBar: AppBar(
        backgroundColor: context.colors.primaryCol,
        title: Text(
          widget.transferName,
          style: TextStyle(
            fontSize: 30,
            fontWeight: FontWeight(600),
            fontFamily: "Rubik",
          ),
        ),
      ),
      backgroundColor: context.colors.primaryCol,
      body: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                _title(state),
                style: TextStyle(
                  fontSize: 20,
                  fontFamily: "Rubik",
                  fontWeight: FontWeight(500),
                ),
              ),

              const SizedBox(height: 8),
              if (_info != null)
                Text(
                  '${_info!.files} files    ${_formatSize(_info!.totalSize.toInt())}',
                  style: TextStyle(
                    fontSize: 14,
                    fontFamily: "Rubik",
                    fontWeight: FontWeight(500),
                  ),
                ),
              const SizedBox(height: 20),
              _StatusIcon(state: state),
              const SizedBox(height: 72),
              _Actions(
                state: state,
                onRetry: _start,
                onClose: () => Navigator.of(context).pop(),
              ),
            ],
          ),
        ),
      ),
    );
  }

  String _title(SendState? s) => switch (s) {
    null || SendState.scanning => 'Scanning folder.',
    SendState.sending => 'Waiting for the other device to accept.',
    SendState.accepted => 'Accepted. Sending...',
    SendState.rejected => 'Remote device declined the transfer',
    SendState.failed =>
      'Something went wrong. Make sure you have Internet connection.',
  };
}

class _StatusIcon extends StatelessWidget {
  const _StatusIcon({required this.state});
  final SendState? state;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return switch (state) {
      null || SendState.scanning || SendState.sending => const Spinner(),
      SendState.accepted => const SizedBox.shrink(),
      SendState.rejected => Icon(Icons.block, size: 56, color: scheme.error),
      SendState.failed => Icon(
        Icons.error_outline,
        size: 56,
        color: context.colors.primaryFont,
      ),
    };
  }
}

class _Actions extends StatelessWidget {
  const _Actions({
    required this.state,
    required this.onRetry,
    required this.onClose,
  });
  final SendState? state;
  final VoidCallback onRetry;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    return switch (state) {
      null || SendState.scanning || SendState.sending => TextButton(
        onPressed: onClose,
        child: const Text('Cancel'),
      ),
      SendState.accepted => PrimaryWideButton(
        onPressed: onClose,
        label: "Done",
      ),
      SendState.rejected || SendState.failed => Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextButton(
            onPressed: onClose,
            child: Text(
              'Close',
              style: TextStyle(
                fontFamily: "Rubik",
                color: context.colors.primaryFont,
                fontSize: 13,
              ),
            ),
          ),
          const SizedBox(width: 8),
          FilledButton(onPressed: onRetry, child: const Text('Try again')),
        ],
      ),
    };
  }
}

String _formatSize(int bytes) {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var v = bytes.toDouble();
  var i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return '${v.toStringAsFixed(i == 0 ? 0 : 1)} ${units[i]}';
}

class _SectionLabel extends StatelessWidget {
  final String text;
  const _SectionLabel(this.text);

  @override
  Widget build(BuildContext context) => Text(
    text,
    style: TextStyle(
      fontSize: 13,
      fontWeight: FontWeight.w600,
      color: context.colors.secondaryFont,
    ),
  );
}

class _ToggleRow extends StatelessWidget {
  final IconData icon;
  final String title;
  final String subtitle;
  final bool selected;
  final VoidCallback onTap;

  const _ToggleRow({
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
        decoration: BoxDecoration(
          color: selected
              ? context.colors.thirdCol
              : context.colors.secondaryCol,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: selected
                ? context.colors.classicBorderActive
                : Colors.transparent,
            width: 1,
          ),
        ),
        child: Row(
          children: [
            Icon(icon, size: 20, color: context.colors.secondaryFont),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: TextStyle(
                      fontSize: 15,
                      fontWeight: FontWeight.w500,
                      color: context.colors.primaryFont,
                    ),
                  ),
                  Text(
                    subtitle,
                    style: TextStyle(
                      fontSize: 12,
                      color: context.colors.thirdFont,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
