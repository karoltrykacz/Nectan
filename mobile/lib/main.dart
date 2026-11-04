import 'dart:collection';
import 'dart:ui';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_foreground_task/flutter_foreground_task.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:media_scanner/media_scanner.dart';
import 'package:nectan_mobile/add_remote.dart';
import 'package:nectan_mobile/devices.dart';
import 'package:nectan_mobile/elements.dart';
import 'package:nectan_mobile/connectivity.dart';
import 'package:nectan_mobile/src/rust/api/simple.dart';
import 'package:nectan_mobile/src/rust/frb_generated.dart';
import 'package:flutter/cupertino.dart';
import 'package:nectan_mobile/transfers.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'dart:async';
import 'package:path_provider/path_provider.dart';
import 'dart:io';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:nectan_mobile/settings.dart' hide sq;
import 'package:permission_handler/permission_handler.dart';
import 'package:nectan_mobile/navigation.dart';
import 'package:nectan_mobile/themes.dart';

@immutable
class AppColors extends ThemeExtension<AppColors> {
  final Color primaryCol;
  final Color secondaryCol;
  final Color thirdCol;
  final Color primaryFont;
  final Color secondaryFont;
  final Color thirdFont;
  final Color classicBorder;
  final Color classicBorderActive;

  const AppColors({
    required this.primaryCol,
    required this.secondaryCol,
    required this.thirdCol,
    required this.primaryFont,
    required this.secondaryFont,
    required this.thirdFont,
    required this.classicBorder,
    required this.classicBorderActive,
  });

  @override
  AppColors copyWith({
    Color? primaryCol,
    Color? secondaryCol,
    Color? thirdCol,
    Color? primaryFont,
    Color? secondaryFont,
    Color? thirdFont,
    Color? classicBorder,
    Color? classicBorderActive,
  }) {
    return AppColors(
      primaryCol: primaryCol ?? this.primaryCol,
      secondaryCol: secondaryCol ?? this.secondaryCol,
      thirdCol: thirdCol ?? this.thirdCol,
      primaryFont: primaryFont ?? this.primaryFont,
      secondaryFont: secondaryFont ?? this.secondaryFont,
      thirdFont: thirdFont ?? this.thirdFont,
      classicBorder: classicBorder ?? this.classicBorder,
      classicBorderActive: classicBorderActive ?? this.classicBorderActive,
    );
  }

  @override
  AppColors lerp(covariant ThemeExtension<AppColors>? other, double t) {
    if (other is! AppColors) return this;
    return AppColors(
      primaryCol: Color.lerp(primaryCol, other.primaryCol, t)!,
      secondaryCol: Color.lerp(secondaryCol, other.secondaryCol, t)!,
      thirdCol: Color.lerp(thirdCol, other.thirdCol, t)!,
      primaryFont: Color.lerp(primaryFont, other.primaryFont, t)!,
      secondaryFont: Color.lerp(secondaryFont, other.secondaryFont, t)!,
      thirdFont: Color.lerp(thirdFont, other.thirdFont, t)!,
      classicBorder: Color.lerp(classicBorder, other.classicBorder, t)!,
      classicBorderActive: Color.lerp(
        classicBorderActive,
        other.classicBorderActive,
        t,
      )!,
    );
  }
}

const lightAppColors = AppColors(
  primaryCol: Color(0xfff1f1f1),
  secondaryCol: Color(0xffdddddd),
  thirdCol: Color(0xffcccccc),
  primaryFont: Color(0xff292524),
  secondaryFont: Color(0xff3e3938),
  thirdFont: Color(0xff555555),
  classicBorder: Color(0x1B000000),
  classicBorderActive: Color(0x47000000),
);

const darkAppColors = AppColors(
  primaryCol: Color(0xff1a1a1a),
  secondaryCol: Color(0xff1f1f1f),
  thirdCol: Color(0xff272727),
  primaryFont: Color(0xfff1f1f1),
  secondaryFont: Color(0xff9e9e9e),
  thirdFont: Color(0xff757575),
  classicBorder: Color(0x1BFFFFFF),
  classicBorderActive: Color(0x47FFFFFF),
);

// Themes (palettes, provider, picker) live in themes.dart.

extension AppColorsX on BuildContext {
  AppColors get colors => Theme.of(this).extension<AppColors>()!;
}

class MyApp extends ConsumerWidget {
  final bool isFirstLaunch;
  const MyApp({super.key, required this.isFirstLaunch});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(connectivityNotifier);
    final selected = ref.watch(appThemeProvider);
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      // "System" gets both light and dark; every other theme is used as-is.
      theme: selected.isSystem ? AppThemes.light.data : selected.data,
      darkTheme: selected.isSystem ? AppThemes.dark.data : null,
      themeMode: selected.isSystem ? ThemeMode.system : ThemeMode.light,
      home: isFirstLaunch ? const SetupScreen() : const RootScreen(),
    );
  }
}

const String discoveryUrl = "https://nectan.ngrok.dev";
const platform = MethodChannel('com.trykacz.nectan/media_scanner');

