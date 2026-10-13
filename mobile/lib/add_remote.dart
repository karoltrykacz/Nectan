import 'dart:async';
import 'dart:math';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:nectan_mobile/devices.dart';
import 'package:nectan_mobile/elements.dart';
import 'package:nectan_mobile/connectivity.dart';
import 'package:nectan_mobile/main.dart';
import 'package:nectan_mobile/src/rust/api/simple.dart';
import 'package:qr_flutter/qr_flutter.dart' show QrImageView, QrVersions;

class CodeTimerNotifier extends Notifier<int> {
  @override
  int build() {
    ref.onDispose(() {
      _timer?.cancel();
    });
    return 0;
  }

  Timer? _timer;
  late String code = _generateCode();

  String _generateCode() {
    final random = Random();
    return List.generate(6, (_) => random.nextInt(10)).join();
  }

  void _start() async {
    try {
      debugPrint("🟩 Issuing code");
      // Server may return other code in case of code conflict
      code = await issueCode(code: code);
      debugPrint("🟩 Code issued");
    } catch (e) {
      debugPrint("🟥 Code issuing error $e"); // ! unhandled erorr
    }

    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (state <= 0) {
        _timer?.cancel();
      } else {
        state = state - 1;
      }
    });
  }

  void renew() {
    code = _generateCode();
    state = 90;
    _start();
  }
}

final codeTimerProvider = NotifierProvider<CodeTimerNotifier, int>(
  CodeTimerNotifier.new,
);

class CodeScreen extends ConsumerStatefulWidget {
  const CodeScreen({super.key});

  @override
  ConsumerState<CodeScreen> createState() => _CodeScreenState();
}

enum CodeLookupState { idle, looking, found, notFound }

/// Which side of the pairing the user is on.
enum CodeMode { enter, show }

class _CodeScreenState extends ConsumerState<CodeScreen> {
  final _codeController = TextEditingController();
  final _codeFocus = FocusNode();
  CodeLookupState _lookupState = CodeLookupState.idle;
  Future<String>? _lookupFuture;
  Future<String?>? _ticketFuture;
  CodeMode _mode = CodeMode.enter;

  Future<void> _startLookup(String enteredCode) async {
    setState(() => _lookupState = CodeLookupState.looking);
    _lookupFuture = lookupCode(code: enteredCode);
    try {
      await _lookupFuture!;
      if (!mounted) return;
      setState(() => _lookupState = CodeLookupState.found);
      debugPrint("Found code");
    } catch (e) {
      debugPrint("$e");
      if (!mounted) return;
      setState(() => _lookupState = CodeLookupState.notFound);
    }
  }

  void _onCodeChanged(String value) {
    if (value.length == 6) {
      _startLookup(value);
    } else {
      setState(() => _lookupState = CodeLookupState.idle);
    }
  }

  void _setMode(CodeMode mode) {
    if (mode == _mode) return;
    setState(() => _mode = mode);
    _codeFocus.unfocus();
  }

  @override
  void initState() {
    super.initState();

    // Fetched once, so the QR button doesn't re-request it on every timer tick.
    _ticketFuture = getTicket();

    _codeController.addListener(() => _onCodeChanged(_codeController.text));

    // Renew only if already expired when entering the screen
    WidgetsBinding.instance.addPostFrameCallback((_) {
      ref.read(connectivityNotifier.notifier).recheck();
      final secondsLeft = ref.read(codeTimerProvider);
      if (secondsLeft == 0) {
        ref.read(codeTimerProvider.notifier).renew();
      }
    });
  }

