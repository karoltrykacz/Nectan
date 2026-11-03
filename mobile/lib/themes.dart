import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:nectan_mobile/main.dart';
import 'package:shared_preferences/shared_preferences.dart';

RoundedSuperellipseBorder sq(double radius, {BorderSide? side}) =>
    RoundedSuperellipseBorder(
      borderRadius: BorderRadius.circular(radius),
      side: side ?? BorderSide.none,
    );

AppColors _palette({
  required int primary,
  required int secondary,
  required int third,
  required int font,
  required int font2,
  required int font3,
  required int border,
}) {
  final b = Color(border);
  return AppColors(
    primaryCol: Color(primary),
    secondaryCol: Color(secondary),
    thirdCol: Color(third),
    primaryFont: Color(font),
    secondaryFont: Color(font2),
    thirdFont: Color(font3),
    classicBorder: b.withValues(alpha: 0.5),
    classicBorderActive: b,
  );
}

class AppTheme {
  AppTheme({
    required this.id,
    required this.name,
    required this.brightness,
    required this.colors,
    required this.accent,
    this.isSystem = false,
  });

  final String id;
  final String name;
  final Brightness brightness;
  final AppColors colors;
  final Color accent;
  final bool isSystem;

  late final ThemeData data = ThemeData(
    brightness: brightness,
    fontFamily: 'Rubik',
    colorScheme: ColorScheme.fromSeed(
      seedColor: accent,
      brightness: brightness,
    ),
    scaffoldBackgroundColor: colors.primaryCol,
    extensions: [colors],
  );
}

abstract final class AppThemes {
  static final system = AppTheme(
    id: 'system',
    name: 'System',
    brightness: Brightness.dark,
    colors: darkAppColors,
    accent: Colors.blue,
    isSystem: true,
  );

  static final light = AppTheme(
    id: 'light',
    name: 'Light',
    brightness: Brightness.light,
    colors: lightAppColors,
    accent: Colors.blue,
  );