final FlutterLocalNotificationsPlugin flutterLocalNotificationsPlugin =
    FlutterLocalNotificationsPlugin();

class UserInfo {
  final String? username;

  UserInfo({required this.username});
}

class UserInfoNotifier extends Notifier<UserInfo> {
  final String? initialUsername;
  final String? promoCode;

  UserInfoNotifier([this.initialUsername, this.promoCode]);

  @override
  UserInfo build() {
    loadUsername();
    return UserInfo(username: initialUsername);
  }

  Future<void> refresh() async {
    debugPrint("🟦 Refreshing userinfo");
    loadUsername();
  }

  Future<void> loadUsername() async {
    final name = await getUsername();
    debugPrint("🟦 Received username $name");
    state = UserInfo(username: name);
  }

  void updateUsername(String? newName) {
    state = UserInfo(username: newName);
  }

  void updatePromoCode(String? code) {
    state = UserInfo(username: state.username);
  }
}

final userInfoProvider = NotifierProvider<UserInfoNotifier, UserInfo>(() {
  return UserInfoNotifier();
});

class FastRoute<T> extends CupertinoPageRoute<T> {
  FastRoute({required super.builder});

  @override
  Duration get transitionDuration => const Duration(milliseconds: 300);

  @override
  Duration get reverseTransitionDuration => const Duration(milliseconds: 300);
}

class ConnectionRequestNotifier extends Notifier<ConnectionRequest?> {
  StreamSubscription? _sub;

  @override
  ConnectionRequest? build() => null;

  void setStream(Stream<ConnectionRequest> stream) {
    debugPrint("🟦 set connection request stream");

    _sub?.cancel();
    _sub = stream.listen(
      (request) async {
        debugPrint("🟦 Connection stream event: $request");
        await showAppNotification(
          title: 'Connection request',
          body: '${request.username} wants to connect.',
        );
        state = request;
      },
      onError: (error, stackTrace) {
        debugPrint("🟥 Connection stream error: $error\n$stackTrace");
      },
      onDone: () {
        debugPrint("🟨 Connection stream closed");
      },
    );
  }

  void clear() {
    debugPrint("🟦 Clearing connection request notifier");
    state = null;
  }
}

final connectionRequestNotifierProvider =
    NotifierProvider<ConnectionRequestNotifier, ConnectionRequest?>(
      ConnectionRequestNotifier.new,
    );

class TransferRequestNotifier extends Notifier<TransferOfferDto?> {
  StreamSubscription? _sub;

  @override
  TransferOfferDto? build() => null;

  void setStream(Stream<TransferOfferDto> stream) {
    debugPrint("🟦 set connection request stream");

    _sub?.cancel();
    _sub = stream.listen(
      (request) async {
        debugPrint("🟦 Transfer offer stream event: $request");

        await showAppNotification(
          title: 'New transfer',
          body: 'Accept transfer from ${request.senderDeviceName}',
        );

        state = request;
      },
      onError: (error, stackTrace) {
        debugPrint("🟥 Connection stream error: $error\n$stackTrace");
      },
      onDone: () {
        debugPrint("🟨 Connection stream closed");
      },
    );
  }

  void clear() {
    debugPrint("🟦 Clearing connection request notifier");
    state = null;
  }
}

final transferRequestNotifierProvider =
    NotifierProvider<TransferRequestNotifier, TransferOfferDto?>(
      TransferRequestNotifier.new,
    );

class NearbyDeviceFoundNotifier extends Notifier<DeviceDto?> {
  StreamSubscription? _sub;

  @override
  DeviceDto? build() => null;

  void setStream(Stream<DeviceDto> stream) {
    _sub?.cancel();
    _sub = stream.listen(
      (deviceId) {
        state = deviceId;
      },
      onError: (error, stackTrace) {
        debugPrint("🟥 Found nearby device error: $error\n$stackTrace");
      },
      onDone: () {
        debugPrint("Found neaby device stream closed");
      },
    );
  }

  void clear() {
    state = null;
  }
}

final nearbyDeviceFoundNotifierProvider =
    NotifierProvider<NearbyDeviceFoundNotifier, DeviceDto?>(
      NearbyDeviceFoundNotifier.new,
    );

class TextMessageNotifier extends Notifier<List<TextMessageDto>> {
  StreamSubscription? _sub;

  @override
  List<TextMessageDto> build() => [];

  void setStream(Stream<TextMessageDto> stream) {
    _sub?.cancel();
    _sub = stream.listen(
      (message) async {
        await showAppNotification(
          title: 'Message from ${message.sender}',
          body: message.content,
        );
        state = [...state, message];
      },
      onError: (error, stackTrace) {
        debugPrint("🟥 Text message stream error: $error\n$stackTrace");
      },
      onDone: () {
        debugPrint("🟨 Text message stream closed");
      },
    );
  }

  void removeFirst() {
    if (state.isEmpty) return;
    state = state.sublist(1);
  }
}

final textMessageQueueProvider =
    NotifierProvider<TextMessageNotifier, List<TextMessageDto>>(
      TextMessageNotifier.new,
    );