  @override
  void dispose() {
    _codeController.dispose();
    _codeFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final secondsLeft = ref.watch(codeTimerProvider);
    final notifier = ref.read(codeTimerProvider.notifier);
    final code = notifier.code;
    final keyboardOpen = MediaQuery.of(context).viewInsets.bottom > 0;

    final connectivity = ref.watch(connectivityNotifier);
    final isOnline = connectivity.value ?? true;

    return Scaffold(
      resizeToAvoidBottomInset: true,
      backgroundColor: context.colors.primaryCol,
      appBar: AppBar(
        backgroundColor: context.colors.primaryCol,
        leading: IconButton(
          icon: const Icon(Icons.chevron_left),
          onPressed: () => Navigator.of(context).pop(),
          iconSize: 20,
          padding: EdgeInsets.zero,
        ),
        titleSpacing: 0,
        title: const Text(
          'Add Remote Device',
          style: TextStyle(
            fontFamily: 'Rubik',
            fontWeight: FontWeight.w600,
            fontSize: 24,
          ),
        ),
      ),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 600),
          child: !isOnline
              ? SafeArea(
                  child: Center(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Icon(
                          Icons.wifi_off_rounded,
                          size: 48,
                          color: context.colors.thirdFont,
                        ),
                        const SizedBox(height: 16),
                        Text(
                          'You\'re offline',
                          style: TextStyle(
                            color: context.colors.primaryFont,
                            fontSize: 22,
                            fontWeight: FontWeight.w600,
                            fontFamily: 'Rubik',
                          ),
                        ),
                        const SizedBox(height: 8),
                        Text(
                          'Connect to the Internet to add remote devices.',
                          textAlign: TextAlign.center,
                          style: TextStyle(
                            color: context.colors.secondaryFont,
                            fontSize: 14,
                          ),
                        ),
                      ],
                    ),
                  ),
                )
              : SafeArea(
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(24, 8, 24, 16),
                    child: Column(
                      children: [
                        Expanded(
                          child: SingleChildScrollView(
                            padding: const EdgeInsets.only(top: 16),
                            child: AnimatedSwitcher(
                              duration: const Duration(milliseconds: 200),
                              child: _mode == CodeMode.show
                                  ? KeyedSubtree(
                                      key: const ValueKey('show'),
                                      child: _buildShowMode(code, secondsLeft),
                                    )
                                  : KeyedSubtree(
                                      key: const ValueKey('enter'),
                                      child: _buildEnterMode(),
                                    ),
                            ),
                          ),
                        ),
                        const ExpandableTip(
                          expandable: true,
                          title: 'How to connect?',
                          tip:
                              "1. Ask your friend for code and enter it on your device.\n"
                              "2. Wait until he accepts connection.\n"
                              "3. Done! Send your files.\n\n"
                              "Both devices need an Internet connection.",
                        ),

                        const SizedBox(height: 20),
                        if (!keyboardOpen) ...[
                          const SizedBox(height: 12),
                          _buildModeToggle(),
                          const SizedBox(height: 20),
                          _buildQRButton(),
                        ],
                      ],
                    ),
                  ),
                ),
        ),
      ),
    );
  }

  Widget _buildModeToggle() {
    return Container(
      padding: const EdgeInsets.all(4),
      decoration: BoxDecoration(
        color: context.colors.thirdCol,
        borderRadius: BorderRadius.circular(14),
      ),
      child: Row(
        children: [
          _buildModeSegment(
            label: 'Enter code',
            icon: Icons.keyboard_alt_outlined,
            mode: CodeMode.enter,
          ),
          _buildModeSegment(
            label: 'Show my code',
            icon: Icons.visibility_outlined,
            mode: CodeMode.show,
          ),
        ],
      ),
    );
  }

  Widget _buildModeSegment({
    required String label,
    required IconData icon,
    required CodeMode mode,
  }) {
    final selected = _mode == mode;
    return Expanded(
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => _setMode(mode),
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 180),
          padding: const EdgeInsets.symmetric(vertical: 12),
          decoration: BoxDecoration(
            color: selected ? context.colors.primaryFont : Colors.transparent,
            borderRadius: BorderRadius.circular(10),
          ),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(
                icon,
                size: 16,
                color: selected
                    ? context.colors.primaryCol
                    : context.colors.secondaryFont,
              ),
              const SizedBox(width: 6),
              Text(
                label,
                style: TextStyle(
                  fontFamily: 'Rubik',
                  fontSize: 14,
                  fontWeight: FontWeight.w600,
                  color: selected
                      ? context.colors.primaryCol
                      : context.colors.secondaryFont,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildEnterMode() {
    final codeReady = _codeController.text.length == 6;

    return Column(
      children: [
        Text(
          "Enter the other device's code",
          style: TextStyle(
            color: context.colors.primaryFont,
            fontSize: 18,
            fontWeight: FontWeight.w600,
            fontFamily: "Rubik",
          ),
        ),
        const SizedBox(height: 8),

        ValueListenableBuilder(
          valueListenable: _codeController,
          builder: (context, value, child) {
            return GestureDetector(
              onTap: () => _codeFocus.requestFocus(),
              child: Container(
                width: double.infinity,
                padding: const EdgeInsets.symmetric(vertical: 20),
                color: Colors.transparent,
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    _buildInputGroup(value.text, 0),
                    const SizedBox(width: 20),
                    _buildInputGroup(value.text, 3),
                  ],
                ),
              ),
            );
          },
        ),

        SizedBox(
          height: 0,
          width: 0,
          child: TextField(
            controller: _codeController,
            focusNode: _codeFocus,
            keyboardType: TextInputType.number,
            maxLength: 6,
            autofocus: false,
            decoration: const InputDecoration(counterText: ''),
          ),
        ),

        const SizedBox(height: 16),

        PrimaryWideButton(
          label: "Connect",
          onPressed: codeReady
              ? () {
                  Navigator.of(context)
                      .push(
                        FastRoute(
                          builder: (context) => ConnectScreen(
                            code: _codeController.text,
                            lookupFuture: _lookupFuture,
                            initialState: _lookupState,
                          ),
                        ),
                      )
                      .then((_) {
                        _codeController.clear();
                        setState(() => _lookupState = CodeLookupState.idle);
                      });
                }
              : null,
        ),
      ],
    );
  }

  Widget _buildShowMode(String code, int secondsLeft) {
    return Column(children: [_buildYourCodeSection(code, secondsLeft)]);
  }

  Widget _buildMyCodeDigits(String digits, bool isExpired) {
    return Text(
      digits,
      style: TextStyle(
        fontSize: 44,
        fontWeight: FontWeight.w600,
        fontFamily: "Rubik",
        color: isExpired
            ? context.colors.thirdFont
            : context.colors.primaryFont,
        letterSpacing: 6,
      ),
    );
  }

  Widget _buildYourCodeSection(String code, int secondsLeft) {
    final isExpired = secondsLeft == 0;
    final isLow = secondsLeft <= 60 && !isExpired;

    return Column(
      children: [
        Text(
          'Your code',
          style: TextStyle(
            color: context.colors.primaryFont,
            fontSize: 18,
            fontWeight: FontWeight.w600,
            fontFamily: "Rubik",
          ),
        ),
        const SizedBox(height: 6),
        Text(
          'Share it with your friend',
          style: TextStyle(
            color: context.colors.secondaryFont,
            fontSize: 13,
            fontFamily: "Rubik",
          ),
        ),
        const SizedBox(height: 20),
        Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            _buildMyCodeDigits(code.substring(0, 3), isExpired),
            const SizedBox(width: 12),
            _buildMyCodeDigits(code.substring(3, 6), isExpired),
          ],
        ),
        const SizedBox(height: 12),
        if (isExpired)
          TextButton.icon(
            onPressed: () => ref.read(codeTimerProvider.notifier).renew(),
            icon: Icon(
              Icons.refresh,
              size: 15,
              color: context.colors.secondaryFont,
            ),
            label: Text(
              'Code expired — tap to renew',
              style: TextStyle(
                color: context.colors.secondaryFont,
                fontSize: 13,
                fontFamily: "Rubik",
              ),
            ),
          )
        else
          Column(
            children: [
              Text(
                'Expires in ${secondsLeft}s',
                style: TextStyle(
                  color: context.colors.secondaryFont,
                  fontSize: 13,
                  fontFamily: "Rubik",
                ),
              ),
              if (isLow)
                GestureDetector(
                  onTap: () => ref.read(codeTimerProvider.notifier).renew(),
                  child: Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: Text(
                      'Renew',
                      style: TextStyle(
                        color: context.colors.primaryFont,
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                ),
            ],
          ),
        const SizedBox(height: 32),
      ],
    );
  }

  Widget _buildQRButton() {
    return FutureBuilder(
      future: _ticketFuture,
      builder: (context, snapshot) {
        if (!snapshot.hasData || snapshot.data == null) {
          return const SizedBox.shrink();
        }
        final ticket = snapshot.data!;
        return TextButton.icon(
          onPressed: () {
            Navigator.of(
              context,
            ).push(FastRoute(builder: (context) => QrScreen(ticket: ticket)));
          },
          icon: Icon(
            Icons.qr_code,
            color: context.colors.primaryFont,
            size: 18,
          ),
          label: Text(
            'Show QR code',
            style: TextStyle(
              fontFamily: "Rubik",
              color: context.colors.primaryFont,
              fontSize: 13,
            ),
          ),
        );
      },
    );
  }

  Widget _buildInputGroup(String text, int start) {
    return Row(
      children: List.generate(3, (i) {
        final index = start + i;
        final filled = index < text.length;
        return Padding(
          padding: EdgeInsets.only(right: i < 2 ? 10 : 0),
          child: Text(
            filled ? text[index] : '·',
            style: TextStyle(
              fontFamily: 'Rubik',
              fontSize: 36,
              fontWeight: FontWeight.w600,
              color: filled
                  ? context.colors.primaryFont
                  : context.colors.thirdFont,
            ),
          ),
        );
      }),
    );
  }
}

