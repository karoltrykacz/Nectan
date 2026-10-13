import 'dart:io';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:nectan_mobile/elements.dart';
import 'package:photo_manager/photo_manager.dart';
import 'package:video_player/video_player.dart';

class MediaPickerScreen extends StatefulWidget {
  final RequestType mediaType;
  final void Function(List<String> paths) onDone;

  const MediaPickerScreen({
    super.key,
    this.mediaType = RequestType.image,
    required this.onDone,
  });

  @override
  State<MediaPickerScreen> createState() => _MediaPickerScreenState();
}

class _MediaPickerScreenState extends State<MediaPickerScreen> {
  List<AssetPathEntity> _albums = [];
  AssetPathEntity? _currentAlbum;
  List<AssetEntity> _assets = [];
  final Set<AssetEntity> _selected = {};
  bool _loading = true;
  int _currentPage = 0;
  static const int _pageSize = 80;
  bool _hasMore = true;
  final ScrollController _scrollController = ScrollController();

  int _crossAxisCount = 3;
  double _baseScale = 3.0;

  int _totalAssetsCount = 0;
  bool _isBulkSelecting = false;

  bool get _isAudio => widget.mediaType == RequestType.audio;

  // FIX: filter with date desc sort applied at album init level
  FilterOptionGroup get _dateDescFilter => FilterOptionGroup(
    orders: [const OrderOption(type: OrderOptionType.createDate, asc: false)],
  );

  Future<void> _loadAssets({bool reset = false}) async {
    if (_currentAlbum == null) return;
    if (reset) {
      final count = await _currentAlbum!.assetCountAsync;
      setState(() {
        _assets = [];
        _currentPage = 0;
        _hasMore = true;
        _loading = true;
        _totalAssetsCount = count;
      });
    }
    final newAssets = await _currentAlbum!.getAssetListPaged(
      page: _currentPage,
      size: _pageSize,
    );
    // await _currentAlbum!.getAssetListPaged(
    // page: _currentPage,
    // size: _pageSize,
    //
    // filterOptionGroup: _dateDescFilter,
    // );
    setState(() {
      _assets.addAll(newAssets);
      _hasMore = newAssets.length == _pageSize;
      _loading = false;
    });
  }

  @override
  void initState() {
    super.initState();
    _init();
    _scrollController.addListener(_onScroll);
  }

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  Future<void> _init() async {
    final fetchedAlbums = await PhotoManager.getAssetPathList(
      type: widget.mediaType,
      onlyAll: false,
      filterOption: _dateDescFilter, // FIX: sort at album level too
    );

    if (fetchedAlbums.isEmpty) {
      setState(() => _loading = false);
      return;
    }

    fetchedAlbums.sort(
      (a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()),
    );

    AssetPathEntity? primaryAlbum;
    const folderPriorities = ['all', 'camera', 'dcim', 'recents', 'recent'];

    for (final priorityName in folderPriorities) {
      try {
        primaryAlbum = fetchedAlbums.firstWhere(
          (album) => album.name.toLowerCase() == priorityName,
        );
        break;
      } catch (_) {}
    }

    if (primaryAlbum == null) {
      try {
        primaryAlbum = fetchedAlbums.firstWhere((album) => album.isAll);
      } catch (_) {}
    }

    primaryAlbum ??= fetchedAlbums.first;

    // _currentAlbum

    setState(() {
      _albums = fetchedAlbums;
      _currentAlbum = primaryAlbum;
    });
    // _selectAll();

    await _loadAssets(reset: true);
  }