  static final dark = AppTheme(
    id: 'dark',
    name: 'Dark',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff1a1a1a,
      secondary: 0xff1f1f1f,
      third: 0xff272727,
      font: 0xffffffff,
      font2: 0xffcccccc,
      font3: 0xffaaaaaa,
      border: 0xff404040,
    ),
    accent: const Color(0xff333333),
  );

  static final sand = AppTheme(
    id: 'sand',
    name: 'Sand',
    brightness: Brightness.light,
    colors: _palette(
      primary: 0xfff4efe6,
      secondary: 0xffebe4d6,
      third: 0xffddd3c0,
      font: 0xff2b2418,
      font2: 0xff4a4030,
      font3: 0xff6e6350,
      border: 0xffc5b9a1,
    ),
    accent: const Color(0xffc5b9a1),
  );

  static final solarizedDark = AppTheme(
    id: 'solarized-dark',
    name: 'Solarized Dark',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff002b36,
      secondary: 0xff073642,
      third: 0xff0d4350,
      font: 0xfffdf6e3,
      font2: 0xffeee8d5,
      font3: 0xff93a1a1,
      border: 0xff1a5563,
    ),
    accent: const Color(0xffb58900),
  );

  static final gruvbox = AppTheme(
    id: 'gruvbox',
    name: 'Gruvbox',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff282828,
      secondary: 0xff32302f,
      third: 0xff3c3836,
      font: 0xffebdbb2,
      font2: 0xffd5c4a1,
      font3: 0xffbdae93,
      border: 0xff49483e,
    ),
    accent: const Color(0xfffabd2f),
  );

  static final midnight = AppTheme(
    id: 'midnight',
    name: 'Midnight',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff0b1020,
      secondary: 0xff111a2e,
      third: 0xff1a2742,
      font: 0xfff0f4ff,
      font2: 0xffc4d0e8,
      font3: 0xff8fa5c9,
      border: 0xff2c3d5c,
    ),
    accent: const Color(0xff263653),
  );

  static final broccoli = AppTheme(
    id: 'broccoli',
    name: 'Broccoli',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff004b23,
      secondary: 0xff006400,
      third: 0xff007200,
      font: 0xfff1f1f1,
      font2: 0xffe1e1e1,
      font3: 0xffd1d1d1,
      border: 0xff008000,
    ),
    accent: const Color(0xff008000),
  );

  static final forest = AppTheme(
    id: 'forest',
    name: 'Forest',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff12191a,
      secondary: 0xff182224,
      third: 0xff1f2d2e,
      font: 0xffe6f0e8,
      font2: 0xffc2d6c6,
      font3: 0xff8fb09a,
      border: 0xff2a3d3e,
    ),
    accent: const Color(0xff7fd99a),
  );

  static final pinky = AppTheme(
    id: 'pinky',
    name: 'Pinky',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xffb5566f,
      secondary: 0xffc2677f,
      third: 0xffcd7a90,
      font: 0xffffffff,
      font2: 0xfff1f1f1,
      font3: 0xffe1e1e1,
      border: 0xffcd7a90,
    ),
    accent: const Color(0xffd88fa1),
  );

  static final iceCream = AppTheme(
    id: 'ice-cream',
    name: 'Neapolitan ice cream',
    brightness: Brightness.light,
    colors: _palette(
      primary: 0xfff1c5d4,
      secondary: 0xfffcf7d9,
      third: 0xfff6d8d6,
      font: 0xff614736,
      font2: 0xff614736,
      font3: 0xff614736,
      border: 0xff614736,
    ),
    accent: const Color(0xfff1c5d4),
  );

  static final glamour = AppTheme(
    id: 'glamour',
    name: 'Glamour',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff5b0619,
      secondary: 0xff710d23,
      third: 0xff8b1a29,
      font: 0xfff1f1f1,
      font2: 0xffe1e1e1,
      font3: 0xffd1d1d1,
      border: 0xff8b1a29,
    ),
    accent: const Color(0xffa7252c),
  );

  static final fishy = AppTheme(
    id: 'fishy',
    name: 'Fishy business',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff1b2d61,
      secondary: 0xff234b8f,
      third: 0xff2c6bbe,
      font: 0xfffff9f2,
      font2: 0xffd7f8f8,
      font3: 0xffb1f8ff,
      border: 0xff5d9bff,
    ),
    accent: const Color(0xff5d9bff),
  );

  static final kill = AppTheme(
    id: 'kill',
    name: 'Killer',
    brightness: Brightness.light,
    colors: _palette(
      primary: 0xffe68612,
      secondary: 0xfff8b71e,
      third: 0xfffac31a,
      font: 0xff000000,
      font2: 0xff000000,
      font3: 0xff000000,
      border: 0xfffcd613,
    ),
    accent: const Color(0xfffcd613),
  );

  static final highContrast = AppTheme(
    id: 'high-contrast',
    name: 'High Contrast',
    brightness: Brightness.dark,
    colors: _palette(
      primary: 0xff000000,
      secondary: 0xff0a0a0a,
      third: 0xff141414,
      font: 0xffffffff,
      font2: 0xfff0f0f0,
      font3: 0xffffff66,
      border: 0xffffffff,
    ),
    accent: const Color(0xffffff66),
  );

  static final List<AppTheme> all = [
    system,
    light,
    dark,
    sand,
    glamour,
    solarizedDark,
    gruvbox,
    broccoli,
    fishy,
    midnight,
    forest,
    pinky,
    iceCream,
    kill,
    highContrast,
  ];

  static AppTheme byId(String? id) =>
      all.firstWhere((t) => t.id == id, orElse: () => dark);
}

class AppThemeNotifier extends Notifier<AppTheme> {
  static const _prefsKey = 'app_theme_id';
  static String? initialId;

  static Future<void> preload() async {
    final prefs = await SharedPreferences.getInstance();
    initialId = prefs.getString(_prefsKey);
  }

  @override
  AppTheme build() {
    _restore();
    return AppThemes.byId(initialId);
  }

  Future<void> _restore() async {
    final prefs = await SharedPreferences.getInstance();
    final id = prefs.getString(_prefsKey);
    if (id != null && id != state.id) {
      state = AppThemes.byId(id);
    }
  }

  Future<void> select(AppTheme theme) async {
    state = theme;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_prefsKey, theme.id);
  }
}

final appThemeProvider = NotifierProvider<AppThemeNotifier, AppTheme>(
  AppThemeNotifier.new,
);

final themeModeProvider = Provider<ThemeMode>((ref) {
  final theme = ref.watch(appThemeProvider);
  if (theme.isSystem) return ThemeMode.system;
  return theme.brightness == Brightness.light
      ? ThemeMode.light
      : ThemeMode.dark;
});

