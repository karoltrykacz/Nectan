import 'dart:async';
import 'dart:math';
import 'dart:ui';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:nectan_mobile/elements.dart';
import 'package:nectan_mobile/l10n_extension.dart';
import 'package:nectan_mobile/main.dart';
import 'package:nectan_mobile/themes.dart';
import 'package:photo_manager/photo_manager.dart';
import 'package:shared_preferences/shared_preferences.dart';

class _AlbumEntry {
  final AssetPathEntity album;
  final String id;
  final String name;
  final int count;
  final Uint8List? cover;

  const _AlbumEntry({
    required this.album,
    required this.id,
    required this.name,
    required this.count,
    this.cover,
  });
}

class GalleryScreen extends StatefulWidget {
  const GalleryScreen({super.key});

  @override
  State<GalleryScreen> createState() => _GalleryScreenState();
}

class _GalleryScreenState extends State<GalleryScreen> {
  List<_AlbumEntry> _albums = [];
  Set<String> _pinned = {};
  bool _loading = true;
  bool _denied = false;

  @override
  void initState() {
    super.initState();
    _init();
  }

  Future<void> _loadPinned() async {
    final prefs = await SharedPreferences.getInstance();
    _pinned = (prefs.getStringList('pinnedAlbums') ?? []).toSet();
  }

  Future<void> _togglePin(String id) async {
    HapticFeedback.selectionClick();
    setState(() {
      if (_pinned.contains(id)) {
        _pinned.remove(id);
      } else {
        _pinned.add(id);
      }
      _sortAlbums(_albums);
    });
    final prefs = await SharedPreferences.getInstance();
    await prefs.setStringList('pinnedAlbums', _pinned.toList());
  }

  void _sortAlbums(List<_AlbumEntry> entries) {
    entries.sort((a, b) {
      final aPinned = _pinned.contains(a.id);
      final bPinned = _pinned.contains(b.id);
      if (aPinned != bPinned) return aPinned ? -1 : 1;
      return a.name.toLowerCase().compareTo(b.name.toLowerCase());
    });
  }