  Future<void> _selectAll() async {
    if (_currentAlbum == null) return;

    if (_selected.isNotEmpty) {
      setState(() => _selected.clear());
    } else {
      setState(() => _isBulkSelecting = true);
      try {
        final allAssets = await _currentAlbum!.getAssetListRange(
          start: 0,
          end: _totalAssetsCount,
          // filterOption: _dateDescFilter, // FIX: keep same sort order
        );
        setState(() {
          _selected.addAll(allAssets);
          _assets = allAssets;
          _hasMore = false;
        });
      } catch (e) {
        debugPrint("🟥 Bulk selection failed: $e");
      } finally {
        setState(() => _isBulkSelecting = false);
      }
    }
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

  void _toggleSelect(AssetEntity asset) {
    setState(() {
      if (_selected.contains(asset)) {
        _selected.remove(asset);
      } else {
        _selected.add(asset);
      }
    });
  }

  void _openPreview(AssetEntity asset) {
    Navigator.push(
      context,
      MaterialPageRoute(builder: (_) => _FullScreenPreview(asset: asset)),
    );
  }

  Future<void> _onDone() async {
    Navigator.pop(context, _selected.toList());
  }

  void _showAlbumPicker() {
    final allAlbumIndex = _albums.indexWhere((album) => album.isAll);
    List<AssetPathEntity> displayAlbums = List.from(_albums);
    AssetPathEntity? allAlbum;

    if (allAlbumIndex != -1) {
      allAlbum = displayAlbums.removeAt(allAlbumIndex);
    }

    showModalBottomSheet(
      context: context,
      backgroundColor: const Color(0xFF1C1C1E),
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(16)),
      ),
      builder: (_) {
        final itemCount = displayAlbums.length + 1;
        return ListView.builder(
          shrinkWrap: true,
          padding: const EdgeInsets.only(top: 16.0, bottom: 16.0),
          itemCount: itemCount,
          itemBuilder: (_, i) {
            AssetPathEntity? targetAlbum;
            String displayName = '';
            bool isCurrent = false;

            if (i == 0) {
              targetAlbum =
                  allAlbum ?? (_albums.isNotEmpty ? _albums.first : null);
              displayName = 'Show All';
              isCurrent =
                  _currentAlbum?.id == targetAlbum?.id ||
                  _currentAlbum?.isAll == true;
            } else {
              targetAlbum = displayAlbums[i - 1];
              displayName = targetAlbum.name;
              isCurrent =
                  _currentAlbum?.id == targetAlbum.id &&
                  _currentAlbum?.isAll != true;
            }

            if (targetAlbum == null) return const SizedBox.shrink();

            return Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20.0),
              child: ListTile(
                dense: true,
                visualDensity: const VisualDensity(horizontal: 0, vertical: -4),
                contentPadding: const EdgeInsets.symmetric(horizontal: 20.0),
                title: Text(
                  displayName,
                  style: TextStyle(
                    fontSize: 15,
                    color: isCurrent ? const Color(0xffffa200) : Colors.white,
                    fontWeight: isCurrent ? FontWeight.w600 : FontWeight.normal,
                  ),
                ),
                trailing: isCurrent
                    ? const Icon(
                        Icons.check_box_rounded,
                        size: 20,
                        color: Color(0xffffa200),
                      )
                    : null,
                onTap: () {
                  Navigator.pop(context);
                  setState(() => _currentAlbum = targetAlbum);
                  _loadAssets(reset: true);
                },
              ),
            );
          },
        );
      },
    );
  }

  // Widget _buildAudioTile(AssetEntity asset) {
  //   final isSelected = _selected.contains(asset);
  //   final selectionIndex = _selected.toList().indexOf(asset);
  //   return ListTile(
  //     contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
  //     leading: Stack(
  //       alignment: Alignment.center,
  //       children: [
  //         Container(
  //           width: 48,
  //           height: 48,
  //           decoration: BoxDecoration(
  //             color: const Color(0xFF2C2C2E),
  //             borderRadius: BorderRadius.circular(8),
  //           ),
  //           child: const Icon(
  //             Icons.music_note_rounded,
  //             color: Colors.white38,
  //             size: 24,
  //           ),
  //         ),
  //         if (isSelected)
  //           Container(
  //             width: 48,
  //             height: 48,
  //             decoration: BoxDecoration(
  //               color: Colors.black54,
  //               borderRadius: BorderRadius.circular(8),
  //             ),
  //             child: Center(
  //               child: Text(
  //                 '${selectionIndex + 1}',
  //                 style: const TextStyle(
  //                   color: Colors.white,
  //                   fontSize: 13,
  //                   fontWeight: FontWeight.bold,
  //                 ),
  //               ),
  //             ),
  //           ),
  //       ],
  //     ),
  //     title: Text(
  //       asset.title ?? 'Unknown',
  //       style: const TextStyle(color: Colors.white, fontSize: 14),
  //       maxLines: 1,
  //       overflow: TextOverflow.ellipsis,
  //     ),
  //     subtitle: Text(
  //       _formatDuration(asset.duration),
  //       style: const TextStyle(color: Colors.white54, fontSize: 12),
  //     ),
  //     trailing: isSelected
  //         ? const Icon(
  //             Icons.check_circle_rounded,
  //             color: Color(0xffffa200),
  //             size: 22,
  //           )
  //         : const Icon(Icons.circle_outlined, color: Colors.white24, size: 22),
  //     onTap: () => _toggleSelect(asset),
  //     onLongPress: () => _openPreview(asset),
  //   );
  // }
  Widget _buildAudioTile(AssetEntity asset) {
    final isSelected = _selected.contains(asset);
    final selectionIndex = _selected.toList().indexOf(asset);
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
      leading: GestureDetector(
        onTap: () => _openPreview(asset),
        child: Stack(
          alignment: Alignment.center,
          children: [
            Container(
              width: 48,
              height: 48,
              decoration: BoxDecoration(
                color: const Color(0xFF2C2C2E),
                borderRadius: BorderRadius.circular(8),
              ),
              child: Icon(
                isSelected
                    ? Icons.music_note_rounded
                    : Icons.play_arrow_rounded,
                color: Colors.white,
                size: 24,
              ),
            ),
            if (isSelected)
              Container(
                width: 48,
                height: 48,
                decoration: BoxDecoration(
                  color: Colors.black54,
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Center(
                  child: Text(
                    '${selectionIndex + 1}',
                    style: const TextStyle(
                      color: Colors.white,
                      fontSize: 13,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
      title: Text(
        asset.title ?? 'Unknown',
        style: const TextStyle(color: Colors.white, fontSize: 14),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      subtitle: Text(
        _formatDuration(asset.duration),
        style: const TextStyle(color: Colors.white54, fontSize: 12),
      ),
      trailing: isSelected
          ? const Icon(
              Icons.check_circle_outline_rounded,
              color: Color(0xffffa200),
              size: 22,
            )
          : const Icon(Icons.circle_outlined, color: Colors.white24, size: 22),
      onTap: () => _toggleSelect(asset),
      onLongPress: () => _openPreview(asset),
    );
  }

  @override
  Widget build(BuildContext context) {
    final String appBarTitle = _currentAlbum?.isAll == true
        ? 'Show All'
        : (_currentAlbum?.name ?? '');

    final int targetDimension = (_crossAxisCount >= 4) ? 160 : 260;

    return Scaffold(
      backgroundColor: const Color(0xff1a1a1a),
      appBar: AppBar(
        backgroundColor: const Color(0xFF1C1C1E),
        leading: IconButton(
          icon: const Icon(Icons.close, color: Colors.white),
          onPressed: () => Navigator.pop(context),
        ),
        title: GestureDetector(
          onTap: _showAlbumPicker,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                appBarTitle,
                style: const TextStyle(
                  color: Colors.white,
                  fontSize: 16,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const SizedBox(width: 4),
              const Icon(Icons.keyboard_arrow_down, color: Colors.white),
            ],
          ),
        ),
        actions: [
          _isBulkSelecting
              ? const Padding(
                  padding: EdgeInsets.symmetric(horizontal: 24.0),
                  child: SizedBox(width: 16, height: 16, child: Spinner()),
                )
              : TextButton(
                  onPressed: _selectAll,
                  child: Text(
                    _selected.isNotEmpty
                        ? 'Deselect All (${_selected.length})'
                        : 'Select All ($_totalAssetsCount)',
                    style: const TextStyle(color: Color(0xffffa200)),
                  ),
                ),
        ],
      ),
      body: _loading && _assets.isEmpty
          ? const Center(child: CircularProgressIndicator())
          : _isAudio
          ? ListView.separated(
              controller: _scrollController,
              itemCount: _assets.length,
              separatorBuilder: (_, _) =>
                  const Divider(color: Colors.white10, height: 1),
              itemBuilder: (_, i) => _buildAudioTile(_assets[i]),
            )
          : GestureDetector(
              onScaleStart: (details) {
                _baseScale = _crossAxisCount.toDouble();
              },
              onScaleUpdate: (details) {
                if (details.pointerCount < 2) return;
                final double targetScale = _baseScale / details.scale;
                final int newCount = targetScale.round().clamp(2, 5);
                if (newCount != _crossAxisCount) {
                  setState(() => _crossAxisCount = newCount);
                }
              },
              child: GridView.builder(
                controller: _scrollController,
                scrollCacheExtent: ScrollCacheExtent.pixels(1000),
                gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                  crossAxisCount: _crossAxisCount,
                  mainAxisSpacing: 2,
                  crossAxisSpacing: 2,
                  childAspectRatio: 1.0, // FIX: square tiles, no stretch
                ),
                itemCount: _assets.length,
                itemBuilder: (_, i) {
                  final asset = _assets[i];
                  final isSelected = _selected.contains(asset);
                  final selectionIndex = _selected.isEmpty
                      ? -1
                      : _selected.toList().indexOf(asset);

                  return GestureDetector(
                    onTap: () => _toggleSelect(asset),
                    onLongPress: () => _openPreview(asset),
                    child: Stack(
                      fit: StackFit.expand,
                      children: [
                        _AssetThumbnail(
                          asset: asset,
                          dimension: targetDimension,
                        ),
                        if (_selected.isNotEmpty && !isSelected)
                          Container(color: Colors.black38),
                        Positioned(
                          top: 6,
                          right: 6,
                          child: AnimatedContainer(
                            duration: const Duration(milliseconds: 150),
                            width: 24,
                            height: 24,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: isSelected
                                  ? Colors.white
                                  : Colors.transparent,
                            ),
                            child: isSelected
                                ? Center(
                                    child: Text(
                                      '${selectionIndex + 1}',
                                      style: const TextStyle(
                                        color: Colors.black,
                                        fontSize: 11,
                                        fontWeight: FontWeight.bold,
                                      ),
                                    ),
                                  )
                                : null,
                          ),
                        ),
                        if (asset.type == AssetType.video)
                          Positioned(
                            bottom: 4,
                            right: 4,
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 4,
                                vertical: 2,
                              ),
                              decoration: BoxDecoration(
                                color: Colors.black54,
                                borderRadius: BorderRadius.circular(4),
                              ),
                              child: Text(
                                _formatDuration(asset.duration),
                                style: const TextStyle(
                                  color: Colors.white,
                                  fontSize: 10,
                                ),
                              ),
                            ),
                          ),
                      ],
                    ),
                  );
                },
              ),
            ),
      floatingActionButton: _selected.isNotEmpty
          ? FloatingActionButton(
              onPressed: _onDone,
              backgroundColor: Colors.white,
              foregroundColor: Colors.black,
              child: Transform.rotate(
                angle: -1.5708,
                child: const Icon(Icons.send_rounded),
              ),
            )
          : null,
    );
  }

  String _formatDuration(int seconds) {
    final m = seconds ~/ 60;
    final s = seconds % 60;
    return '$m:${s.toString().padLeft(2, '0')}';
  }
}

// ── Thumbnail ─────────────────────────────────────────────────────────────────
class _AssetThumbnail extends StatefulWidget {
  final AssetEntity asset;
  final int dimension;

  const _AssetThumbnail({required this.asset, required this.dimension});

  @override
  State<_AssetThumbnail> createState() => _AssetThumbnailState();
}

class _AssetThumbnailState extends State<_AssetThumbnail> {
  Uint8List? _thumb;
  bool _isDisposed = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(_AssetThumbnail oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.asset.id != widget.asset.id ||
        oldWidget.dimension != widget.dimension) {
      _load();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }

  Future<void> _load() async {
    if (widget.asset.type == AssetType.audio) return;
    final bytes = await widget.asset.thumbnailDataWithSize(
      ThumbnailSize(widget.dimension, widget.dimension),
    );
    if (!_isDisposed && mounted) {
      setState(() => _thumb = bytes);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_thumb == null) {
      return Container(
        color: const Color(0xFF2C2C2E),
        child: widget.asset.type == AssetType.audio
            ? const Icon(
                Icons.music_note_rounded,
                color: Colors.white38,
                size: 32,
              )
            : null,
      );
    }
    return Image.memory(
      _thumb!,
      fit: BoxFit.cover,
      cacheWidth: widget.dimension,
      cacheHeight: widget.dimension,
      gaplessPlayback: true,
    );
  }
}

class _FullScreenPreview extends StatefulWidget {
  final AssetEntity asset;
  const _FullScreenPreview({required this.asset});
  @override
  State<_FullScreenPreview> createState() => _FullScreenPreviewState();
}

class _FullScreenPreviewState extends State<_FullScreenPreview> {
  VideoPlayerController? _controller;
  bool _loading = true;
  bool _showUI = true;

  bool get _isAudio => widget.asset.type == AssetType.audio;

  @override
  void initState() {
    super.initState();
    if (widget.asset.type == AssetType.video ||
        widget.asset.type == AssetType.audio) {
      _initPlayer();
    } else {
      _loading = false;
    }
  }

  Future<void> _initPlayer() async {
    final file = await widget.asset.originFile;
    if (file == null || !mounted) return;
    _controller = VideoPlayerController.file(file);
    await _controller!.initialize();
    if (mounted) {
      setState(() => _loading = false);
      _controller!.play();
      _controller!.addListener(() => setState(() {}));
    }
  }

  @override
  void dispose() {
    _controller?.dispose();
    super.dispose();
  }

  String _fmt(Duration d) {
    final m = d.inMinutes.remainder(60).toString().padLeft(2, '0');
    final s = d.inSeconds.remainder(60).toString().padLeft(2, '0');
    return '$m:$s';
  }

  void _toggleUI() => setState(() => _showUI = !_showUI);

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: _showUI
          ? AppBar(
              backgroundColor: Colors.black,
              leading: IconButton(
                icon: const Icon(Icons.arrow_back_rounded, color: Colors.white),
                onPressed: () => Navigator.pop(context),
              ),
              title: _isAudio
                  ? Text(
                      widget.asset.title ?? 'Audio',
                      style: const TextStyle(color: Colors.white, fontSize: 15),
                      overflow: TextOverflow.ellipsis,
                    )
                  : null,
            )
          : null,
      body: GestureDetector(
        onTap: _toggleUI,
        behavior: HitTestBehavior.opaque,
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Expanded(
              child: Center(
                child: _isAudio
                    ? _buildAudioBody()
                    : widget.asset.type == AssetType.video
                    ? _loading
                          ? const CircularProgressIndicator(color: Colors.white)
                          : AspectRatio(
                              aspectRatio: _controller!.value.aspectRatio,
                              child: VideoPlayer(_controller!),
                            )
                    : FutureBuilder<File?>(
                        future: widget.asset.originFile,
                        builder: (context, snapshot) {
                          if (!snapshot.hasData) {
                            return const CircularProgressIndicator(
                              color: Colors.white,
                            );
                          }
                          return InteractiveViewer(
                            maxScale: 4.0,
                            child: Image.file(
                              snapshot.data!,
                              fit: BoxFit.contain,
                            ),
                          );
                        },
                      ),
              ),
            ),
            AnimatedSwitcher(
              duration: const Duration(milliseconds: 200),
              child:
                  (!_isAudio &&
                      widget.asset.type == AssetType.video &&
                      !_loading &&
                      _controller != null &&
                      _showUI)
                  ? SafeArea(
                      top: false,
                      child: _VideoControls(
                        controller: _controller!,
                        fmt: _fmt,
                      ),
                    )
                  : const SizedBox.shrink(),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildAudioBody() {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 32),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 180,
            height: 180,
            decoration: BoxDecoration(
              color: const Color(0xFF2C2C2E),
              borderRadius: BorderRadius.circular(24),
            ),
            child: const Icon(
              Icons.music_note_rounded,
              color: Colors.white24,
              size: 80,
            ),
          ),
          const SizedBox(height: 32),
          Text(
            widget.asset.title ?? 'Unknown',
            style: const TextStyle(
              color: Colors.white,
              fontSize: 18,
              fontWeight: FontWeight.w600,
            ),
            textAlign: TextAlign.center,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
          ),
          const SizedBox(height: 32),
          if (_loading)
            const CircularProgressIndicator(color: Colors.white)
          else if (_controller != null)
            _AudioControls(controller: _controller!, fmt: _fmt),
        ],
      ),
    );
  }
}