class ThemeSwatch extends StatelessWidget {
  const ThemeSwatch({super.key, required this.theme, this.size = 32});

  final AppTheme theme;
  final double size;

  @override
  Widget build(BuildContext context) {
    final border = BorderSide(color: context.colors.classicBorderActive);
    final radius = size * 0.32;

    if (theme.isSystem) {
      return SizedBox(
        width: size,
        height: size,
        child: DecoratedBox(
          decoration: ShapeDecoration(shape: sq(radius, side: border)),
          child: ClipRSuperellipse(
            borderRadius: BorderRadius.circular(radius),
            child: Row(
              children: [
                Expanded(child: ColoredBox(color: lightAppColors.primaryCol)),
                Expanded(child: ColoredBox(color: darkAppColors.primaryCol)),
              ],
            ),
          ),
        ),
      );
    }

    return Container(
      width: size,
      height: size,
      decoration: ShapeDecoration(
        color: theme.colors.primaryCol,
        shape: sq(radius, side: border),
      ),
      child: Center(
        child: Container(
          width: size * 0.42,
          height: size * 0.42,
          decoration: ShapeDecoration(
            color: theme.colors.primaryFont,
            shape: sq(size * 0.14),
          ),
        ),
      ),
    );
  }
}

class ThemeScreen extends ConsumerWidget {
  const ThemeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(appThemeProvider);
    final colors = context.colors;

    return Scaffold(
      backgroundColor: colors.primaryCol,
      appBar: AppBar(
        backgroundColor: colors.primaryCol,
        scrolledUnderElevation: 0,
        leading: IconButton(
          icon: const Icon(Icons.chevron_left),
          onPressed: () => Navigator.of(context).pop(),
          iconSize: 20,
          padding: EdgeInsets.zero,
        ),
        titleSpacing: 0,
        title: Text(
          'Theme',
          style: TextStyle(
            fontFamily: 'Rubik',
            color: colors.primaryFont,
            fontWeight: FontWeight.w600,
            fontSize: 24,
          ),
        ),
      ),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 720),
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(20, 8, 20, 12),
                  child: _ThemePreview(name: selected.name),
                ),
                Expanded(
                  child: ListView.builder(
                    padding: const EdgeInsets.fromLTRB(12, 0, 12, 32),
                    itemCount: AppThemes.all.length,
                    itemBuilder: (context, i) {
                      final theme = AppThemes.all[i];
                      final isSelected = theme.id == selected.id;

                      return ListTile(
                        shape: sq(14),
                        contentPadding: const EdgeInsets.symmetric(
                          horizontal: 12,
                          vertical: 2,
                        ),
                        title: Text(
                          theme.name,
                          style: TextStyle(
                            color: colors.primaryFont,
                            fontFamily: 'Rubik',
                            fontSize: 16,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                        trailing: isSelected
                            ? Icon(
                                Icons.check_rounded,
                                color: colors.primaryFont,
                              )
                            : null,
                        onTap: () {
                          HapticFeedback.selectionClick();
                          ref.read(appThemeProvider.notifier).select(theme);
                        },
                      );
                    },
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

class _ThemePreview extends StatelessWidget {
  const _ThemePreview({required this.name});

  final String name;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(16),
      decoration: ShapeDecoration(
        color: c.secondaryCol,
        shape: sq(20, side: BorderSide(color: c.classicBorderActive)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            name,
            style: TextStyle(
              color: c.primaryFont,
              fontFamily: 'Rubik',
              fontSize: 18,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            'This is how your text will look.',
            style: TextStyle(
              color: c.secondaryFont,
              fontFamily: 'Rubik',
              fontSize: 14,
            ),
          ),
          const SizedBox(height: 2),
          Text(
            'Secondary details and captions',
            style: TextStyle(
              color: c.thirdFont,
              fontFamily: 'Rubik',
              fontSize: 12,
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              const Spacer(),
              Container(
                padding: const EdgeInsets.symmetric(
                  horizontal: 16,
                  vertical: 8,
                ),
                decoration: ShapeDecoration(
                  color: c.primaryFont,
                  shape: sq(12),
                ),
                child: Text(
                  'Example',
                  style: TextStyle(
                    color: c.primaryCol,
                    fontFamily: 'Rubik',
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
