import 'dart:io';

import 'package:flutter/material.dart';
import 'package:nectan_mobile/main.dart';
import 'package:nectan_mobile/src/rust/api/simple.dart';
import 'dart:async';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:nectan_mobile/transfers.dart';
import 'package:permission_handler/permission_handler.dart';

class _SectionLabel extends StatelessWidget {
  const _SectionLabel(this.text);
  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(4, 0, 0, 8),
      child: Text(
        text.toUpperCase(),
        style: const TextStyle(
          color: Color(0xff888888),
          fontSize: 12,
          fontFamily: 'Rubik',
          fontWeight: FontWeight.w600,
          letterSpacing: 0.6,
        ),
      ),
    );
  }
}

class _SettingsRow extends StatelessWidget {
  const _SettingsRow({
    required this.label,
    this.subtitle,
    this.trailing,
    this.onTap,
    this.isDestructive = false,
  });

  final String label;
  final String? subtitle;
  final Widget? trailing;
  final VoidCallback? onTap;
  final bool isDestructive;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(8),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    label,
                    style: TextStyle(
                      color: isDestructive
                          ? const Color(0xffff3b30)
                          : context.colors.primaryFont,
                      fontSize: 15,
                      fontWeight: FontWeight(500),
                      fontFamily: "Rubik",
                    ),
                  ),
                  if (subtitle != null) ...[
                    const SizedBox(height: 3),
                    Text(
                      subtitle!,
                      style: TextStyle(
                        color: context.colors.secondaryFont,
                        fontSize: 14,
                        fontWeight: FontWeight.w400,
                        fontFamily: "Inter",
                      ),
                    ),
                  ],
                ],
              ),
            ),
            trailing ??
                Icon(
                  Icons.chevron_right,
                  color: context.colors.primaryFont,
                  size: 20,
                ),
          ],
        ),
      ),
    );
  }
}

class _RowDivider extends StatelessWidget {
  const _RowDivider();
  @override
  Widget build(BuildContext context) {
    return Divider(
      height: 1,
      thickness: 1,
      color: context.colors.thirdCol,
      indent: 16,
      endIndent: 16,
    );
  }
}

class _SettingsCard extends StatelessWidget {
  const _SettingsCard({required this.children});
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      decoration: BoxDecoration(
        color: context.colors.secondaryCol,
        borderRadius: BorderRadius.circular(12),
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(12),
        child: Column(children: children),
      ),
    );
  }
}

class _PromoCard extends StatelessWidget {
  const _PromoCard({
    required this.title,
    required this.subtitle,
    required this.onTap,
    this.icon,
  });