// ── Audio controls ────────────────────────────────────────────────────────────
class _AudioControls extends StatelessWidget {
  final VideoPlayerController controller;
  final String Function(Duration) fmt;

  const _AudioControls({required this.controller, required this.fmt});

  @override
  Widget build(BuildContext context) {
    final pos = controller.value.position;
    final dur = controller.value.duration;
    final isPlaying = controller.value.isPlaying;

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        VideoProgressIndicator(
          controller,
          allowScrubbing: true,
          colors: const VideoProgressColors(
            playedColor: Color(0xffffa200),
            bufferedColor: Colors.white24,
            backgroundColor: Colors.white12,
          ),
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            Text(
              fmt(pos),
              style: const TextStyle(color: Colors.white54, fontSize: 12),
            ),
            const Spacer(),
            Text(
              fmt(dur),
              style: const TextStyle(color: Colors.white54, fontSize: 12),
            ),
          ],
        ),
        const SizedBox(height: 16),
        IconButton(
          iconSize: 56,
          icon: Icon(
            isPlaying ? Icons.pause_circle_filled : Icons.play_circle_filled,
            color: Colors.white,
          ),
          onPressed: () => isPlaying ? controller.pause() : controller.play(),
        ),
      ],
    );
  }
}

// ── Video controls ────────────────────────────────────────────────────────────
class _VideoControls extends StatelessWidget {
  final VideoPlayerController controller;
  final String Function(Duration) fmt;

  const _VideoControls({required this.controller, required this.fmt});

  @override
  Widget build(BuildContext context) {
    final pos = controller.value.position;
    final dur = controller.value.duration;
    final isPlaying = controller.value.isPlaying;

    return Container(
      color: Colors.black,
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          VideoProgressIndicator(
            controller,
            allowScrubbing: true,
            colors: const VideoProgressColors(
              playedColor: Colors.white,
              bufferedColor: Colors.white38,
              backgroundColor: Colors.white12,
            ),
          ),
          const SizedBox(height: 6),
          Row(
            children: [
              Text(
                fmt(pos),
                style: const TextStyle(color: Colors.white70, fontSize: 12),
              ),
              const Spacer(),
              IconButton(
                icon: Icon(
                  isPlaying ? Icons.pause_rounded : Icons.play_arrow_rounded,
                  color: Colors.white,
                  size: 32,
                ),
                onPressed: () =>
                    isPlaying ? controller.pause() : controller.play(),
              ),
              const Spacer(),
              Text(
                fmt(dur),
                style: const TextStyle(color: Colors.white70, fontSize: 12),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