class ConnectScreen extends StatefulWidget {
  final String code;
  final Future<String>? lookupFuture;
  final CodeLookupState initialState;

  const ConnectScreen({
    super.key,
    required this.code,
    required this.lookupFuture,
    required this.initialState,
  });

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen> {
  StreamSubscription? _sub;
  AddRemoteDeviceEvent? _currentStatus;

  void _startConnection() {
    final stream = addRemoteDevice();
    _sub = stream.listen(
      (event) => setState(() => _currentStatus = event),
      onDone: () => debugPrint("🟦 Add remote device done."),
    );
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      appBar: AppBar(
        backgroundColor: context.colors.primaryCol,
        titleSpacing: 0,
        title: const Text(
          'Connect',
          style: TextStyle(
            fontFamily: 'Rubik',
            fontWeight: FontWeight.w600,
            fontSize: 24,
          ),
        ),
      ),
      body: FutureBuilder<String>(
        future: widget.lookupFuture,
        builder: (context, snapshot) {
          if (snapshot.connectionState != ConnectionState.done) {
            return Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const SizedBox(height: 16),
                  Text(
                    'Finding device.',
                    style: TextStyle(
                      color: context.colors.primaryFont,
                      fontFamily: 'Rubik',
                      fontSize: 20,
                      fontWeight: FontWeight(500),
                    ),
                  ),
                  const SizedBox(height: 24),
                  const Spinner(),
                ],
              ),
            );
          }

          if (snapshot.hasError) {
            return Center(
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 24),
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Text(
                      'Invalid code.',
                      style: TextStyle(
                        color: context.colors.primaryFont,
                        fontSize: 20,
                        fontFamily: 'Rubik',
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 24),
                    Text(
                      'Make sure the code you entered is correct and that you have an Internet connection.',
                      textAlign: TextAlign.center,
                      style: TextStyle(
                        color: context.colors.secondaryFont,
                        fontSize: 16,
                        fontFamily: 'Rubik',
                        fontWeight: FontWeight.w400,
                      ),
                    ),
                    const SizedBox(height: 24),
                    TextButton.icon(
                      // The code field is cleared by CodeScreen after pop.
                      onPressed: () => Navigator.of(context).pop(),
                      label: Text(
                        'Try again',
                        style: TextStyle(
                          color: context.colors.secondaryFont,
                          fontSize: 14,
                        ),
                      ),
                      icon: Icon(
                        Icons.refresh,
                        color: context.colors.secondaryFont,
                        size: 16,
                      ),
                    ),
                  ],
                ),
              ),
            );
          }

          return _ConnectingBody(
            status: _currentStatus,
            onInit: _startConnection,
          );
        },
      ),
    );
  }
}