  Future<void> _init() async {
    await _loadPinned();

    final permission = await PhotoManager.requestPermissionExtend();
    if (!permission.isAuth) {
      if (mounted) {
        setState(() {
          _denied = true;
          _loading = false;
        });
      }
      return;
    }

    final albums = await PhotoManager.getAssetPathList(
      type: RequestType.common,
      onlyAll: false,
      filterOption: FilterOptionGroup(
        orders: [
          const OrderOption(type: OrderOptionType.createDate, asc: false),
        ],
      ),
    );

    final results = await Future.wait(
      albums.where((album) => !album.isAll).map((album) async {
        final count = await album.assetCountAsync;
        if (count == 0) return null;
        final first = await album.getAssetListRange(start: 0, end: 1);
        Uint8List? cover;
        if (first.isNotEmpty) {
          cover = await first.first.thumbnailDataWithSize(
            const ThumbnailSize(400, 400),
          );
        }
        return _AlbumEntry(
          album: album,
          id: album.id,
          name: album.name,
          count: count,
          cover: cover,
        );
      }),
    );
    final entries = results.whereType<_AlbumEntry>().toList();

    _sortAlbums(entries);

    if (mounted) {
      setState(() {
        _albums = entries;
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
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
                    Text(
                      context.l10n.galleryTitle,
                      style: TextStyle(
                        fontFamily: 'Rubik',
                        fontSize: 30,
                        fontWeight: FontWeight.w600,
                        color: context.colors.primaryFont,
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 12),
            Expanded(
              child: _loading
                  ? const Center(child: Spinner())
                  : _denied
                  ? Center(
                      child: Text(
                        context.l10n.galleryPermissionDenied,
                        style: TextStyle(
                          color: context.colors.secondaryFont,
                          fontFamily: "Rubik",
                        ),
                      ),
                    )
                  : _albums.isEmpty
                  ? Center(
                      child: Text(
                        context.l10n.noAlbums,
                        style: TextStyle(
                          color: context.colors.secondaryFont,
                          fontFamily: "Rubik",
                        ),
                      ),
                    )
                  : GridView.builder(
                      padding: const EdgeInsets.fromLTRB(12, 0, 12, 120),
                      gridDelegate:
                          const SliverGridDelegateWithFixedCrossAxisCount(
                            crossAxisCount: 2,
                            mainAxisSpacing: 16,
                            crossAxisSpacing: 10,
                            childAspectRatio: 0.78,
                          ),
                      itemCount: _albums.length,
                      itemBuilder: (context, index) => _AlbumTile(
                        entry: _albums[index],
                        pinned: _pinned.contains(_albums[index].id),
                        onTogglePin: () => _togglePin(_albums[index].id),
                      ),
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

class _AlbumTile extends StatelessWidget {
  final _AlbumEntry entry;
  final bool pinned;
  final VoidCallback onTogglePin;

  const _AlbumTile({
    required this.entry,
    required this.pinned,
    required this.onTogglePin,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => Navigator.push(
        context,
        FastRoute(builder: (_) => _AlbumScreen(entry: entry)),
      ),
      onLongPress: onTogglePin,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          AspectRatio(
            aspectRatio: 1,
            child: Material(
              shape: sq(18),
              clipBehavior: Clip.antiAlias,
              color: context.colors.secondaryCol,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  if (entry.cover != null)
                    Image.memory(
                      entry.cover!,
                      fit: BoxFit.cover,
                      gaplessPlayback: true,
                    )
                  else
                    Icon(
                      Icons.photo_rounded,
                      color: context.colors.thirdFont,
                      size: 32,
                    ),
                  if (pinned)
                    Positioned(
                      top: 6,
                      right: 6,
                      child: Container(
                        padding: const EdgeInsets.all(4),
                        decoration: BoxDecoration(
                          color: Colors.black54,
                          borderRadius: BorderRadius.circular(6),
                        ),
                        child: const Icon(
                          Icons.push_pin_rounded,
                          color: Colors.white,
                          size: 14,
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 6),
          Text(
            entry.name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              fontFamily: 'Rubik',
              fontSize: 14,
              fontWeight: FontWeight.w500,
              color: context.colors.primaryFont,
            ),
          ),
          Text(
            context.l10n.elementsCount(entry.count),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              fontFamily: 'Rubik',
              fontSize: 12,
              color: context.colors.secondaryFont,
            ),
          ),
        ],
      ),
    );
  }
}

class _AlbumScreen extends StatefulWidget {
  final _AlbumEntry entry;

  const _AlbumScreen({required this.entry});

  @override
  State<_AlbumScreen> createState() => _AlbumScreenState();
}

class _AlbumScreenState extends State<_AlbumScreen> {
  final List<AssetEntity> _assets = [];
  bool _loading = true;
  int _currentPage = 0;
  static const int _pageSize = 80;
  bool _hasMore = true;
  final ScrollController _scrollController = ScrollController();
  DateTime? _earliest;
  DateTime? _latest;

  @override
  void initState() {
    super.initState();
    _loadAssets();
    _loadDateRange();
    _scrollController.addListener(_onScroll);
  }

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  Future<void> _loadDateRange() async {
    final album = widget.entry.album;
    final first = await album.getAssetListRange(start: 0, end: 1);
    final last = await album.getAssetListRange(
      start: widget.entry.count - 1,
      end: widget.entry.count,
    );
    if (!mounted) return;
    setState(() {
      if (first.isNotEmpty) _latest = first.first.createDateTime;
      if (last.isNotEmpty) _earliest = last.first.createDateTime;
    });
  }

  Future<void> _loadAssets() async {
    final newAssets = await widget.entry.album.getAssetListPaged(
      page: _currentPage,
      size: _pageSize,
    );
    if (!mounted) return;
    setState(() {
      _assets.addAll(newAssets);
      _hasMore = newAssets.length == _pageSize;
      _loading = false;
    });
  }

  void _onScroll() {
    if (_scrollController.position.pixels >
            _scrollController.position.maxScrollExtent - 600 &&
        _hasMore &&
        !_loading) {
      _currentPage++;
      _loadAssets();
    }
  }

  String _formatDuration(int seconds) {
    final m = seconds ~/ 60;
    final s = seconds % 60;
    return '$m:${s.toString().padLeft(2, '0')}';
  }

  String _dateRangeLabel(String locale) {
    final format = DateFormat('d MMM y', locale);
    return '${format.format(_earliest!)} – ${format.format(_latest!)}'
        .toUpperCase();
  }

  List<(DateTime, List<int>)> _sections() {
    final sections = <(DateTime, List<int>)>[];
    DateTime? currentDay;
    for (var i = 0; i < _assets.length; i++) {
      final date = _assets[i].createDateTime;
      final day = DateTime(date.year, date.month, date.day);
      if (currentDay != day) {
        sections.add((day, []));
        currentDay = day;
      }
      sections.last.$2.add(i);
    }
    return sections;
  }

  Widget _buildHero(BuildContext context, String locale) {
    final cover = widget.entry.cover;

    Widget background;
    if (cover != null) {
      background = Image.memory(
        cover,
        fit: BoxFit.cover,
        gaplessPlayback: true,
      );
    } else {
      background = Container(color: context.colors.secondaryCol);
    }

    return GestureDetector(
      onTap: () => Navigator.of(
        context,
      ).push(FastRoute(builder: (_) => _SlideshowScreen(entry: widget.entry))),
      child: Stack(
        fit: StackFit.expand,
        children: [
          background,
          SafeArea(
            bottom: false,
            child: Column(
              children: [
                Align(
                  alignment: Alignment.centerLeft,
                  child: IconButton(
                    icon: const Icon(
                      Icons.arrow_back_rounded,
                      color: Colors.white,
                    ),
                    onPressed: () => Navigator.of(context).pop(),
                  ),
                ),
                const Spacer(),
                if (_earliest != null && _latest != null)
                  Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 10,
                      vertical: 3,
                    ),
                    decoration: BoxDecoration(
                      color: Colors.black.withValues(alpha: 0.45),
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Text(
                      _dateRangeLabel(locale),
                      style: const TextStyle(
                        fontFamily: 'Rubik',
                        color: Colors.white,
                        fontSize: 10,
                        fontWeight: FontWeight.w600,
                        letterSpacing: 0.5,
                      ),
                    ),
                  ),
                const SizedBox(height: 8),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 24),
                  child: Text(
                    widget.entry.name,
                    textAlign: TextAlign.center,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontFamily: 'Rubik',
                      color: Colors.white,
                      fontSize: 28,
                      fontWeight: FontWeight.w700,
                      shadows: [Shadow(blurRadius: 8, color: Colors.black45)],
                    ),
                  ),
                ),
                const SizedBox(height: 20),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildFilterChips(BuildContext context) {
    final labels = ['Notes', 'Screenshots', 'Documents'];
    return SizedBox(
      height: 44,
      child: ListView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        children: [
          Container(
            width: 44,
            height: 44,
            decoration: ShapeDecoration(
              color: context.colors.secondaryCol,
              shape: sq(14),
            ),
            child: Icon(
              Icons.tune_rounded,
              color: context.colors.primaryFont,
              size: 20,
            ),
          ),
          for (final label in labels)
            Padding(
              padding: const EdgeInsets.only(left: 8),
              child: Container(
                height: 44,
                padding: const EdgeInsets.symmetric(horizontal: 18),
                decoration: BoxDecoration(
                  color: context.colors.secondaryCol,
                  borderRadius: BorderRadius.circular(22),
                ),
                child: Center(
                  child: Text(
                    label,
                    style: TextStyle(
                      fontFamily: 'Rubik',
                      fontSize: 14,
                      fontWeight: FontWeight.w500,
                      color: context.colors.primaryFont,
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildItem(int index) {
    final asset = _assets[index];
    return Stack(
      fit: StackFit.expand,
      children: [
        _AssetThumbnail(asset: asset),
        if (asset.type == AssetType.video)
          Positioned(
            bottom: 4,
            right: 4,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
              decoration: BoxDecoration(
                color: Colors.black54,
                borderRadius: BorderRadius.circular(4),
              ),
              child: Text(
                _formatDuration(asset.duration),
                style: const TextStyle(
                  color: Colors.white,
                  fontSize: 10,
                  fontFamily: 'Rubik',
                ),
              ),
            ),
          ),
      ],
    );
  }

  String _scrubLabel(double fraction, String locale) {
    final sections = _sections();
    var totalRows = 0;
    for (final (_, indices) in sections) {
      totalRows += (indices.length / 4).ceil() + 1;
    }
    if (totalRows == 0) return '';
    final target = (fraction * totalRows).floor();
    var acc = 0;
    for (final (day, indices) in sections) {
      acc += (indices.length / 4).ceil() + 1;
      if (target < acc) return DateFormat('MMM y', locale).format(day);
    }
    return DateFormat('MMM y', locale).format(sections.last.$1);
  }

  @override
  Widget build(BuildContext context) {
    final locale = Localizations.localeOf(context).toString();
    final sections = _sections();
    final heroHeight = MediaQuery.sizeOf(context).height * 0.45;
    final dayFormat = DateFormat('EEE, d MMM', locale);

    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      body: Stack(
        children: [
          CustomScrollView(
            controller: _scrollController,
            slivers: [
              SliverAppBar(
                expandedHeight: heroHeight,
                pinned: false,
                automaticallyImplyLeading: false,
                backgroundColor: context.colors.primaryCol,
                flexibleSpace: _buildHero(context, locale),
              ),
              SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                  child: _buildFilterChips(context),
                ),
              ),
              if (_loading && sections.isEmpty)
                const SliverFillRemaining(child: Center(child: Spinner()))
              else ...[
                for (final (day, indices) in sections)
                  SliverToBoxAdapter(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Padding(
                          padding: const EdgeInsets.fromLTRB(14, 18, 14, 8),
                          child: Text(
                            dayFormat.format(day),
                            style: TextStyle(
                              fontFamily: 'Rubik',
                              fontSize: 14,
                              fontWeight: FontWeight.w500,
                              color: context.colors.secondaryFont,
                            ),
                          ),
                        ),
                        GridView.count(
                          crossAxisCount: 4,
                          mainAxisSpacing: 2,
                          crossAxisSpacing: 2,
                          padding: const EdgeInsets.symmetric(horizontal: 2),
                          shrinkWrap: true,
                          physics: const NeverScrollableScrollPhysics(),
                          children: [
                            for (final index in indices) _buildItem(index),
                          ],
                        ),
                      ],
                    ),
                  ),
                const SliverToBoxAdapter(child: SizedBox(height: 32)),
              ],
            ],
          ),
          if (sections.isNotEmpty)
            Positioned(
              top: 0,
              bottom: 0,
              right: 0,
              width: 28,
              child: _DateScrubber(
                controller: _scrollController,
                labelBuilder: (fraction) => _scrubLabel(fraction, locale),
              ),
            ),
        ],
      ),
    );
  }
}

class _DateScrubber extends StatefulWidget {
  final ScrollController controller;
  final String Function(double fraction) labelBuilder;

  const _DateScrubber({required this.controller, required this.labelBuilder});

  @override
  State<_DateScrubber> createState() => _DateScrubberState();
}

class _DateScrubberState extends State<_DateScrubber> {
  static const double _handleHeight = 48;

  bool _dragging = false;
  double _dragFraction = 0;

  double _fraction() {
    if (_dragging) return _dragFraction;
    if (!widget.controller.hasClients) return 0;
    final position = widget.controller.position;
    if (position.maxScrollExtent <= 0) return 0;
    return (position.pixels / position.maxScrollExtent).clamp(0.0, 1.0);
  }

  void _onDrag(double dy, double height) {
    final usable = height - _handleHeight;
    if (usable <= 0) return;
    final fraction = ((dy - _handleHeight / 2) / usable).clamp(0.0, 1.0);
    setState(() {
      _dragging = true;
      _dragFraction = fraction;
    });
    if (widget.controller.hasClients) {
      final position = widget.controller.position;
      widget.controller.jumpTo(fraction * position.maxScrollExtent);
    }
  }

  void _endDrag() => setState(() => _dragging = false);

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final height = constraints.maxHeight;
        return AnimatedBuilder(
          animation: widget.controller,
          builder: (context, _) {
            final fraction = _fraction();
            final handleTop = fraction * (height - _handleHeight);
            return GestureDetector(
              behavior: HitTestBehavior.opaque,
              onVerticalDragStart: (d) =>
                  _onDrag(d.localPosition.dy, height),
              onVerticalDragUpdate: (d) =>
                  _onDrag(d.localPosition.dy, height),
              onVerticalDragEnd: (_) => _endDrag(),
              onVerticalDragCancel: _endDrag,
              child: Stack(
                clipBehavior: Clip.none,
                children: [
                  Positioned(
                    right: 6,
                    top: handleTop,
                    child: AnimatedContainer(
                      duration: const Duration(milliseconds: 120),
                      width: _dragging ? 6 : 4,
                      height: _handleHeight,
                      decoration: BoxDecoration(
                        color: context.colors.thirdFont.withValues(
                          alpha: _dragging ? 0.9 : 0.4,
                        ),
                        borderRadius: BorderRadius.circular(3),
                      ),
                    ),
                  ),
                  if (_dragging)
                    Positioned(
                      right: 20,
                      top: (handleTop + 4).clamp(
                        0.0,
                        height - 40,
                      ),
                      child: IgnorePointer(
                        child: Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 14,
                            vertical: 8,
                          ),
                          decoration: BoxDecoration(
                            color: context.colors.primaryFont,
                            borderRadius: BorderRadius.circular(20),
                          ),
                          child: Text(
                            widget.labelBuilder(fraction),
                            maxLines: 1,
                            softWrap: false,
                            style: TextStyle(
                              color: context.colors.primaryCol,
                              fontFamily: 'Rubik',
                              fontSize: 13,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            );
          },
        );
      },
    );
  }
}

class _ThumbCache {
  static const int _maxEntries = 600;
  static final Map<String, Uint8List> _map = {};

  static Uint8List? get(String key) => _map[key];

  static void put(String key, Uint8List value) {
    if (_map.length >= _maxEntries) {
      _map.remove(_map.keys.first);
    }
    _map[key] = value;
  }
}

class _AssetThumbnail extends StatefulWidget {
  final AssetEntity asset;

  const _AssetThumbnail({required this.asset});

  @override
  State<_AssetThumbnail> createState() => _AssetThumbnailState();
}

class _AssetThumbnailState extends State<_AssetThumbnail> {
  Uint8List? _thumb;
  bool _isDisposed = false;

  String get _cacheKey => '${widget.asset.id}_260';

  @override
  void initState() {
    super.initState();
    _thumb = _ThumbCache.get(_cacheKey);
    if (_thumb == null) _load();
  }

  @override
  void didUpdateWidget(_AssetThumbnail oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.asset.id != widget.asset.id) {
      _thumb = _ThumbCache.get(_cacheKey);
      if (_thumb == null) {
        _load();
      } else {
        setState(() {});
      }
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }

  Future<void> _load() async {
    final bytes = await widget.asset.thumbnailDataWithSize(
      const ThumbnailSize(260, 260),
    );
    if (bytes != null) _ThumbCache.put(_cacheKey, bytes);
    if (!_isDisposed && mounted) {
      setState(() => _thumb = bytes);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_thumb == null) {
      return Container(color: context.colors.secondaryCol);
    }
    return Image.memory(
      _thumb!,
      fit: BoxFit.cover,
      cacheWidth: 260,
      gaplessPlayback: true,
    );
  }
}

class _SlideshowScreen extends StatefulWidget {
  final _AlbumEntry entry;

  const _SlideshowScreen({required this.entry});

  @override
  State<_SlideshowScreen> createState() => _SlideshowScreenState();
}

class _SlideshowScreenState extends State<_SlideshowScreen> {
  List<AssetEntity> _assets = [];
  int _index = 0;
  Timer? _timer;
  Timer? _hideTimer;
  int _seconds = 5;
  bool _shuffle = false;
  bool _blurred = true;
  bool _loading = true;
  bool _controlsVisible = true;
  final _random = Random();

  int get _count => _assets.length;

  @override
  void initState() {
    super.initState();
    SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
    _scheduleHide();
    _load();
  }

  @override
  void dispose() {
    _timer?.cancel();
    _hideTimer?.cancel();
    SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
    super.dispose();
  }

  void _scheduleHide() {
    _hideTimer?.cancel();
    _hideTimer = Timer(const Duration(seconds: 2), () {
      if (mounted) setState(() => _controlsVisible = false);
    });
  }

  void _toggleControls() {
    setState(() => _controlsVisible = !_controlsVisible);
    if (_controlsVisible) _scheduleHide();
  }

  Future<void> _load() async {
    final all = await widget.entry.album.getAssetListRange(
      start: 0,
      end: widget.entry.count,
    );
    _assets = all.where((a) => a.type == AssetType.image).toList();
    if (!mounted) return;
    setState(() => _loading = false);
    _restartTimer();
  }

  void _restartTimer() {
    _timer?.cancel();
    if (_count < 2) return;
    _timer = Timer.periodic(Duration(seconds: _seconds), (_) => _next());
  }

  void _next() {
    setState(() {
      _index = _shuffle ? _random.nextInt(_count) : (_index + 1) % _count;
    });
  }

  Widget _currentImage(BoxFit fit) {
    return _SlideImage(asset: _assets[_index], fit: fit);
  }

  void _showSettings() {
    showModalBottomSheet(
      context: context,
      backgroundColor: const Color(0xFF1C1C1E),
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(16)),
      ),
      builder: (_) => StatefulBuilder(
        builder: (context, setSheet) {
          Widget optionChip(String label, bool selected, VoidCallback onTap) {
            return GestureDetector(
              onTap: () {
                onTap();
                setState(() {});
                setSheet(() {});
              },
              child: Container(
                padding: const EdgeInsets.symmetric(
                  horizontal: 18,
                  vertical: 12,
                ),
                decoration: BoxDecoration(
                  color: selected ? Colors.white : const Color(0xFF2C2C2E),
                  borderRadius: BorderRadius.circular(24),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      label,
                      style: TextStyle(
                        fontFamily: 'Rubik',
                        fontSize: 14,
                        fontWeight: FontWeight.w500,
                        color: selected ? Colors.black : Colors.white70,
                      ),
                    ),
                    if (selected) ...[
                      const SizedBox(width: 6),
                      const Icon(Icons.check, color: Colors.black, size: 16),
                    ],
                  ],
                ),
              ),
            );
          }

          Widget sectionLabel(String text) => Padding(
            padding: const EdgeInsets.only(top: 24, bottom: 12),
            child: Text(
              text,
              style: const TextStyle(
                fontFamily: 'Rubik',
                fontSize: 16,
                fontWeight: FontWeight.w600,
                color: Colors.white,
              ),
            ),
          );

          return Padding(
            padding: const EdgeInsets.fromLTRB(20, 20, 20, 32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    const Text(
                      'Slideshow settings',
                      style: TextStyle(
                        fontFamily: 'Rubik',
                        fontSize: 20,
                        fontWeight: FontWeight.w600,
                        color: Colors.white,
                      ),
                    ),
                    GestureDetector(
                      onTap: () => Navigator.of(context).pop(),
                      child: const Icon(
                        Icons.close,
                        color: Colors.white,
                        size: 22,
                      ),
                    ),
                  ],
                ),
                sectionLabel('Time per photo'),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    for (final (label, secs) in [
                      ('2s', 2),
                      ('5s', 5),
                      ('10s', 10),
                      ('15s', 15),
                      ('30s', 30),
                      ('1m', 60),
                      ('5m', 300),
                      ('10m', 600),
                    ])
                      optionChip(label, _seconds == secs, () {
                        _seconds = secs;
                        _restartTimer();
                      }),
                  ],
                ),
                sectionLabel('Photo order'),
                Wrap(
                  spacing: 8,
                  children: [
                    optionChip('In order', !_shuffle, () => _shuffle = false),
                    optionChip('Shuffle', _shuffle, () => _shuffle = true),
                  ],
                ),
                sectionLabel('Background'),
                Wrap(
                  spacing: 8,
                  children: [
                    optionChip('Blurred', _blurred, () => _blurred = true),
                    optionChip('Black', !_blurred, () => _blurred = false),
                  ],
                ),
              ],
            ),
          );
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      body: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: _toggleControls,
        child: Stack(
          fit: StackFit.expand,
          children: [
            if (_loading)
              const Center(child: Spinner())
            else if (_count > 0) ...[
              if (_blurred)
                ImageFiltered(
                  imageFilter: ImageFilter.blur(sigmaX: 40, sigmaY: 40),
                  child: _currentImage(BoxFit.cover),
                ),
              Container(
                color: _blurred
                    ? Colors.black.withValues(alpha: 0.35)
                    : Colors.black,
              ),
              Center(child: _currentImage(BoxFit.contain)),
            ],
            Positioned(
              top: 16,
              left: 20,
              right: 0,
              child: AnimatedOpacity(
                opacity: _controlsVisible ? 1 : 0,
                duration: const Duration(milliseconds: 200),
                child: IgnorePointer(
                  ignoring: !_controlsVisible,
                  child: SafeArea(
                    bottom: false,
                    child: Row(
                      children: [
                        IconButton(
                          icon: const Icon(Icons.close, color: Colors.white),
                          onPressed: () => Navigator.of(context).pop(),
                        ),
                        Expanded(
                          child: Text(
                            widget.entry.name,
                            textAlign: TextAlign.left,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(
                              fontFamily: 'Rubik',
                              color: Colors.white,
                              fontSize: 20,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ),
                        IconButton(
                          icon: const Icon(
                            Icons.settings_outlined,
                            color: Colors.white,
                          ),
                          onPressed: _showSettings,
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

class _SlideImage extends StatefulWidget {
  final AssetEntity asset;
  final BoxFit fit;

  const _SlideImage({required this.asset, required this.fit});

  @override
  State<_SlideImage> createState() => _SlideImageState();
}

class _SlideImageState extends State<_SlideImage> {
  Uint8List? _data;
  bool _isDisposed = false;

  String get _cacheKey => '${widget.asset.id}_1440';

  @override
  void initState() {
    super.initState();
    _data = _ThumbCache.get(_cacheKey);
    if (_data == null) _load();
  }

  @override
  void didUpdateWidget(_SlideImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.asset.id != widget.asset.id) {
      _data = _ThumbCache.get(_cacheKey);
      if (_data == null) {
        _load();
      } else {
        setState(() {});
      }
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }

  Future<void> _load() async {
    final bytes = await widget.asset.thumbnailDataWithSize(
      const ThumbnailSize(1440, 1440),
    );
    if (bytes != null) _ThumbCache.put(_cacheKey, bytes);
    if (!_isDisposed && mounted) {
      setState(() => _data = bytes);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_data == null) return const SizedBox.shrink();
    return Image.memory(_data!, fit: widget.fit, gaplessPlayback: true);
  }
}
