import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:nectan_mobile/main.dart';
import 'package:shared_preferences/shared_preferences.dart';

final deviceEmojisProvider =
    NotifierProvider<DeviceEmojisNotifier, Map<String, String>>(
      DeviceEmojisNotifier.new,
    );

class DeviceEmojisNotifier extends Notifier<Map<String, String>> {
  static const _prefix = 'device_emoji_';

  @override
  Map<String, String> build() {
    _load();
    return {};
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    final emojis = <String, String>{};

    for (final key in prefs.getKeys()) {
      if (key.startsWith(_prefix)) {
        final deviceId = key.substring(_prefix.length);
        final emoji = prefs.getString(key);

        if (emoji != null) {
          emojis[deviceId] = emoji;
        }
      }
    }

    state = emojis;
  }

  String emojiFor(String deviceId) {
    return state[deviceId] ?? '💻';
  }

  Future<void> setEmoji(String deviceId, String emoji) async {
    state = {...state, deviceId: emoji};

    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('$_prefix$deviceId', emoji);
  }

  Future<void> resetEmoji(String deviceId) async {
    final updated = {...state};
    updated.remove(deviceId);
    state = updated;

    final prefs = await SharedPreferences.getInstance();
    await prefs.remove('$_prefix$deviceId');
  }
}

class DeviceEmojiPicker extends StatefulWidget {
  final String selectedEmoji;
  final ValueChanged<String> onSelected;
  final VoidCallback onReset;

  const DeviceEmojiPicker({
    super.key,
    required this.selectedEmoji,
    required this.onSelected,
    required this.onReset,
  });

