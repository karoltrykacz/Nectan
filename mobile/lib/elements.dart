import 'package:flutter/material.dart';

class Spinner extends StatefulWidget {
  const Spinner({super.key});

  @override
  State<Spinner> createState() => SpinnerState();
}

class SpinnerState extends State<Spinner> with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 400),
  )..repeat();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return RotationTransition(
      turns: _controller,
      child: SizedBox(
        width: 16,
        height: 16,
        child: CustomPaint(painter: SpinnerArcPainter()),
      ),
    );
  }
}

class SpinnerArcPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final center = Offset(size.width / 2, size.height / 2);
    final radius = (size.width - 2) / 2; // inset by strokeWidth/2

    // Full ring (dim)
    canvas.drawCircle(
      center,
      radius,
      Paint()
        ..color = const Color(0x1AFFFFFF)
        ..strokeWidth = 2
        ..style = PaintingStyle.stroke,
    );

    // Arc (bright)
    canvas.drawArc(
      Rect.fromCircle(center: center, radius: radius),
      -1.57,
      1.2,
      false,
      Paint()
        ..color = const Color(0xffbebebe)
        ..strokeWidth = 2
        ..style = PaintingStyle.stroke
        ..strokeCap = StrokeCap.round,
    );
  }

  @override
  bool shouldRepaint(SpinnerArcPainter _) => false;
}