class AutoacceptedNotifier extends Notifier<String?> {
  StreamSubscription? _sub;

  @override
  String? build() => null;

  void setStream(Stream<String> stream) {
    _sub?.cancel();
    _sub = stream.listen(
      (from) async {
        await showAppNotification(
          title: 'Transfer accepted',
          body: 'Auto-accepted transfer from $from.',
        );
      },
      onError: (error, stackTrace) {
        debugPrint("🟥 Autoaccepted stream error: $error\n$stackTrace");
      },
      onDone: () {
        debugPrint("🟨 Autoaccepted stream closed");
      },
    );
  }

  void clear() => state = null;
}

final autoacceptedProvider = NotifierProvider<AutoacceptedNotifier, String?>(
  AutoacceptedNotifier.new,
);

class FoundNearbyTextNotifier extends Notifier<String?> {
  StreamSubscription? _sub;

  @override
  String? build() => null;

  void setStream(Stream<String> stream) {
    _sub?.cancel();
    _sub = stream.listen(
      (message) async {
        await showAppNotification(title: 'Device nearby', body: message);
      },
      onError: (error, stackTrace) {
        debugPrint("🟥 Found nearby stream error: $error\n$stackTrace");
      },
      onDone: () {
        debugPrint("🟨 Found nearby stream closed");
      },
    );
  }

  void clear() => state = null;
}

final foundNearbyTextProvider =
    NotifierProvider<FoundNearbyTextNotifier, String?>(
      FoundNearbyTextNotifier.new,
    );

int _notificationId = 0;

Future<void> showAppNotification({
  required String title,
  required String body,
}) async {
  if (Platform.isLinux) {
    debugPrint("Omitted notification show. LINUX");
    return;
  }
  if (WidgetsBinding.instance.lifecycleState == AppLifecycleState.resumed) {
    return;
  }
  if (!await NotificationSettings.enabled()) return;

  const androidDetails = AndroidNotificationDetails(
    'nectan_messages',
    'Nectan',
    importance: Importance.high,
    priority: Priority.high,
    largeIcon: DrawableResourceAndroidBitmap('@mipmap/ic_launcher'),
    icon: '@mipmap/ic_launcher',
  );
  const notificationDetails = NotificationDetails(android: androidDetails);

  _notificationId += 1;
  await flutterLocalNotificationsPlugin.show(
    id: _notificationId,
    title: title,
    body: body,
    notificationDetails: notificationDetails,
  );
}

Future<void> initForegroundTask() async {
  FlutterForegroundTask.init(
    foregroundTaskOptions: ForegroundTaskOptions(
      eventAction: ForegroundTaskEventAction.nothing(),
      allowWakeLock: true,
      allowWifiLock: true,
    ),
    androidNotificationOptions: AndroidNotificationOptions(
      channelId: 'nectan_transfer_channel',
      channelName: 'Nectan File Transfer',
      channelDescription: 'Shows active file transfers in the background.',
      channelImportance: NotificationChannelImportance.LOW,
      priority: NotificationPriority.LOW,
    ),
    iosNotificationOptions: const IOSNotificationOptions(
      showNotification: true,
    ),
  );
}

class VerifyEmailScreeen extends ConsumerStatefulWidget {
  final String token;
  final String sessionId;
  const VerifyEmailScreeen({
    super.key,
    required this.token,
    required this.sessionId,
  });

  @override
  ConsumerState<VerifyEmailScreeen> createState() => _VerifyEmailScreeenState();
}

class _VerifyEmailScreeenState extends ConsumerState<VerifyEmailScreeen> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(body: const Text("XD"));
  }
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await initForegroundTask();
  await RustLib.init();

  final token = RootIsolateToken.instance!;

  await registerCallback(
    cb: (dirPath) async {
      BackgroundIsolateBinaryMessenger.ensureInitialized(token);
      debugPrint('🟦 Scanning dir: $dirPath');
      await for (final f in Directory(dirPath).list(recursive: true)) {
        if (f is File) {
          try {
            await MediaScanner.loadMedia(path: f.path);
            debugPrint('🟦 OK: ${f.path}');
          } catch (e) {
            debugPrint('🟦 ERR: $e — ${f.path}');
          }
        }
      }
      debugPrint('🟦 Scanning done.');
    },
  );

  SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
  SystemChrome.setSystemUIOverlayStyle(const SystemUiOverlayStyle());
  SystemChrome.setPreferredOrientations([
    DeviceOrientation.portraitUp,
    DeviceOrientation.portraitDown,
  ]);

  final appDir = await getApplicationDocumentsDirectory();

  bool isFirstLaunch = true;

  try {
    isFirstLaunch = await initEngine(basePath: appDir.path);
  } catch (e) {
    debugPrint("🟥 Rust lib init failed: $e");
  }

  await AppThemeNotifier.preload();
  await SpeedLimits.apply();

  runApp(ProviderScope(child: MyApp(isFirstLaunch: isFirstLaunch)));
}

class SetupScreen extends ConsumerStatefulWidget {
  const SetupScreen({super.key});