  final String title;
  final String subtitle;
  final VoidCallback onTap;
  final Widget? icon;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: double.infinity,
        height: 80,
        padding: const EdgeInsets.symmetric(horizontal: 20),
        decoration: BoxDecoration(
          color: const Color(0xff272727),
          borderRadius: BorderRadius.circular(12),
        ),
        child: Row(
          children: [
            if (icon != null) ...[icon!, const SizedBox(width: 12)],
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: TextStyle(
                      color: context.colors.primaryFont,
                      fontSize: 18,
                      fontFamily: 'Rubik',
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 3),
                  Text(
                    subtitle,
                    style: TextStyle(
                      color: context.colors.secondaryFont,
                      fontFamily: 'Inter',
                      fontSize: 14,
                      fontWeight: FontWeight.w400,
                    ),
                    overflow: TextOverflow.ellipsis,
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

void showActionSheet({
  required BuildContext context,
  required String title,
  required String subtitle,
  required String actionLabel,
  required VoidCallback onConfirm,
}) {
  showModalBottomSheet(
    context: context,
    backgroundColor: Colors.transparent,
    isScrollControlled: true,
    useSafeArea: true,
    builder: (_) => Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 16 + 32),
      child: Container(
        decoration: BoxDecoration(
          color: const Color(0xff1e1e1e),
          borderRadius: BorderRadius.circular(16),
        ),
        padding: const EdgeInsets.fromLTRB(20, 28, 20, 20),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              title,
              style: TextStyle(
                color: context.colors.primaryFont,
                fontSize: 18,
                fontFamily: 'Rubik',
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 8),
            Text(
              subtitle,
              textAlign: TextAlign.center,
              style: const TextStyle(
                color: Color(0xff888888),
                fontSize: 14,
                fontFamily: 'Inter',
                fontWeight: FontWeight.w400,
              ),
            ),
            const SizedBox(height: 24),
            SizedBox(
              width: double.infinity,
              height: 52,
              child: ElevatedButton(
                onPressed: () {
                  Navigator.of(context).pop();
                  onConfirm();
                },
                style: ElevatedButton.styleFrom(
                  backgroundColor: const Color(0xffff4d4d),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                  ),
                  elevation: 0,
                ),
                child: Text(
                  actionLabel,
                  style: const TextStyle(
                    color: Colors.white,
                    fontSize: 16,
                    fontFamily: 'Rubik',
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ),
            const SizedBox(height: 8),
            SizedBox(
              width: double.infinity,
              height: 52,
              child: TextButton(
                onPressed: () => Navigator.of(context).pop(),
                style: TextButton.styleFrom(
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                  ),
                ),
                child: const Text(
                  'Cancel',
                  style: TextStyle(
                    color: Color(0xff888888),
                    fontSize: 16,
                    fontFamily: "Rubok",
                    fontWeight: FontWeight.w500,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    ),
  );
}

class SettingsScreen extends ConsumerStatefulWidget {
  const SettingsScreen({super.key});
  @override
  ConsumerState<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends ConsumerState<SettingsScreen>
    with WidgetsBindingObserver {
  PermissionStatus? notificationPermission;

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) _loadPermissionsState();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
    Future.microtask(() async {
      // ref.listenManual(currentTabProvider, (prev, next)=>{
      //
      // }));
      ref.listenManual(currentTabProvider, ((previous, next) {
        if (next == 2) {
          debugPrint("🟦 Settings refreshing permissions.");
          _loadPermissionsState();
        }
      }));
      ref.read(userInfoProvider.notifier).refresh();
    });
    _loadPermissionsState();
    WidgetsBinding.instance.addObserver(this);
  }

  // chang whole bluetooth to global notifier provider?
  // change it to subscription? -> useless?
  Future<void> _loadPermissionsState() async {
    if (!Platform.isLinux) {
      debugPrint("Settings. Permissions state.");

      final perm = await Permission.notification.status;
      debugPrint("🟨 Notification permission: $perm");
      setState(() {
        notificationPermission = perm;
      });
    } else {
      debugPrint("Omiiting permission check LINUX");
    }
  }

  @override
  Widget build(BuildContext context) {
    final userInfo = ref.watch(userInfoProvider);
    final theme = ref.watch(themeModeProvider);
    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      body: SafeArea(
        child: SingleChildScrollView(
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 720),
              child: Padding(
                padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(0, 24, 0, 16),
                      child: SizedBox(
                        height: 48,
                        child: GestureDetector(
                          onTap: () => Navigator.of(context).push(
                            MaterialPageRoute(
                              builder: (_) => const ChangeUsernameScreen(),
                            ),
                          ),
                          child: Text(
                            userInfo.username ?? "user",
                            overflow: TextOverflow.ellipsis,
                            maxLines: 1,
                            style: TextStyle(
                              fontFamily: 'Rubik',
                              fontSize: 30,
                              fontWeight: FontWeight.w600,
                              color: context.colors.primaryFont,
                            ),
                          ),
                        ),
                      ),
                    ),
                    const SizedBox(height: 12),
                    if (notificationPermission != null &&
                        notificationPermission != PermissionStatus.granted) ...[
                      _PromoCard(
                        title: 'Enable notifications',
                        subtitle: "Keep transfers alive in background",
                        onTap: () async {
                          final permission = await Permission.notification
                              .request();
                          debugPrint(
                            "🟦 Notification permission status: $permission",
                          );
                          setState(() {
                            notificationPermission = permission;
                          });
                          if (permission != PermissionStatus.granted) {
                            // show modal if its not greanted
                            if (context.mounted) {
                              showNotificationsPermissionDeniedModal(
                                context,
                                permission,
                              );
                            }
                          }
                        },
                      ),
                    ],
                    const SizedBox(height: 28),

                    const _SectionLabel('Account'),
                    _SettingsCard(
                      children: [
                        _SettingsRow(
                          label: 'Username',
                          subtitle: userInfo.username,
                          onTap: () => Navigator.of(context).push(
                            MaterialPageRoute(
                              builder: (_) => const ChangeUsernameScreen(),
                            ),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 28),
                    const _SectionLabel('App settings'),
                    _SettingsCard(
                      children: [
                        _SettingsRow(
                          label: 'Theme',
                          subtitle: theme.isLight ? "Light" : "Dark",
                          onTap: () =>
                              ref.read(themeModeProvider.notifier).toggle(),
                        ),
                      ],
                    ),

                    const SizedBox(height: 24),
                    const _SectionLabel('About'),
                    _SettingsCard(
                      children: [
                        _SettingsRow(
                          label: 'Version',
                          trailing: Text(
                            '1',
                            style: TextStyle(
                              color: context.colors.primaryFont,
                              fontSize: 14,
                              fontFamily: 'Rubik',
                            ),
                          ),
                        ),
                        const _RowDivider(),
                        _SettingsRow(
                          label: 'Terms of Service',
                          onTap: () {
                            // FastRoute(builder: (context) => const ToSScreen());
                            Navigator.of(context).push(
                              FastRoute(
                                builder: (context) => const ToSScreen(),
                              ),
                            );
                          },
                        ),
                        const _RowDivider(),

                        _SettingsCard(
                          children: [
                            _SettingsRow(
                              label: 'Privacy Policy',
                              onTap: () {
                                Navigator.of(context).push(
                                  FastRoute(
                                    builder: (context) =>
                                        const PrivacyPolicyScreen(),
                                  ),
                                );
                              },
                            ),
                          ],
                        ),
                      ],
                    ),
                    const SizedBox(height: 24),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class ChangeUsernameScreen extends ConsumerStatefulWidget {
  const ChangeUsernameScreen({super.key});

  @override
  ConsumerState<ChangeUsernameScreen> createState() => _ChangeUsernameState();
}

class _ChangeUsernameState extends ConsumerState<ChangeUsernameScreen> {
  final TextEditingController _controller = TextEditingController();
  final FocusNode _focusNode = FocusNode();

  bool _isValid = false;

  @override
  void initState() {
    super.initState();
    _controller.addListener(_validateInput);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _focusNode.requestFocus();
    });
  }

  @override
  void dispose() {
    _focusNode.dispose();
    _controller.dispose();
    super.dispose();
  }

  void _validateInput() {
    final text = _controller.text;
    setState(() {
      _isValid = text.isNotEmpty && text.length <= 32;
    });
  }

  Future<void> _handleSave() async {
    final username = _controller.text;
    await saveUsername(username: username);
    ref.read(userInfoProvider.notifier).updateUsername(username);
    if (!mounted) return;
    Navigator.of(context).pop();
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
            children: [
              const Spacer(),
              const Text(
                'Nectan',
                style: TextStyle(
                  fontSize: 48,
                  fontWeight: FontWeight.bold,
                  fontFamily: 'Rubik',
                ),
              ),
              const SizedBox(height: 8),
              Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 300),
                  child: Text(
                    'Username will be visible to others after you restart application.',
                    style: TextStyle(
                      fontSize: 14,
                      color: context.colors.secondaryFont,
                      fontWeight: FontWeight.w400,
                      fontFamily: "Rubik",
                    ),
                    textAlign: TextAlign.center,
                  ),
                ),
              ),
              const Spacer(),
              TextField(
                controller: _controller,
                maxLength: 32,
                maxLines: 1,
                showCursor: true,
                autofocus: true,
                focusNode: _focusNode,
                cursorColor: context.colors.primaryFont,
                cursorWidth: 1,
                textAlign: TextAlign.center,
                style: TextStyle(
                  color: context.colors.primaryFont,
                  fontSize: 16,
                  fontWeight: FontWeight.w500,
                  decoration: TextDecoration.none,
                  fontFamily: "Rubik",
                ),
                decoration: InputDecoration(
                  hintText: 'Enter username',
                  hintStyle: TextStyle(
                    color: context.colors.secondaryFont,
                    fontSize: 16,
                    fontWeight: FontWeight.w400,
                    fontFamily: "Rubik",
                  ),
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
              const Spacer(),
              PrimaryWideButton(
                label: "Save",
                onPressed: () => {_isValid ? _handleSave() : null},
              ),
              const SizedBox(height: 20),
              SizedBox(
                height: 24,
                child: GestureDetector(
                  onTap: () {
                    if (!mounted) return;
                    Navigator.of(context).pop();
                  },
                  child: Text(
                    "Cancel",
                    style: TextStyle(
                      fontFamily: "Rubik",
                      fontSize: 15,
                      color: context.colors.secondaryFont,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ),
              ),

              const Spacer(),
            ],
          ),
        ),
      ),
    );
  }
}

class PrivacyPolicyScreen extends StatelessWidget {
  const PrivacyPolicyScreen({super.key});

  @override
  Widget build(BuildContext context) {
    TextStyle bodyStyle = TextStyle(
      fontFamily: 'Rubik',
      fontSize: 14,
      color: context.colors.secondaryFont,
      height: 1.5,
      fontWeight: FontWeight(400),
    );

    TextStyle headingStyle = TextStyle(
      fontFamily: 'Rubik',
      fontSize: 20,
      fontWeight: FontWeight.bold,
      color: context.colors.primaryFont,
    );

    return Scaffold(
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
          'Terms Of Service',
          style: TextStyle(
            fontFamily: 'Rubik',
            fontWeight: FontWeight.w600,
            fontSize: 24,
          ),
        ),
      ),
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: SingleChildScrollView(
                padding: const EdgeInsets.symmetric(
                  horizontal: 20,
                  vertical: 10,
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Last Updated: 20.09.2026\n', style: bodyStyle),
                    Text(
                      'At Nectan, your privacy is important to us. This Privacy Policy explains how we collect, use, disclose, and safeguard your information when you use our mobile application and related services (collectively, the "Service").\n\n'
                      'By using the Service, you consent to the practices described in this policy.',
                      style: bodyStyle,
                    ),
                    const Divider(color: Colors.white24, height: 32),

                    Text('1. Information We Collect', style: headingStyle),
                    const SizedBox(height: 8),
                    Text(
                      "When you first launch Nectan, a random user ID is generated on your device "
                      "and sent to Nectan servers. It is used to identify your device and "
                      "authenticate the device information sent when connecting. "
                      "The ID is not linked to your name, email, or any account, and is not shared "
                      "with third parties. Clearing the app's data or reinstalling the app "
                      "generates a new ID.",
                      style: bodyStyle,
                    ),
                  ],
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class ToSScreen extends StatelessWidget {
  const ToSScreen({super.key});

  @override
  Widget build(BuildContext context) {
    TextStyle bodyStyle = TextStyle(
      fontFamily: 'Rubik',
      fontSize: 16,
      color: context.colors.secondaryFont,
      height: 1.5,
    );

    TextStyle headingStyle = TextStyle(
      fontFamily: 'Rubik',
      fontSize: 20,
      fontWeight: FontWeight.bold,
      color: context.colors.primaryFont,
    );

    return Scaffold(
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
        title: Text(
          'Terms Of Service',
          style: TextStyle(
            fontFamily: 'Rubik',
            color: context.colors.primaryFont,
            fontWeight: FontWeight.w600,
            fontSize: 24,
          ),
        ),
      ),

      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 1000),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: SingleChildScrollView(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 20,
                      vertical: 10,
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Last Updated: 11.09.2026\n', style: bodyStyle),
                        Text(
                          'Welcome to Nectan. These Terms of Service govern your use of our mobile application, website, and related services (collectively, the "Service"), operated by Nectan.\n\n'
                          'By accessing or using the Service, you agree to be bound by these Terms. If you disagree with any part of the terms, you may not access the Service.',
                          style: bodyStyle,
                        ),
                        const Divider(color: Colors.white24, height: 32),

                        Text(
                          '1. Account Creation and Privacy',
                          style: headingStyle,
                        ),
                        const SizedBox(height: 8),
                        Text(
                          "Registration: To use Nectan you don't have to provide an email address. Your account is tied to randomly generated user id not tied to any of your personal information. To learn more visit https://nectan.co/help",
                          style: bodyStyle,
                        ),
                        const SizedBox(height: 24),

                        Text('2. Acceptable Use', style: headingStyle),
                        const SizedBox(height: 8),
                        Text(
                          'You agree not to use the Service to violate any laws, infringe upon intellectual property rights, interfere with system integrity, or use automated scripts/data mining tools within the App.',
                          style: bodyStyle,
                        ),
                        const SizedBox(height: 24),
                        Text(
                          '3. Limitation of Liability & Disclaimer',
                          style: headingStyle,
                        ),
                        const SizedBox(height: 8),
                        Text(
                          'The Service is provided on an "AS IS" and "AS AVAILABLE" basis. In no event shall we be liable for any indirect, incidental, special, consequential, or punitive damages arising out of your use of the Service.',
                          style: bodyStyle,
                        ),
                        const SizedBox(height: 24),

                        Text('4. Changes to Terms', style: headingStyle),
                        const SizedBox(height: 8),
                        Text(
                          'We reserve the right to modify these Terms at any time. If a revision is material, we will provide notice prior to any new terms taking effect. By continuing to use the App, you agree to the updated terms.',
                          style: bodyStyle,
                        ),
                        const SizedBox(height: 24),

                        Text('5. Contact Us', style: headingStyle),
                        const SizedBox(height: 8),
                        Text(
                          'If you have any questions about these Terms, please contact us at: support@nectan.co',
                          style: bodyStyle,
                        ),
                        const SizedBox(height: 40),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