class _ConnectingBody extends StatefulWidget {
  final AddRemoteDeviceEvent? status;
  final VoidCallback onInit;

  const _ConnectingBody({required this.status, required this.onInit});

  @override
  State<_ConnectingBody> createState() => _ConnectingBodyState();
}

class _ConnectingBodyState extends State<_ConnectingBody> {
  @override
  void initState() {
    super.initState();
    widget.onInit();
  }

  String get _statusLabel => switch (widget.status) {
    null => 'Connecting…',
    AddRemoteDeviceEvent_WaitingForResponse() => 'Waiting for response.',
    AddRemoteDeviceEvent_ConnectionFailed() => 'Connection failed.',
    AddRemoteDeviceEvent_Connected() => 'Connected.',
    AddRemoteDeviceEvent_Rejected() => 'Connection rejected.',
    AddRemoteDeviceEvent_Error(:final field0) => 'Error: $field0',
  };

  bool get _showSpinner => switch (widget.status) {
    AddRemoteDeviceEvent_Connected() => false,
    AddRemoteDeviceEvent_Rejected() => false,
    AddRemoteDeviceEvent_Error() => false,
    _ => true,
  };

  bool get _isConnected => widget.status is AddRemoteDeviceEvent_Connected;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Text(
            _isConnected ? 'Connected.' : 'Device found.',
            style: TextStyle(
              color: context.colors.primaryFont,
              fontSize: 20,
              fontFamily: 'Rubik',
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 24),
          if (!_isConnected)
            Text(
              _statusLabel,
              style: TextStyle(
                color: context.colors.primaryFont,
                fontSize: 14,
                fontFamily: 'Inter',
                fontWeight: FontWeight.w500,
              ),
            ),
          const SizedBox(height: 24),
          SizedBox(
            height: 36,
            child: _showSpinner
                ? const Spinner()
                : TextButton.icon(
                    onPressed: () => Navigator.of(
                      context,
                      rootNavigator: true,
                    ).popUntil((route) => route.isFirst),
                    label: Text(
                      'Go to devices',
                      style: TextStyle(
                        color: context.colors.secondaryFont,
                        fontSize: 14,
                      ),
                    ),
                  ),
          ),
        ],
      ),
    );
  }
}