  @override
  ConsumerState<SetupScreen> createState() => _SetupScreenState();
}

class _SetupScreenState extends ConsumerState<SetupScreen> {
  final TextEditingController _controller = TextEditingController();
  bool _isValid = false;
  bool _isLoading = false;

  @override
  void initState() {
    super.initState();
    _controller.addListener(_validateInput);
  }

  @override
  void dispose() {
    _controller.dispose();

    super.dispose();
  }

  void _validateInput() {
    final text = _controller.text;
    setState(() {
      _isValid = text.isNotEmpty && text.length <= 32;
    });
  }

  Future<void> _handleContinue() async {
    setState(() => _isLoading = true);

    try {
      final username = _controller.text;

      await saveUsername(username: username);
      debugPrint("🟦 Username saved.");

      ref.read(userInfoProvider.notifier).updateUsername(username);

      if (mounted) {
        Navigator.pushReplacement(
          context,
          MaterialPageRoute(builder: (_) => const RootScreen()),
        );
      }
    } catch (e) {
      debugPrint("Error during setup: $e");
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text("Failed to initialize: $e")));
      }
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 24.0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.center,
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              const Spacer(),
              const Spacer(),
              Center(
                child: Text(
                  "Nectan",
                  style: TextStyle(
                    color: context.colors.primaryFont,
                    fontSize: 48,
                    fontWeight: FontWeight.bold,
                    fontFamily: "Rubik",
                  ),
                ),
              ),
              const SizedBox(height: 12),
              Center(
                child: Text(
                  "Enter your username.",
                  style: TextStyle(
                    fontSize: 14,
                    color: context.colors.secondaryFont,
                    fontWeight: FontWeight.w500,
                    fontFamily: "Rubik",
                  ),
                ),
              ),
              const Spacer(),

              Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 340),
                  child: TextField(
                    controller: _controller,
                    maxLength: 32,
                    maxLines: 1,
                    showCursor: true,
                    autofocus: true,
                    cursorColor: context.colors.secondaryFont,
                    cursorWidth: 1,
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      color: context.colors.primaryFont,
                      fontSize: 16,
                      fontFamily: "Rubik",
                      fontWeight: FontWeight.w600,
                      decoration: TextDecoration.none,
                    ),
                    decoration: InputDecoration(
                      filled: true,
                      fillColor: context.colors.secondaryCol,
                      border: OutlineInputBorder(
                        borderRadius: BorderRadius.circular(12),
                        borderSide: const BorderSide(
                          color: Color.fromARGB(97, 161, 161, 161),
                        ),
                      ),
                      enabledBorder: OutlineInputBorder(
                        borderRadius: BorderRadius.circular(12),
                        borderSide: const BorderSide(
                          color: Color.fromARGB(117, 161, 161, 161),
                        ),
                      ),
                      focusedBorder: OutlineInputBorder(
                        borderRadius: BorderRadius.circular(12),
                        borderSide: const BorderSide(
                          color: Color.fromARGB(114, 190, 190, 190),
                          width: 1,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
              const Spacer(),
              PrimaryWideButton(
                label: 'Continue',
                onPressed: (_isValid && !_isLoading)
                    ? _handleContinue
                    : () => {},
              ),
              const Spacer(),
            ],
          ),
        ),
      ),
    );
  }
}

class SecondaryTextButton extends StatelessWidget {
  final String label;
  final VoidCallback onPressed;
  const SecondaryTextButton({
    super.key,
    required this.label,
    required this.onPressed,
  });
  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: double.infinity,
      height: 52,
      child: TextButton(
        onPressed: onPressed,
        style: TextButton.styleFrom(
          shape: RoundedSuperellipseBorder(
            borderRadius: BorderRadius.circular(20),
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            color: context.colors.primaryFont,
            fontSize: 16,
            fontFamily: 'Rubik',
            fontWeight: FontWeight.w500,
          ),
        ),
      ),
    );
  }
}

// class SmallButton extends StatelessWidget {
//   final String label;
//   final VoidCallback? onPressed;
//   final double maxWidth;
//   const SmallButton({
//     super.key,
//     required this.label,
//     required this.onPressed,
//     this.maxWidth = 300,
//   });
//
//   @override
//   Widget build(BuildContext context) {
//     return ConstrainedBox(
//       constraints: BoxConstraints(maxWidth: maxWidth),
//       child: SizedBox(
//         height: 42,
//         child: ElevatedButton(
//           onPressed: onPressed,
//           style: ElevatedButton.styleFrom(
//             padding: const EdgeInsets.symmetric(horizontal: 24),
//             tapTargetSize: MaterialTapTargetSize.shrinkWrap,
//             shape: RoundedRectangleBorder(
//               borderRadius: BorderRadius.circular(12),
//             ),
//             foregroundColor: context.colors.primaryFont,
//             backgroundColor: Colors.white,
//             disabledBackgroundColor: Colors.white,
//           ),
//           child: Text(
//             label,
//             style: const TextStyle(
//               fontFamily: 'Rubik',
//               fontSize: 15,
//               fontWeight: FontWeight.w600,
//               color: Colors.black,
//             ),
//           ),
//         ),
//       ),
//     );
//   }
// }

