import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:nectan_mobile/main.dart'; // for context.colors

class _NavDestination {
  const _NavDestination(this.label, this.iconBuilder);

  /// Used only for accessibility (screen readers), never shown.
  final String label;
  final Widget Function(Color color) iconBuilder;
}

final List<_NavDestination> _destinations = [
  _NavDestination(
    'Devices',
    (c) => SvgPicture.asset(
      'assets/icons/home.svg',
      width: 22,
      height: 22,
      colorFilter: ColorFilter.mode(c, BlendMode.srcIn),
    ),
  ),
  _NavDestination(
    'Transfers',
    (c) => Icon(Icons.swap_vert_rounded, color: c, size: 24),
  ),
  _NavDestination(
    'Settings',
    (c) => SvgPicture.asset(
      'assets/icons/settings.svg',
      width: 22,
      height: 22,
      colorFilter: ColorFilter.mode(c, BlendMode.srcIn),
    ),
  ),
];

/// Floating, centered, icon-only navigation pill with squircle corners.
///
/// The selection indicator follows [position], the (fractional) page position
/// of the PageView, so it moves in sync while the user swipes the pages. The
/// pill itself can also be dragged: it reports the finger position through
/// [onScrub] and the final snapped tab through [onScrubEnd].
///
/// Use as `Scaffold.bottomNavigationBar` together with `extendBody: true`.
class BottomPillNav extends StatefulWidget {
  const BottomPillNav({
    super.key,
    required this.position,
    required this.onTap,
    required this.onScrubStart,
    required this.onScrub,
    required this.onScrubEnd,
  });

  final ValueListenable<double> position;
  final ValueChanged<int> onTap;
  final VoidCallback onScrubStart;
  final ValueChanged<double> onScrub;
  final ValueChanged<int> onScrubEnd;

  static const double _itemWidth = 64;
  static const double _itemHeight = 42;
  static const double _padding = 4;
  static const double _outerRadius = 24;
  static const double _innerRadius = _outerRadius - _padding;

  @override
  State<BottomPillNav> createState() => _BottomPillNavState();
}

class _BottomPillNavState extends State<BottomPillNav> {
  double _dragPage = 0;
  int _lastIndex = 0;

  int get _maxIndex => _destinations.length - 1;

  void _onDragStart(DragStartDetails details) {
    _dragPage = widget.position.value;
    _lastIndex = _dragPage.round();
    HapticFeedback.selectionClick();
    widget.onScrubStart();
  }

  void _onDragUpdate(DragUpdateDetails details) {
    _dragPage = (_dragPage + details.delta.dx / BottomPillNav._itemWidth).clamp(
      0.0,
      _maxIndex.toDouble(),
    );

    final index = _dragPage.round();
    if (index != _lastIndex) {
      _lastIndex = index;
      HapticFeedback.selectionClick();
    }
    widget.onScrub(_dragPage);
  }

  void _onDragEnd(DragEndDetails details) {
    final velocity = details.primaryVelocity ?? 0;
    final projected = _dragPage + velocity / BottomPillNav._itemWidth * 0.06;
    widget.onScrubEnd(projected.round().clamp(0, _maxIndex).toInt());
  }

  void _onDragCancel() {
    widget.onScrubEnd(_dragPage.round().clamp(0, _maxIndex).toInt());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;

    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: Center(
          heightFactor: 1,
          child: GestureDetector(
            onHorizontalDragStart: _onDragStart,
            onHorizontalDragUpdate: _onDragUpdate,
            onHorizontalDragEnd: _onDragEnd,
            onHorizontalDragCancel: _onDragCancel,
            child: Container(
              padding: const EdgeInsets.all(BottomPillNav._padding),
              decoration: ShapeDecoration(
                color: colors.thirdCol,
                shape: RoundedSuperellipseBorder(
                  borderRadius: BorderRadius.circular(
                    BottomPillNav._outerRadius,
                  ),
                  side: BorderSide(color: colors.classicBorder),
                ),
                shadows: const [
                  BoxShadow(
                    color: Color(0x40000000),
                    blurRadius: 24,
                    offset: Offset(0, 8),
                  ),
                ],
              ),
              child: ValueListenableBuilder<double>(
                valueListenable: widget.position,
                builder: (context, position, _) {
                  return Stack(
                    children: [
                      // Selection indicator, glued to the page position.
                      Positioned(
                        left: position * BottomPillNav._itemWidth,
                        top: 0,
                        width: BottomPillNav._itemWidth,
                        height: BottomPillNav._itemHeight,
                        child: DecoratedBox(
                          decoration: ShapeDecoration(
                            color: colors.primaryFont,
                            shape: RoundedSuperellipseBorder(
                              borderRadius: BorderRadius.circular(
                                BottomPillNav._innerRadius,
                              ),
                            ),
                          ),
                        ),
                      ),
                      Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          for (var i = 0; i < _destinations.length; i++)
                            _PillItem(
                              destination: _destinations[i],
                              selected: position.round() == i,
                              // 1.0 when the indicator is exactly under the
                              // icon, fading to 0.0 one item away.
                              closeness: (1 - (position - i).abs()).clamp(
                                0.0,
                                1.0,
                              ),
                              selectedColor: colors.primaryCol,
                              unselectedColor: colors.secondaryFont,
                              onTap: () => widget.onTap(i),
                            ),
                        ],
                      ),
                    ],
                  );
                },
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _PillItem extends StatelessWidget {
  const _PillItem({
    required this.destination,
    required this.selected,
    required this.closeness,
    required this.selectedColor,
    required this.unselectedColor,
    required this.onTap,
  });

  final _NavDestination destination;
  final bool selected;
  final double closeness;
  final Color selectedColor;
  final Color unselectedColor;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final color = Color.lerp(unselectedColor, selectedColor, closeness)!;

    return Semantics(
      button: true,
      selected: selected,
      label: destination.label,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () {
          if (!selected) HapticFeedback.selectionClick();
          onTap();
        },
        child: SizedBox(
          width: BottomPillNav._itemWidth,
          height: BottomPillNav._itemHeight,
          child: Center(child: destination.iconBuilder(color)),
        ),
      ),
    );
  }
}
