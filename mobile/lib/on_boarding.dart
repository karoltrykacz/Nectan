// import 'package:flutter/material.dart';
// import 'package:flutter_riverpod/flutter_riverpod.dart';
// import 'package:nectan_mobile/devices.dart';
// import 'package:nectan_mobile/main.dart';
//
// class OnboardingScreen extends ConsumerStatefulWidget {
//   const OnboardingScreen({super.key});
//
//   @override
//   ConsumerState<OnboardingScreen> createState() => _OnboardingScreenState();
// }
//
// class _OnboardingScreenState extends ConsumerState<OnboardingScreen> {
//   final PageController _controller = PageController();
//   int _current = 0;
//
//   final _pages = [
//     const _PageData(
//       title: 'Welcome to Nectan',
//       subtitle: 'Free and fast file transfers between devices.',
//     ),
//     const _PageData(
//       title: 'Connecting',
//       subtitle: 'Eaisly connect two devices, using 6 digit code.',
//     ),
//     _PageData(
//       title: 'Sending xd',
//       subtitle: 'Sending is competely free. Nearby.',
//       tip: buildTip(
//         "What are nearby devices?",
//         "Devices found on the same Wi-Fi network or within Bluetooth range.",
//         true,
//       ),
//     ),
//   ];
//
//   void _next() {
//     if (_current < _pages.length - 1) {
//       _controller.nextPage(
//         duration: const Duration(milliseconds: 200),
//         curve: Curves.easeInOut,
//       );
//     } else {
//       _finish();
//     }
//   }
//
//   void _finish() {
//     Navigator.of(
//       context,
//     ).pushReplacement(FastRoute(builder: (_) => const SetupScreen()));
//   }
//
//   @override
//   void dispose() {
//     _controller.dispose();
//     super.dispose();
//   }
//
//   @override
//   Widget build(BuildContext context) {
//     final isLast = _current == _pages.length - 1;
//
//     return Scaffold(
//       backgroundColor: context.colors.primaryCol,
//       body: SafeArea(
//         child: Column(
//           children: [
//             Expanded(
//               child: PageView.builder(
//                 controller: _controller,
//                 itemCount: _pages.length,
//                 onPageChanged: (i) => setState(() => _current = i),
//                 itemBuilder: (_, i) => _OnboardingPage(data: _pages[i]),
//               ),
//             ),
//             Padding(
//               padding: const EdgeInsets.fromLTRB(32, 0, 32, 40),
//               child: Column(
//                 children: [
//                   Row(
//                     mainAxisAlignment: MainAxisAlignment.center,
//                     children: List.generate(_pages.length, (i) {
//                       final active = i == _current;
//                       return AnimatedContainer(
//                         duration: const Duration(milliseconds: 220),
//                         margin: const EdgeInsets.symmetric(horizontal: 3),
//                         width: active ? 18 : 6,
//                         height: 6,
//                         decoration: BoxDecoration(
//                           color: active
//                               ? context.colors.primaryFont
//                               : context.colors.thirdFont,
//                           borderRadius: BorderRadius.circular(3),
//                         ),
//                       );
//                     }),
//                   ),
//                   const SizedBox(height: 20),
//
//                   PrimaryWideButton(label: "Next", onPressed: _next),
//
//                   if (!isLast) ...[
//                     const SizedBox(height: 24),
//                     SizedBox(
//                       height: 24,
//                       child: GestureDetector(
//                         onTap: _finish,
//                         child: Text(
//                           "Skip",
//                           style: TextStyle(
//                             fontFamily: "Rubik",
//                             fontSize: 15,
//                             color: context.colors.thirdFont,
//                             fontWeight: FontWeight.w500,
//                           ),
//                         ),
//                       ),
//                     ),
//                   ] else ...[
//                     const SizedBox(height: 24),
//                     const SizedBox(height: 24),
//                   ],
//                 ],
//               ),
//             ),
//           ],
//         ),
//       ),
//     );
//   }
// }
//
// class _PageData {
//   final String title;
//   final String subtitle;
//   // final IconData? calloutIcon;
//   // final String? calloutTitle;
//   final Widget? tip;
//
//   const _PageData({
//     required this.title,
//     required this.subtitle,
//     this.tip,
//     // required this.calloutIcon,
//     // required this.calloutTitle,
//     // required this.calloutBody,
//   });
// }
//
// class _OnboardingPage extends StatelessWidget {
//   final _PageData data;
//
//   const _OnboardingPage({required this.data});
//
//   @override
//   Widget build(BuildContext context) {
//     return Padding(
//       padding: const EdgeInsets.symmetric(horizontal: 32),
//       child: Column(
//         mainAxisAlignment: MainAxisAlignment.center,
//         children: [
//           const SizedBox(height: 72),
//           Text(
//             data.title,
//             textAlign: TextAlign.center,
//             style: TextStyle(
//               fontFamily: 'Rubik',
//               fontSize: 26,
//               fontWeight: FontWeight.w700,
//               color: context.colors.primaryFont,
//             ),
//           ),
//           const SizedBox(height: 12),
//           Text(
//             data.subtitle,
//             textAlign: TextAlign.center,
//             style: TextStyle(
//               fontFamily: 'Inter',
//               fontSize: 14,
//               color: context.colors.thirdFont,
//               height: 1.6,
//             ),
//           ),
//           if (data.tip != null) ...[const SizedBox(height: 24), data.tip!],
//         ],
//       ),
//     );
//   }
// }