class PrimaryWideButton extends StatefulWidget {
  final String label;
  final VoidCallback? onPressed;
  final double maxWidth;
  final bool isLoading;
  final bool failed;
  final String? failedLabel;
  final bool? destructive;

  const PrimaryWideButton({
    super.key,
    required this.label,
    required this.onPressed,
    this.maxWidth = 300,
    this.isLoading = false,
    this.failed = false,
    this.failedLabel,
    this.destructive = false,
  });

  @override
  State<PrimaryWideButton> createState() => _PrimaryWideButtonState();
}

class _PrimaryWideButtonState extends State<PrimaryWideButton> {
  double _scale = 1.0;

  @override
  Widget build(BuildContext context) {
    final color = widget.destructive == true
        ? Color.fromARGB(255, 223, 47, 65)
        : context.colors.primaryFont;
    final fontColor = widget.destructive == true
        ? Colors.white
        : context.colors.primaryCol;

    return Listener(
      onPointerDown: (_) => setState(() => _scale = 0.95),
      onPointerUp: (_) => setState(() => _scale = 1.0),
      onPointerCancel: (_) => setState(() => _scale = 1.0),
      child: AnimatedScale(
        scale: _scale,
        duration: const Duration(milliseconds: 100),
        curve: Curves.easeOut,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: widget.maxWidth),
          child: SizedBox(
            width: double.infinity,
            height: 56,
            child: ElevatedButton(
              onPressed: (widget.onPressed == null || widget.failed)
                  ? null
                  : widget.onPressed,
              style: ElevatedButton.styleFrom(
                shape: RoundedSuperellipseBorder(
                  borderRadius: BorderRadius.circular(20),
                ),
                backgroundColor: color,
                disabledBackgroundColor: context.colors.thirdFont,
              ),
              child: widget.isLoading
                  ? Spinner()
                  : widget.failed
                  ? Text(
                      widget.failedLabel!,
                      style: TextStyle(
                        fontFamily: 'Rubik',
                        fontSize: 16,
                        fontWeight: FontWeight.w600,
                        color: fontColor,
                      ),
                    )
                  : Text(
                      widget.label,
                      style: TextStyle(
                        fontFamily: 'Rubik',
                        fontSize: 16,
                        fontWeight: FontWeight.w600,
                        color: fontColor,
                      ),
                    ),
            ),
          ),
        ),
      ),
    );
  }
}

Future<void> _initApp(WidgetRef ref) async {
  final appDir = await getApplicationDocumentsDirectory();
  final devicesSink = RustStreamSink<List<DeviceDto>>();
  final newConnectionSink = RustStreamSink<ConnectionRequest>();
  final transfersSink = RustStreamSink<List<TransferMetaDto>>();
  final transferOfferSink = RustStreamSink<TransferOfferDto>();
  final foundNearbyDeviceSink = RustStreamSink<DeviceDto>();
  final textMessageSink = RustStreamSink<TextMessageDto>();
  final autoacceptSink = RustStreamSink<String>();
  final foundNearbyTextSink = RustStreamSink<String>();

  try {
    await initApp(
      appDir: appDir.path,
      devicesSink: devicesSink,
      newConnectionSink: newConnectionSink,
      transfersSink: transfersSink,
      transferOfferSink: transferOfferSink,
      foundDeviceNearbySink: foundNearbyDeviceSink,
      textMessageSink: textMessageSink,
      autoacceptSink: autoacceptSink,
      foundNearbySink: foundNearbyTextSink,
    );

    ref
        .read(devicePoolProvider.notifier)
        .setStream(
          devicesSink.stream.handleError((e) {
            debugPrint("🟥 Device stream error: $e");
          }),
        );

    ref
        .read(connectionRequestNotifierProvider.notifier)
        .setStream(
          newConnectionSink.stream.handleError((e) {
            debugPrint("🟥 Connection stream error: $e");
          }),
        );

    ref
        .read(transferRequestNotifierProvider.notifier)
        .setStream(
          transferOfferSink.stream.handleError((e) {
            debugPrint("🟥 Transfer offer stream error: $e");
          }),
        );

    ref
        .read(transfersPoolProvider.notifier)
        .setStream(
          transfersSink.stream.handleError((e) {
            debugPrint("🟥 Transfers stream error: $e");
          }),
        );

    ref
        .read(nearbyDeviceFoundNotifierProvider.notifier)
        .setStream(
          foundNearbyDeviceSink.stream.handleError((e) {
            debugPrint("🟥 Connected to nearby device stream error: $e");
          }),
        );

    ref
        .read(textMessageQueueProvider.notifier)
        .setStream(
          textMessageSink.stream.handleError((e) {
            debugPrint("🟥 Text message stream error: $e");
          }),
        );

    ref
        .read(autoacceptedProvider.notifier)
        .setStream(
          autoacceptSink.stream.handleError((e) {
            debugPrint("🟥 Autoaccept stream error: $e");
          }),
        );

    ref
        .read(foundNearbyTextProvider.notifier)
        .setStream(
          foundNearbyTextSink.stream.handleError((e) {
            debugPrint("🟥 Found nearby stream error: $e");
          }),
        );

    debugPrint("🟦 App initialized");
  } catch (e) {
    debugPrint("🟥 App initalizing error: $e");
  }
}