  static const Map<String, List<String>> categories = {
    'Technology': [
      '💻',
      '🖥️',
      '🖨️',
      '⌨️',
      '🖱️',
      '🖲️',
      '💽',
      '💾',
      '💿',
      '📀',
      '📱',
      '📲',
      '☎️',
      '📞',
      '📟',
      '📠',
      '🔋',
      '🪫',
      '🔌',
      '🛜',
      '📡',
      '🧮',
      '⚙️',
      '🔧',
      '🔩',
      '🪛',
      '🧰',
      '🔍',
      '🔬',
      '🧪',
      '🖥️',
      '🗜️',
      '📶',
      '🛰️',
      '💡',
      '🔦',
      '🪫',
      '🔐',
      '🔒',
      '🔓',
      '🗝️',
      '🧲',
    ],
    'Gaming': [
      '🎮',
      '🕹️',
      '👾',
      '🎯',
      '🎲',
      '🧩',
      '♟️',
      '🏆',
      '🥇',
      '🎰',
      '🃏',
      '🪄',
      '🧸',
      '🪅',
      '🎳',
      '🎱',
      '🏅',
      '🎖️',
      '🛹',
      '🎴',
      '🪀',
      '🪁',
      '🧠',
      '👻',
    ],
    'Audio & Video': [
      '🎧',
      '🎤',
      '🎙️',
      '🎚️',
      '🎛️',
      '📻',
      '📺',
      '📷',
      '📸',
      '📹',
      '🎥',
      '📽️',
      '🎬',
      '🔊',
      '🔉',
      '🔈',
      '🔇',
      '🎵',
      '🎶',
      '💿',
      '📢',
      '📣',
      '🎼',
      '🎹',
      '🥁',
      '🎸',
      '🎺',
      '🎻',
      '🪇',
      '🪈',
    ],
    'Animals': [
      '🐶',
      '🐱',
      '🐭',
      '🐹',
      '🐰',
      '🦊',
      '🐻',
      '🐼',
      '🐨',
      '🐯',
      '🦁',
      '🐮',
      '🐷',
      '🐸',
      '🐵',
      '🐧',
      '🐦',
      '🦉',
      '🦆',
      '🦅',
      '🦋',
      '🐢',
      '🐍',
      '🐙',
      '🦀',
      '🐬',
      '🐳',
      '🦈',
      '🦄',
      '🐲',
      '🐴',
      '🦓',
      '🦒',
      '🦘',
      '🦥',
      '🦦',
      '🦔',
      '🐿️',
      '🦇',
      '🐺',
      '🦃',
      '🦚',
      '🦜',
      '🦩',
      '🐝',
      '🐞',
      '🐛',
      '🪲',
    ],
    'Transport': [
      '🚗',
      '🚕',
      '🚙',
      '🚌',
      '🚎',
      '🏎️',
      '🚓',
      '🚑',
      '🚒',
      '🚐',
      '🚚',
      '🚛',
      '🚜',
      '🏍️',
      '🛵',
      '🚲',
      '🛴',
      '🚂',
      '✈️',
      '🚀',
      '🛸',
      '🚁',
      '🚢',
      '⛵',
      '🚤',
      '🛥️',
      '🚆',
      '🚇',
      '🚊',
      '🚉',
      '🚡',
      '🚠',
      '🚃',
      '🚄',
      '🚅',
      '🛩️',
      '🛫',
      '🛬',
      '🪂',
      '🛶',
      '⛽',
      '🚦',
    ],
    'Work & Places': [
      '🏠',
      '🏡',
      '🏢',
      '🏬',
      '🏭',
      '🏦',
      '🏫',
      '🏥',
      '🏨',
      '🏪',
      '🏛️',
      '⛪',
      '🏰',
      '🗼',
      '🗽',
      '🏗️',
      '🛖',
      '🏕️',
      '💼',
      '📁',
      '📂',
      '📚',
      '📖',
      '📝',
      '📋',
      '📌',
      '📎',
      '🗂️',
      '🗃️',
      '🗄️',
      '🏘️',
      '🏙️',
      '🌆',
      '🌇',
      '🏚️',
      '🏛️',
      '🛒',
      '🏟️',
      '🏰',
      '⛺',
      '🏤',
      '🏣',
    ],
    'Nature': [
      '🌍',
      '🌎',
      '🌏',
      '🌙',
      '☀️',
      '⭐',
      '🌟',
      '✨',
      '⚡',
      '🔥',
      '💧',
      '🌈',
      '☁️',
      '🌧️',
      '❄️',
      '🌊',
      '🌲',
      '🌳',
      '🌴',
      '🌵',
      '🌷',
      '🌻',
      '🌹',
      '🍀',
      '🌸',
      '🌺',
      '🌼',
      '🌾',
      '🍁',
      '🍂',
      '🌋',
      '🏔️',
      '⛰️',
      '🌄',
      '🌅',
      '🌌',
      '🌠',
      '🌪️',
      '🌫️',
      '☄️',
      '🪐',
      '🌑',
    ],
    'Food & Drinks': [
      '🍎',
      '🍐',
      '🍊',
      '🍋',
      '🍉',
      '🍇',
      '🍓',
      '🫐',
      '🍒',
      '🍑',
      '🥑',
      '🍕',
      '🍔',
      '🍟',
      '🌮',
      '🍣',
      '🍜',
      '🍩',
      '☕',
      '🍵',
      '🧋',
      '🥤',
      '🍺',
      '🍰',
      '🍍',
      '🥭',
      '🍌',
      '🍈',
      '🥝',
      '🥥',
      '🥕',
      '🌽',
      '🥦',
      '🥐',
      '🥨',
      '🧀',
      '🍪',
      '🍫',
      '🍿',
      '🍯',
      '🧁',
      '🍦',
    ],
    'Symbols': [
      '❤️',
      '🧡',
      '💛',
      '💚',
      '💙',
      '💜',
      '🖤',
      '🤍',
      '🤎',
      '💖',
      '💎',
      '💠',
      '🔴',
      '🟠',
      '🟡',
      '🟢',
      '🔵',
      '🟣',
      '⚪',
      '⚫',
      '🟤',
      '🔶',
      '🔷',
      '🔺',
      '🔻',
      '💯',
      '♻️',
      '🔰',
      '🚩',
      '🏳️',
      '✨',
      '💫',
      '🌟',
      '❇️',
      '✳️',
      '❌',
      '⭕',
      '✅',
      '☑️',
      '➕',
      '➖',
      '➗',
      '💲',
      '🔱',
      '⚜️',
      '🔆',
      '🔅',
      '♾️',
    ],
    'People': [
      '👨',
      '👩',
      '🧑',
      '👨‍💻',
      '👩‍💻',
      '🧑‍💻',
      '👨‍🔧',
      '👩‍🔧',
      '🧑‍🔧',
      '🧑‍🚀',
      '👨‍🚀',
      '👩‍🚀',
      '🤖',
      '👻',
      '💀',
      '🎅',
      '🧙',
      '🧑‍🎨',
      '🧑‍🍳',
      '🧑‍🏫',
      '🧑‍🚒',
      '🧑‍⚕️',
      '🧑‍🔬',
      '🧑‍✈️',
      '🧑‍🌾',
      '🧑‍⚖️',
      '🦸',
      '🦹',
      '🧛',
      '🧟',
      '🧜',
      '🧚',
    ],
    'Sports': [
      '⚽',
      '🏀',
      '🏈',
      '⚾',
      '🥎',
      '🎾',
      '🏐',
      '🏉',
      '🥏',
      '🎱',
      '🏓',
      '🏸',
      '🏒',
      '🏑',
      '🥍',
      '🏏',
      '🥅',
      '⛳',
      '🏹',
      '🎣',
      '🤿',
      '🥊',
      '🥋',
      '🎽',
      '⛸️',
      '🥌',
      '🛷',
      '🎿',
      '⛷️',
      '🏂',
      '🏋️',
      '🤸',
      '🤺',
      '🏇',
      '🧘',
      '🏄',
    ],
    'Objects': [
      '⌚',
      '⏰',
      '⏱️',
      '⏲️',
      '🕰️',
      '📅',
      '📆',
      '🗓️',
      '📇',
      '📈',
      '📉',
      '📊',
      '📦',
      '📫',
      '📬',
      '📮',
      '✉️',
      '📧',
      '🛍️',
      '🎁',
      '🎀',
      '🧳',
      '🪑',
      '🛋️',
      '🛏️',
      '🪞',
      '🪟',
      '🧹',
      '🧺',
      '🧼',
      '🔑',
      '🗝️',
      '🧯',
      '🪜',
      '🪣',
      '🧴',
    ],
  };