class QrScreen extends StatelessWidget {
  final String ticket;
  const QrScreen({super.key, required this.ticket});

  Future<void> _openScanner(BuildContext context) async {
    final String? scannedCode = await Navigator.of(
      context,
    ).push<String>(FastRoute(builder: (_) => const _QrScannerScreen()));

    if (scannedCode == null || !context.mounted) return;

    // TODO: wire scannedCode into your existing lookup/connect flow
    debugPrint('🟦 Scanned code: $scannedCode');
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leading: IconButton(
          icon: const Icon(Icons.chevron_left),
          onPressed: () => Navigator.of(context).pop(),
          iconSize: 20,
          padding: EdgeInsets.zero,
        ),
        titleSpacing: 0,
        title: Text(
          'Scan QR Code',
          style: TextStyle(
            fontFamily: 'Rubik',
            fontWeight: FontWeight.w600,
            fontSize: 24,
          ),
        ),
        backgroundColor: context.colors.primaryCol,
      ),
      backgroundColor: context.colors.primaryCol,
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            ClipRRect(
              borderRadius: BorderRadius.circular(20),
              child: QrImageView(
                data: ticket,
                version: QrVersions.auto,
                size: 240,
                backgroundColor: Colors.white,
                padding: EdgeInsets.all(16),
              ),
            ),
            const SizedBox(height: 36),
            Text(
              "Scan to connect your mobile device",
              style: TextStyle(
                fontFamily: 'Rubik',
                fontWeight: FontWeight.w500,
                fontSize: 16,
                color: context.colors.primaryFont,
              ),
            ),
            const SizedBox(height: 12),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 32),
              child: Text(
                "Open Nectan mobile app and scan this code to link your device.",
                textAlign: TextAlign.center,
                style: TextStyle(
                  fontFamily: 'Rubik',
                  fontWeight: FontWeight.w400,
                  fontSize: 15,
                  color: context.colors.secondaryFont,
                ),
              ),
            ),
            const SizedBox(height: 48),
            TextButton.icon(
              onPressed: () => _openScanner(context),
              icon: Icon(
                Icons.qr_code_scanner_rounded,
                color: context.colors.primaryFont,
                size: 18,
              ),
              label: Text(
                'Scan a code instead',
                style: TextStyle(
                  color: context.colors.primaryFont,
                  fontSize: 14,
                  fontWeight: FontWeight.w600,
                  fontFamily: "Rubik",
                ),
              ),
            ),
            const SizedBox(height: 12),
          ],
        ),
      ),
    );
  }
}