final currentTabProvider = NotifierProvider<CurrentTabNotifier, int>(
  CurrentTabNotifier.new,
);

class CurrentTabNotifier extends Notifier<int> {
  @override
  int build() => 0;

  void set(int tab) => state = tab;
}

class IncomingTextScreen extends ConsumerWidget {
  final VoidCallback onClose;
  const IncomingTextScreen({super.key, required this.onClose});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final messages = ref.watch(textMessageQueueProvider);
    final colors = context.colors;

    if (messages.isEmpty) {
      return Scaffold(
        backgroundColor: colors.primaryCol,
        body: const SizedBox.shrink(),
      );
    }

    final message = messages.first;

    Future<void> copy() async {
      await Clipboard.setData(ClipboardData(text: message.content));
      if (context.mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(const SnackBar(content: Text('Copied')));
      }
    }

    void close() {
      Navigator.of(context).pop();
      onClose();
    }

    return Scaffold(
      backgroundColor: colors.primaryCol,
      appBar: AppBar(
        backgroundColor: colors.primaryCol,
        leading: IconButton(
          icon: const Icon(Icons.chevron_left),
          onPressed: close,
          iconSize: 20,
          padding: EdgeInsets.zero,
        ),
        titleSpacing: 0,
        title: Text(
          'Message',
          style: TextStyle(
            fontFamily: 'Rubik',
            color: colors.primaryFont,
            fontWeight: FontWeight.w600,
            fontSize: 24,
          ),
        ),
      ),
      body: SafeArea(
        child: Column(
          children: [
            Expanded(
              child: Center(
                child: SingleChildScrollView(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 32,
                    vertical: 24,
                  ),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.center,
                    children: [
                      Text(
                        message.sender,
                        textAlign: TextAlign.center,
                        style: TextStyle(
                          fontFamily: 'Rubik',
                          color: colors.primaryFont,
                          fontWeight: FontWeight.w600,
                          fontSize: 20,
                        ),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        'sent you a message:',
                        textAlign: TextAlign.center,
                        style: TextStyle(
                          fontFamily: 'Rubik',
                          color: colors.secondaryFont,
                          fontSize: 14,
                        ),
                      ),
                      const SizedBox(height: 28),
                      SelectableText(
                        message.content,
                        textAlign: TextAlign.center,
                        style: TextStyle(
                          fontFamily: 'Rubik',
                          color: colors.secondaryFont,
                          fontSize: 15,
                          height: 1.5,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 8, 20, 24),
              child: Row(
                children: [
                  Expanded(
                    child: FilledButton(
                      onPressed: close,
                      style: FilledButton.styleFrom(
                        backgroundColor: colors.primaryFont,
                        foregroundColor: colors.primaryCol,
                        padding: const EdgeInsets.symmetric(vertical: 16),
                        shape: sq(12),
                      ),
                      child: const Text(
                        'Close',
                        style: TextStyle(
                          fontFamily: 'Rubik',
                          fontWeight: FontWeight.w600,
                          fontSize: 16,
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 14),
                  Expanded(
                    child: FilledButton(
                      onPressed: copy,
                      style: FilledButton.styleFrom(
                        backgroundColor: colors.thirdCol,
                        foregroundColor: colors.primaryFont,
                        padding: const EdgeInsets.symmetric(vertical: 16),
                        shape: sq(12),
                      ),
                      child: const Text(
                        'Copy',
                        style: TextStyle(
                          fontFamily: 'Rubik',
                          fontWeight: FontWeight.w600,
                          fontSize: 16,
                        ),
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

class RootScreen extends ConsumerStatefulWidget {
  const RootScreen({super.key});

  @override
  ConsumerState<RootScreen> createState() => _RootScreenState();
}

class _RootScreenState extends ConsumerState<RootScreen>
    with WidgetsBindingObserver {
  final _dialogQueue = Queue<Widget Function()>();
  bool _dialogShowing = false;
  bool _textScreenShowing = false;

  late final PageController _pageController;
  late final ValueNotifier<double> _pagePosition;
  late final _GuardedPhysics _physics;

  /// True while the user drags the nav pill. The finger drives the pages, so
  /// tab-change side effects (animations, provider sync) stay quiet.
  bool _scrubbing = false;

  /// Target of the running programmatic page animation, to avoid restarting it.
  int? _animatingTo;

  void _enqueueDialog(Widget Function() builder) {
    _dialogQueue.add(builder);
    _processQueue();
  }

  Future<void> _processQueue() async {
    if (_dialogShowing || _dialogQueue.isEmpty) return;
    _dialogShowing = true;

    final builder = _dialogQueue.removeFirst();
    try {
      await showDialog(
        context: context,
        barrierDismissible: false,
        builder: (_) => builder(),
      );
    } finally {
      _dialogShowing = false;
      _processQueue(); // next
    }
  }

  final GlobalKey<NavigatorState> _devicesNavKey = GlobalKey<NavigatorState>();

  /// Finger swiping between tabs is blocked while the Devices tab has a
  /// sub-screen open, so the swipe doesn't drag that sub-screen away.
  bool _canSwipe() {
    final page = _pageController.hasClients ? (_pageController.page ?? 0) : 0;
    final onDevices = page.round() == 0;
    final devicesHasSubScreen = _devicesNavKey.currentState?.canPop() ?? false;
    return !(onDevices && devicesHasSubScreen);
  }

  void _goTo(int index) {
    if (!_pageController.hasClients) return;
    if (_animatingTo == index) return;
    final current = _pageController.page ?? index.toDouble();
    if ((current - index).abs() < 0.001) return;

    _animatingTo = index;
    _pageController
        .animateToPage(
          index,
          duration: const Duration(milliseconds: 300),
          curve: Curves.easeOutCubic,
        )
        .whenComplete(() {
          if (_animatingTo == index) _animatingTo = null;
        });
  }

  void _onScrubStart() {
    _scrubbing = true;
    _animatingTo = null;
  }

  void _onScrub(double page) {
    if (!_pageController.hasClients) return;
    final position = _pageController.position;
    _pageController.jumpTo(
      (page * position.viewportDimension).clamp(
        position.minScrollExtent,
        position.maxScrollExtent,
      ),
    );
  }

  void _onScrubEnd(int index) {
    _scrubbing = false;
    ref.read(currentTabProvider.notifier).set(index);
    _goTo(index);
  }

  /// After a finger swipe settles, sync the selected tab with the page.
  bool _onScrollNotification(ScrollNotification notification) {
    if (notification.depth != 0 || _scrubbing || !mounted) return false;
    if (notification is ScrollEndNotification && _pageController.hasClients) {
      final page = _pageController.page;
      if (page != null) {
        final index = page.round();
        if (index != ref.read(currentTabProvider)) {
          ref.read(currentTabProvider.notifier).set(index);
        }
      }
    }
    return false;
  }

  @override
  void initState() {
    super.initState();

    final initialTab = ref.read(currentTabProvider);
    _pageController = PageController(initialPage: initialTab);
    _pagePosition = ValueNotifier<double>(initialTab.toDouble());
    _pageController.addListener(() {
      if (!_pageController.hasClients) return;
      final page = _pageController.page;
      if (page != null) _pagePosition.value = page;
    });
    _physics = _GuardedPhysics(canScroll: _canSwipe);

    WidgetsBinding.instance.addPostFrameCallback((_) async {
      await _initApp(ref);
      if (!Platform.isLinux) {
        await Permission.notification.request();
      } else {
        debugPrint("Ommiting permission check. LINUX");
      }

      try {
        PackageInfo packageInfo = await PackageInfo.fromPlatform();
        final versionReport =
            "Version report: Build ${packageInfo.buildNumber}. Version ${packageInfo.version}.";
        debugPrint(versionReport);
      } catch (e) {
        debugPrint("🟥 Failed to get version report $e");
      }
    });
  }

  @override
  void dispose() {
    _pageController.dispose();
    _pagePosition.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final currentIndex = ref.watch(currentTabProvider);

    // Slide between tabs instead of an instant swap (pill taps, etc.).
    ref.listen<int>(currentTabProvider, (prev, next) {
      if (_scrubbing) return;
      _goTo(next);
    });

    ref.listen(connectionRequestNotifierProvider, (prev, next) {
      if (next != null) {
        _enqueueDialog(() => ConnectionRequestDialog(request: next));
        ref.read(connectionRequestNotifierProvider.notifier).clear();
      }
    });

    ref.listen(transferRequestNotifierProvider, (prev, next) {
      if (next != null) {
        _enqueueDialog(() => TransferOfferDialog(offer: next));
        ref.read(transferRequestNotifierProvider.notifier).clear();
      }
    });

    ref.listen(nearbyDeviceFoundNotifierProvider, (prev, next) async {
      final show = await showConnectedToNearby();
      if (next != null && show) {
        _enqueueDialog(() => FoundNearbyScreen(device: next));
        ref.read(nearbyDeviceFoundNotifierProvider.notifier).clear();
      }
    });

    ref.listen(textMessageQueueProvider, (prev, next) {
      if (next.isNotEmpty && !_textScreenShowing) {
        _textScreenShowing = true;
        _enqueueDialog(
          () => IncomingTextScreen(
            onClose: () {
              _textScreenShowing = false;
              ref
                  .read(textMessageQueueProvider.notifier)
                  .removeFirst();
            },
          ),
        );
      }
    });

    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      extendBody: true,
      extendBodyBehindAppBar: true,
      body: NotificationListener<ScrollNotification>(
        onNotification: _onScrollNotification,
        child: PageView(
          controller: _pageController,
          physics: _physics,
          children: [
            _KeepAlive(child: DevicesScreen(navigatorKey: _devicesNavKey)),
            _KeepAlive(child: TransfersScreen()),
            _KeepAlive(child: SettingsScreen()),
          ],
        ),
      ),
      bottomNavigationBar: BottomPillNav(
        position: _pagePosition,
        onTap: (i) {
          if (i == 0 && currentIndex == 0) {
            _devicesNavKey.currentState?.popUntil((route) => route.isFirst);
          } else {
            ref.read(currentTabProvider.notifier).set(i);
            _goTo(i);
          }
        },
        onScrubStart: _onScrubStart,
        onScrub: _onScrub,
        onScrubEnd: _onScrubEnd,
      ),
    );
  }
}

/// Keeps a page's state alive while it is off-screen in the PageView, like
/// IndexedStack did (navigator stacks and scroll positions survive).
class _KeepAlive extends StatefulWidget {
  const _KeepAlive({required this.child});
  final Widget child;

  @override
  State<_KeepAlive> createState() => _KeepAliveState();
}

class _KeepAliveState extends State<_KeepAlive>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return widget.child;
  }
}

/// Clamping physics whose finger scrolling can be switched off on demand
/// (checked at the start of each drag, so it needs no rebuild).
class _GuardedPhysics extends ClampingScrollPhysics {
  const _GuardedPhysics({required this.canScroll, super.parent});

  final bool Function() canScroll;

  @override
  _GuardedPhysics applyTo(ScrollPhysics? ancestor) {
    return _GuardedPhysics(canScroll: canScroll, parent: buildParent(ancestor));
  }

  @override
  bool shouldAcceptUserOffset(ScrollMetrics position) {
    return canScroll() && super.shouldAcceptUserOffset(position);
  }
}

class PrimaryButton extends StatelessWidget {
  final String label;
  final VoidCallback onPressed;

  const PrimaryButton({
    super.key,
    required this.label,
    required this.onPressed,
  });
  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 200),
      child: SizedBox(
        width: double.infinity,
        height: 56,
        child: ElevatedButton(
          onPressed: onPressed,
          style: ElevatedButton.styleFrom(
            shape: RoundedSuperellipseBorder(
              borderRadius: BorderRadius.circular(20),
            ),
            foregroundColor: Colors.white,
            backgroundColor: Colors.white,
          ),
          child: Text(
            label,
            style: const TextStyle(
              fontSize: 20,
              fontWeight: FontWeight.w700,
              fontFamily: "Rubik",
              color: Colors.black,
            ),
          ),
        ),
      ),
    );
  }
}

class PermissionDialog extends StatefulWidget {
  final String title;
  final String description;

  final VoidCallback action;
  final String actionLabel;
  final Permission permission;
  final PermissionStatus intialStatus;

  const PermissionDialog({
    super.key,
    required this.title,
    required this.description,
    required this.action,
    required this.actionLabel,
    required this.permission,
    required this.intialStatus,
  });

  @override
  State<PermissionDialog> createState() => _PermissionDialogState();
}

class _PermissionDialogState extends State<PermissionDialog>
    with WidgetsBindingObserver {
  PermissionStatus? permStatus;
  void _requestPermission() async {
    final status = await widget.permission.request();
    debugPrint("🟦 Permission dialog requestes permission: $status");
    if (!mounted) return;
    setState(() {
      permStatus = status;
    });
    if (status == PermissionStatus.granted) {
      Navigator.of(context).pop(true);
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) async {
    if (state == AppLifecycleState.resumed) {
      final status = await widget.permission.status;
      if (!mounted) return;
      if (status == PermissionStatus.granted) {
        debugPrint("🟦 Permission granted.");
        Navigator.of(context).pop(true);
      }
      setState(() {
        permStatus = status;
      });
    }
  }

  @override
  void initState() {
    super.initState();
  }

  @override
  Widget build(BuildContext context) {
    PermissionStatus status = permStatus ?? widget.intialStatus;
    bool isPermanentlyDenied = status == PermissionStatus.permanentlyDenied;

    String description = widget.description;
    if (isPermanentlyDenied) {
      description += " Please provide permission in App settings.";
    }

    return Dialog.fullscreen(
      child: Scaffold(
        body: SafeArea(
          child: Center(
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                children: [
                  const Spacer(),
                  const Spacer(),
                  Text(
                    widget.title,
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      color: context.colors.primaryFont,
                      fontSize: 20,
                      fontWeight: FontWeight.w600,
                      fontFamily: "Rubik",
                    ),
                  ),
                  const SizedBox(height: 24),
                  Text(
                    description,
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      color: context.colors.secondaryFont,
                      fontSize: 14,
                    ),
                  ),
                  const Spacer(),
                  Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      PrimaryWideButton(
                        label: isPermanentlyDenied ? 'Settings' : 'Allow',
                        onPressed: () {
                          if (isPermanentlyDenied) {
                            openAppSettings(); // permission_handler
                          } else {
                            _requestPermission();
                          }
                        },
                      ),
                      const SizedBox(height: 24),
                      SecondaryTextButton(
                        label: "Cancel",
                        onPressed: () => Navigator.pop(context, false),
                      ),
                    ],
                  ),
                  const Spacer(),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