  static Future<void> show(
    BuildContext context, {
    required String selectedEmoji,
    required ValueChanged<String> onSelected,
    required VoidCallback onReset,
  }) {
    return showModalBottomSheet(
      context: context,
      useRootNavigator: true,
      backgroundColor: Colors.transparent,
      isScrollControlled: true,
      useSafeArea: true,
      builder: (_) => DeviceEmojiPicker(
        selectedEmoji: selectedEmoji,
        onSelected: onSelected,
        onReset: onReset,
      ),
    );
  }

  @override
  State<DeviceEmojiPicker> createState() => _DeviceEmojiPickerState();
}

class _DeviceEmojiPickerState extends State<DeviceEmojiPicker> {
  String _category = 'Technology';

  @override
  Widget build(BuildContext context) {
    final emojis = DeviceEmojiPicker.categories[_category]!;

    return Material(
      color: context.colors.primaryCol,
      shape: const RoundedSuperellipseBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(20)),
      ),
      clipBehavior: Clip.antiAlias,
      child: SafeArea(
        top: false,
        left: true,
        right: true,
        bottom: true,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 12),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 36,
                height: 4,
                decoration: BoxDecoration(
                  color: context.colors.thirdFont,
                  borderRadius: BorderRadius.circular(4),
                ),
              ),
              const SizedBox(height: 20),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      'Choose icon',
                      style: TextStyle(
                        fontFamily: 'Rubik',
                        fontSize: 18,
                        fontWeight: FontWeight.w600,
                        color: context.colors.primaryFont,
                      ),
                    ),
                  ),
                  TextButton(
                    onPressed: () {
                      widget.onReset();
                      Navigator.of(context).pop();
                    },
                    child: Text(
                      'Reset',
                      style: TextStyle(color: context.colors.secondaryFont),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              SizedBox(
                height: 40,
                child: ListView(
                  scrollDirection: Axis.horizontal,
                  children: DeviceEmojiPicker.categories.keys.map((category) {
                    final selected = category == _category;

                    return Padding(
                      padding: const EdgeInsets.only(right: 8),
                      child: GestureDetector(
                        onTap: () {
                          setState(() => _category = category);
                        },
                        child: AnimatedContainer(
                          duration: const Duration(milliseconds: 120),
                          padding: const EdgeInsets.symmetric(
                            horizontal: 12,
                            vertical: 8,
                          ),
                          decoration: BoxDecoration(
                            color: selected
                                ? context.colors.thirdCol
                                : Colors.transparent,
                            borderRadius: BorderRadius.circular(10),
                            border: Border.all(
                              color: selected
                                  ? context.colors.classicBorderActive
                                  : context.colors.classicBorder,
                            ),
                          ),
                          child: Center(
                            child: Text(
                              category,
                              style: TextStyle(
                                color: context.colors.primaryFont,
                                fontSize: 12,
                                fontWeight: selected
                                    ? FontWeight.w600
                                    : FontWeight.w400,
                              ),
                            ),
                          ),
                        ),
                      ),
                    );
                  }).toList(),
                ),
              ),
              const SizedBox(height: 16),
              SizedBox(
                height: 260,
                child: GridView.builder(
                  padding: EdgeInsets.zero,
                  itemCount: emojis.length,
                  gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
                    crossAxisCount: 7,
                    mainAxisSpacing: 6,
                    crossAxisSpacing: 6,
                  ),
                  itemBuilder: (context, index) {
                    final emoji = emojis[index];
                    final selected = emoji == widget.selectedEmoji;

                    return GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: () {
                        widget.onSelected(emoji);
                        Navigator.of(context).pop();
                      },
                      child: AnimatedContainer(
                        duration: const Duration(milliseconds: 120),
                        decoration: BoxDecoration(
                          color: selected
                              ? context.colors.thirdCol
                              : Colors.transparent,
                          borderRadius: BorderRadius.circular(10),
                          border: Border.all(
                            color: selected
                                ? context.colors.classicBorderActive
                                : Colors.transparent,
                          ),
                        ),
                        child: Center(
                          child: Text(
                            emoji,
                            style: const TextStyle(fontSize: 24),
                          ),
                        ),
                      ),
                    );
                  },
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