class _QrScannerScreen extends StatefulWidget {
  const _QrScannerScreen();

  @override
  State<_QrScannerScreen> createState() => _QrScannerScreenState();
}

class _QrScannerScreenState extends State<_QrScannerScreen> {
  final MobileScannerController _controller = MobileScannerController();
  bool _handled = false;

  void _onDetect(BarcodeCapture capture) {
    if (_handled) return;
    final barcodes = capture.barcodes;
    if (barcodes.isEmpty) return;

    final String? value = barcodes.first.rawValue;
    if (value == null) return;

    _handled = true;
    Navigator.of(context).pop(value);
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        backgroundColor: Colors.black,
        leading: IconButton(
          icon: const Icon(Icons.chevron_left, color: Colors.white),
          onPressed: () => Navigator.of(context).pop(),
          iconSize: 20,
          padding: EdgeInsets.zero,
        ),
        titleSpacing: 0,
        title: const Text(
          'Scan code',
          style: TextStyle(
            fontFamily: 'Rubik',
            fontWeight: FontWeight.w600,
            fontSize: 20,
            color: Colors.white,
          ),
        ),
        actions: [
          IconButton(
            icon: ValueListenableBuilder(
              valueListenable: _controller,
              builder: (context, state, child) {
                return Icon(
                  state.torchState == TorchState.on
                      ? Icons.flash_on_rounded
                      : Icons.flash_off_rounded,
                  color: Colors.white,
                );
              },
            ),
            onPressed: () => _controller.toggleTorch(),
          ),
        ],
      ),
      body: MobileScanner(controller: _controller, onDetect: _onDetect),
    );
  }
}

class ConnectionRequestDialog extends StatefulWidget {
  final ConnectionRequest request;
  const ConnectionRequestDialog({required this.request, super.key});

  @override
  State<ConnectionRequestDialog> createState() =>
      _ConnectionRequestDialogState();
}

class _ConnectionRequestDialogState extends State<ConnectionRequestDialog> {
  @override
  void dispose() {
    respondToConnectionReq(accept: false);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
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
                    '${widget.request.username}\n is trying to connect',
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
                    'Accept new connection.',
                    style: TextStyle(
                      color: context.colors.secondaryFont,
                      fontSize: 14,
                    ),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    "This action won't transfer any files.",
                    style: TextStyle(
                      color: context.colors.secondaryFont,
                      fontSize: 14,
                    ),
                  ),
                  const Spacer(),

                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      SizedBox(
                        width: 140,
                        child: FilledButton(
                          onPressed: () {
                            respondToConnectionReq(accept: false);
                            Navigator.pop(context);
                          },
                          style: FilledButton.styleFrom(
                            backgroundColor: const Color(0xffDC3545),
                            foregroundColor: Colors.white,
                            padding: const EdgeInsets.symmetric(vertical: 16),
                            shape: RoundedRectangleBorder(
                              borderRadius: BorderRadius.circular(12),
                            ),
                          ),
                          child: const Text(
                            'Reject',
                            style: TextStyle(
                              fontFamily: "Rubik",
                              fontWeight: FontWeight.w600,
                              fontSize: 16,
                            ),
                          ),
                        ),
                      ),

                      const SizedBox(width: 16),
                      SizedBox(
                        width: 140,
                        child: FilledButton(
                          onPressed: () {
                            respondToConnectionReq(accept: true);
                            Navigator.pop(context);
                          },
                          style: FilledButton.styleFrom(
                            backgroundColor: context.colors.primaryFont,
                            foregroundColor: context.colors.primaryCol,
                            padding: const EdgeInsets.symmetric(vertical: 16),
                            shape: RoundedRectangleBorder(
                              borderRadius: BorderRadius.circular(12),
                            ),
                          ),
                          child: const Text(
                            'Accept',
                            style: TextStyle(
                              fontFamily: "Rubik",
                              fontWeight: FontWeight.w600,
                              fontSize: 16,
                            ),
                          ),
                        ),
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
