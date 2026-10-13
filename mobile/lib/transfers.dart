import 'dart:async';
import 'dart:io';
import 'dart:ui';
import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_foreground_task/flutter_foreground_task.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:marquee/marquee.dart';
import 'package:nectan_mobile/devices.dart';
import 'package:nectan_mobile/elements.dart';
import 'package:nectan_mobile/main.dart';
import 'package:nectan_mobile/src/rust/api/simple.dart';
import 'package:path_provider/path_provider.dart';
import 'package:permission_handler/permission_handler.dart';

Future<bool> showStoragePermissionDialog(
  BuildContext context,
  PermissionStatus initialStatus,
) async {
  final result = await Navigator.of(context, rootNavigator: true).push<bool>(
    FastRoute(
      builder: (_) => PermissionDialog(
        title: "Enable storage access",
        description:
            'Without storage permission, Nectan cannot read files and send them to remote devices.',
        permission: Permission.manageExternalStorage,
        intialStatus: initialStatus,
        action: () async {
          try {
            debugPrint("Storage permission request action");
            final permission = await Permission.manageExternalStorage.request();
            debugPrint("Storage permission: $permission");
            if (permission == PermissionStatus.granted) {
              if (context.mounted) {
                Navigator.of(context).pop(true);
              }
            } else {
              if (context.mounted) {
                Navigator.of(context).pop(false);
              }
            }
          } catch (e) {
            debugPrint("🟥 Notification permission error: $e");
            if (context.mounted) {
              Navigator.of(context).pop(false);
            }
          }
        },
        actionLabel: 'Enable notifications',
      ),
    ),
  );

  return result ?? false;
}

void showNotificationsPermissionDeniedModal(
  BuildContext context,
  PermissionStatus initialStatus,
) {
  Navigator.of(context, rootNavigator: true).push(
    FastRoute(
      builder: (_) => PermissionDialog(
        title: "Enable notifications",
        description:
            'Without notification permission, Nectan stops working in the background and transfers will pause.',
        permission: Permission.notification,
        intialStatus: initialStatus,
        action: () async {
          try {
            final permission = await Permission.notification.request();
            if (permission == PermissionStatus.granted) {
              await FlutterForegroundTask.startService(
                notificationTitle: 'Nectan',
                notificationText: 'Active in background.',
              );
              if (context.mounted) Navigator.of(context).pop();
            }
          } catch (e) {
            debugPrint("🟥 Notification permission error: $e");
          }
        },
        actionLabel: 'Enable notifications',
      ),
    ),
  );
}

class _ErrorInfo {
  final String message;
  final String tipTitle;
  final String tipText;
  const _ErrorInfo(this.message, this.tipTitle, this.tipText);
}

_ErrorInfo _detailedErrorInfo(SendItemErrorDto error) {
  switch (error) {
    case SendItemErrorDto.fileAlreadyExisted:
      return const _ErrorInfo(
        "File already existed.",
        "Error",
        "File with identical name was found in transfer directory.",
      );
    case SendItemErrorDto.fileIoError:
      return const _ErrorInfo(
        "We couldn't read this file.",
        "Why this happens",
        "The file may have been moved, renamed, or deleted after you selected it. Try sending again and don't move or modify files while the transfer is active.",
      );
    case SendItemErrorDto.fileOpenPermissionDenied:
      return const _ErrorInfo(
        "We don't have permission to open this file.",
        "What to do",
        "Check the app's storage/file permissions in your device settings, then try sending it again.",
      );
    case SendItemErrorDto.fileNotFound:
      return const _ErrorInfo(
        "This file could not be found.",
        "What to do",
        "It looks like the file no longer exists at its original location. Make sure it wasn't deleted or moved, then send it again.",
      );
    // this wont be visible anyways!
    case SendItemErrorDto.streamError:
      return const _ErrorInfo(
        "Something went wrong while sending the data.",
        "Why this happens",
        "This is usually caused by an unstable connection between devices. Retry transfer.",
      );
    case SendItemErrorDto.openFail:
    case SendItemErrorDto.terminated:
      return const _ErrorInfo(
        "The transfer was interrupted.",
        "What to do",
        "This can happen if the connection dropped mid-transfer. Check that both devices are still connected and try sending again.",
      );
  }
}

class _SendItemErrorScreen extends StatelessWidget {
  final TransferItemDto item;
  const _SendItemErrorScreen({required this.item});

  @override
  Widget build(BuildContext context) {
    assert(item.error != null, "Item error must not be null");
    final info = _detailedErrorInfo(item.error!);

    return Scaffold(
      backgroundColor: context.colors.primaryCol,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Spacer(),
              Text(
                'Failed to transfer item',
                style: TextStyle(
                  color: context.colors.primaryFont,
                  fontSize: 24,
                  fontWeight: FontWeight.w600,
                  fontFamily: 'Rubik',
                ),
              ),
              const SizedBox(height: 12),
              Text(
                item.name,
                style: TextStyle(
                  color: context.colors.primaryFont,
                  fontSize: 20,
                  fontWeight: FontWeight.w500,
                  fontFamily: 'Rubik',
                ),
              ),
              Text(
                item.path,
                style: TextStyle(
                  color: context.colors.primaryFont,
                  fontSize: 12,
                  fontWeight: FontWeight.w500,
                  fontFamily: 'Rubik',
                ),
              ),
              Text(
                sizeOf(item.size),
                style: TextStyle(
                  color: context.colors.primaryFont,
                  fontSize: 12,
                  fontWeight: FontWeight.w500,
                  fontFamily: 'Rubik',
                ),
              ),
              const SizedBox(height: 12),
              Text(
                info.message,
                style: TextStyle(
                  color: context.colors.secondaryFont,
                  fontSize: 15,
                  height: 1.5,
                ),
              ),
              const SizedBox(height: 72),
              buildTip(info.tipTitle, info.tipText),
              const Spacer(),
              Align(
                alignment: Alignment.center,
                child: PrimaryWideButton(
                  label: "Ok",
                  onPressed: () => Navigator.of(context).pop(true),
                ),
              ),
              const SizedBox(height: 72),
            ],
          ),
        ),
      ),
    );
  }
}

class TransfersPoolNotifier extends Notifier<List<TransferMetaDto>> {
  StreamSubscription<List<TransferMetaDto>>? _sub;
  void Function()? onPermissionDenied;

  @override
  List<TransferMetaDto> build() {
    ref.onDispose(() {
      _sub?.cancel();
    });
    return [];
  }

  void setStream(Stream<List<TransferMetaDto>> stream) async {
    debugPrint('🟦 Transfers stream sink was set.');
    _sub?.cancel();
    _listenTo(stream);
    state = await getAllTransfers();
  }

  void _listenTo(Stream<List<TransferMetaDto>> stream) {
    _sub = stream.listen((transfers) async {
      debugPrint('🟦 Transfers stream sink was updated.');
      if (transfers.isNotEmpty) {
        _updateForegroundService();
      } else {
        await FlutterForegroundTask.stopService();
      }
      state = transfers;
    });
  }

  void _updateForegroundService() async {
    var permission = await FlutterForegroundTask.checkNotificationPermission();
    if (permission != NotificationPermission.granted) {
      permission = await FlutterForegroundTask.requestNotificationPermission();
    }

    if (permission == NotificationPermission.granted) {
      await FlutterForegroundTask.startService(
        notificationTitle: 'Nectan',
        notificationText: 'Active in background.',
      );
    } else {
      onPermissionDenied?.call();
    }
  }

  void removeTransfer(String transferId) async {
    state = state.where((t) => t.id != transferId).toList();
    await deleteTransfer(transferId: transferId);
    debugPrint("🟦 Transfer deleted.");
  }
}

final transfersPoolProvider =
    NotifierProvider<TransfersPoolNotifier, List<TransferMetaDto>>(
      TransfersPoolNotifier.new,
    );

class TransfersScreen extends ConsumerWidget {
  const TransfersScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // rebuild only when transfer is added / removed
    final transfersLength = ref.watch(
      transfersPoolProvider.select((list) => list.length),
    );

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
                      'Transfers',
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

            // ── Content ───────────────────────────────────────────────────
            Expanded(
              child: transfersLength == 0
                  ? Center(
                      child: Text(
                        'No transfers',
                        style: TextStyle(
                          color: context.colors.secondaryFont,
                          fontFamily: "Rubik",
                        ),
                      ),
                    )
                  : ListView.builder(
                      itemCount: transfersLength,
                      padding: const EdgeInsets.symmetric(horizontal: 20),
                      itemBuilder: (context, index) {
                        final transferId = ref
                            .read(transfersPoolProvider)[index]
                            .id;
                        return TransferItemTile(id: transferId);
                      },
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

Color lighten(Color color, [double amount = 0.4]) {
  final hsl = HSLColor.fromColor(color);
  return hsl.withLightness((hsl.lightness + amount).clamp(0.0, 1.0)).toColor();
}

class TransferStatusBadge extends StatelessWidget {
  final TransferStateDto state;
  const TransferStatusBadge({super.key, required this.state});

  static (String, Color) _resolve(TransferStateDto state) => switch (state) {
    TransferStateDto.processing => ('Processing', Color(0xff4fc3f7)),
    TransferStateDto.finished => ('Finished', Color(0xff119405)),
    TransferStateDto.cancelled => ('Cancelled', Color(0xff888888)),
  };

  @override
  Widget build(BuildContext context) {
    final (label, color) = _resolve(state);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
      decoration: BoxDecoration(
        color: color,
        borderRadius: BorderRadius.circular(20),
        border: Border.all(color: lighten(color, 0.0), width: 0.5),
      ),
      child: Text(
        label,
        style: TextStyle(
          color: Colors.white,
          fontSize: 12,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}

Future<bool?> showConfirmDialog(
  BuildContext context, {
  required String title,
  required String message,
  String keepLabel = 'Keep',
  String removeLabel = 'Remove',
}) {
  return showGeneralDialog<bool>(
    context: context,
    barrierDismissible: true,
    barrierLabel: title,
    barrierColor: Colors.black.withAlpha(130),
    transitionDuration: Duration.zero,
    transitionBuilder: (_, _, _, child) => child,
    pageBuilder: (ctx, _, _) => Dialog(
      backgroundColor: context.colors.primaryCol,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
      child: Container(
        constraints: const BoxConstraints(maxWidth: 320),
        padding: const EdgeInsets.symmetric(horizontal: 28, vertical: 32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Text(
              title,
              textAlign: TextAlign.center,
              style: TextStyle(
                color: context.colors.primaryFont,
                fontSize: 20,
                fontWeight: FontWeight.w600,
                fontFamily: "Rubik",
              ),
            ),
            const SizedBox(height: 16),
            Text(
              message,
              textAlign: TextAlign.center,
              style: TextStyle(
                color: context.colors.secondaryFont,
                fontSize: 14,
                height: 1.4,
              ),
            ),
            const SizedBox(height: 32),
            Row(
              children: [
                Expanded(
                  child: FilledButton(
                    onPressed: () => Navigator.of(ctx).pop(false),
                    style: FilledButton.styleFrom(
                      backgroundColor: context.colors.primaryFont,
                      foregroundColor: context.colors.primaryCol,
                      padding: const EdgeInsets.symmetric(vertical: 16),
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12),
                      ),
                    ),
                    child: Text(
                      keepLabel,
                      style: const TextStyle(
                        fontFamily: "Rubik",
                        fontWeight: FontWeight.w600,
                        fontSize: 16,
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 14),
                Expanded(
                  child: FilledButton(
                    onPressed: () => Navigator.of(ctx).pop(true),
                    style: FilledButton.styleFrom(
                      backgroundColor: const Color(0xffDC3545),
                      foregroundColor: Colors.white,
                      padding: const EdgeInsets.symmetric(vertical: 16),
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12),
                      ),
                    ),
                    child: Text(
                      removeLabel,
                      style: const TextStyle(
                        fontFamily: "Rubik",
                        fontWeight: FontWeight.w600,
                        fontSize: 16,
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    ),
  );
}

class TransferItemTile extends ConsumerStatefulWidget {
  final String id;

  const TransferItemTile({super.key, required this.id});

  @override
  ConsumerState<TransferItemTile> createState() => _TransferItemTileState();
}

// todo fix speed not showing
String _speedLabel(BigInt speed) {
  return '$speed MB/s';
}

class _TransferItemTileState extends ConsumerState<TransferItemTile> {
  @override
  void initState() {
    super.initState();
  }

  @override
  void dispose() {
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final TransferMetaDto? transfer = ref.watch(
      transfersPoolProvider.select((list) {
        try {
          return list.firstWhere((t) => t.id == widget.id);
        } catch (_) {
          return null;
        }
      }),
    );

    if (transfer == null) return const SizedBox.shrink();

    double targetProgress;
    if (transfer.totalSize == BigInt.from(0)) {
      targetProgress = 0;
    } else {
      targetProgress = transfer.sentBytes / transfer.totalSize;
    }

    return Dismissible(
      key: Key(transfer.id),
      direction: DismissDirection.endToStart,
      confirmDismiss: (direction) async {
        if (transfer.status == TransferStateDto.finished) {
          return true;
        }

        final confirm = await showConfirmDialog(
          context,
          title: 'Cancel transfer',
          message: 'This transfer is not finished.\nProgress will be lost.',
          keepLabel: 'Keep',
          removeLabel: 'Remove',
        );

        if (confirm == true) {
          await deleteTransfer(transferId: transfer.id);
          return true;
        }
        return false;
      },
      onDismissed: (_) {
        ref.read(transfersPoolProvider.notifier).removeTransfer(transfer.id);
      },
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          GestureDetector(
            onTap: () {
              Navigator.of(context, rootNavigator: true).push(
                FastRoute(
                  builder: (_) => TransferDetailScreen(transferMeta: transfer),
                ),
              );
            },
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
              decoration: BoxDecoration(
                color: context.colors.secondaryCol,
                borderRadius: BorderRadius.circular(12),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    crossAxisAlignment: CrossAxisAlignment.center,
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              transfer.name,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                color: context.colors.primaryFont,
                                fontSize: 14,
                                fontWeight: FontWeight(600),
                                fontFamily: "Inter",
                              ),
                            ),
                            const SizedBox(height: 4),
                            Text(
                              transfer.direction == TransferDirection.incoming
                                  ? "From ${transfer.remoteDeviceName}"
                                  : "To ${transfer.remoteDeviceName}",
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                color: context.colors.secondaryFont,
                                fontSize: 12,
                              ),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: 12),
                      if (transfer.failedEntries.toInt() > 0) ...[
                        Icon(
                          Icons.warning_rounded,
                          color: Color(0xffF3523F),
                          size: 26,
                        ),
                        const SizedBox(width: 12),
                      ],
                      if (transfer.status == TransferStateDto.processing) ...[
                        Column(
                          crossAxisAlignment: CrossAxisAlignment.end,
                          children: [
                            Text(
                              '${((targetProgress + 0.001) * 100).toStringAsFixed(2)}%',
                              style: TextStyle(
                                color: context.colors.primaryFont,
                                fontSize: 12,
                              ),
                            ),
                            const SizedBox(height: 6),
                            Text(
                              _speedLabel(transfer.speedBps),
                              style: TextStyle(
                                color: context.colors.secondaryFont,
                                fontSize: 12,
                              ),
                            ),
                          ],
                        ),
                        // if pending.transfer_id
                      ] else ...[
                        AnimatedSwitcher(
                          duration: const Duration(milliseconds: 350),
                          transitionBuilder: (child, animation) =>
                              FadeTransition(
                                opacity: animation,
                                child: SlideTransition(
                                  position: Tween<Offset>(
                                    begin: const Offset(0, 0.3),
                                    end: Offset.zero,
                                  ).animate(animation),
                                  child: child,
                                ),
                              ),
                          child: TransferStatusBadge(
                            key: ValueKey('badge'),
                            state: transfer.status,
                          ),
                        ),
                      ],
                    ],
                  ),
                  AnimatedSize(
                    duration: const Duration(milliseconds: 400),
                    curve: Curves.easeInOut,
                    child: transfer.status == TransferStateDto.finished
                        ? const SizedBox.shrink()
                        : Column(
                            children: [
                              const SizedBox(height: 12),
                              RealProgressBar(progress: targetProgress),
                            ],
                          ),
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 16),
        ],
      ),
    );
  }
}

class BlurPercentText extends StatelessWidget {
  final double value;
  const BlurPercentText({super.key, required this.value});

  @override
  Widget build(BuildContext context) {
    final str = '${value.toStringAsFixed(3)}%';
    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 300),
      transitionBuilder: (child, anim) => ImageFiltered(
        imageFilter: ImageFilter.blur(
          sigmaX: (1 - anim.value) * 6,
          sigmaY: (1 - anim.value) * 6,
        ),
        child: FadeTransition(opacity: anim, child: child),
      ),
      child: Text(
        str,
        key: ValueKey(str),
        style: TextStyle(color: context.colors.primaryFont, fontSize: 12),
      ),
    );
  }
}

class RealProgressBar extends StatelessWidget {
  final double progress; // 0.0 to 1.0

  const RealProgressBar({super.key, required this.progress});

  @override
  Widget build(BuildContext context) {
    return TweenAnimationBuilder<double>(
      tween: Tween<double>(begin: 0.0, end: progress.clamp(0.0, 1.0)),
      duration: const Duration(milliseconds: 200),
      curve: Curves.linear,
      builder: (context, value, _) {
        return ClipRRect(
          borderRadius: BorderRadius.circular(1),
          child: LinearProgressIndicator(
            value: value,
            backgroundColor: context.colors.thirdCol,
            color: context.colors.primaryFont,
            minHeight: 1,
          ),
        );
      },
    );
  }
}

class TransferDetailScreen extends ConsumerStatefulWidget {
  final TransferMetaDto transferMeta;
  const TransferDetailScreen({super.key, required this.transferMeta});

  @override
  ConsumerState<TransferDetailScreen> createState() =>
      _TransferDetailScreenState();
}

class _TransferDetailScreenState extends ConsumerState<TransferDetailScreen> {
  final _items = <TransferItemDto>[];
  final _scroll = ScrollController();
  bool _loading = false;
  bool _hasMore = true;
  static const _pageSize = 30;

  @override
  void initState() {
    super.initState();
    _loadMore();
    _scroll.addListener(() {
      if (_scroll.position.pixels > _scroll.position.maxScrollExtent - 200) {
        _loadMore();
      }
    });
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _loadMore() async {
    if (_loading || !_hasMore) return;
    setState(() => _loading = true);
    final batch = await getTransferItems(
      transferId: widget.transferMeta.id,
      offset: BigInt.from(_items.length),
      limit: BigInt.from(_pageSize),
    );
    debugPrint("🟦 Loaded items batch len: ${batch.length}");

    setState(() {
      _items.addAll(batch);
      _hasMore = batch.length == _pageSize;
      _loading = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    final dirLabel =
        widget.transferMeta.direction == TransferDirection.outcoming
        ? 'Outcoming'
        : 'Incoming';

    final liveTransfer = ref.watch(
      transfersPoolProvider.select((list) {
        try {
          return list.firstWhere((t) => t.id == widget.transferMeta.id);
        } catch (_) {
          return null;
        }
      }),
    );
    final currentTransfer = liveTransfer ?? widget.transferMeta;

    return Scaffold(
      appBar: AppBar(
        elevation: 0,
        leading: IconButton(
          icon: const Icon(Icons.chevron_left),
          onPressed: () => Navigator.of(context).pop(),
          iconSize: 20,
          padding: EdgeInsets.zero,
        ),
        titleSpacing: 0,
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              currentTransfer.name,
              style: TextStyle(
                fontFamily: 'Rubik',
                fontWeight: FontWeight.w600,
                fontSize: 20,
                color: context.colors.primaryFont,
              ),
            ),
            Text(
              '$dirLabel · ${sizeOf(currentTransfer.totalSize)}',
              style: TextStyle(
                fontSize: 11,
                color: context.colors.secondaryFont,
              ),
            ),
          ],
        ),
        actions: [
          Padding(
            padding: const EdgeInsets.only(right: 20, top: 8),
            child: TransferStatusBadge(state: currentTransfer.status),
          ),
        ],
        // add three dots here to manage transfer
      ),
      body: _items.isEmpty && _loading
          ? const Center(child: Spinner())
          : _items.isEmpty
          ? Center(
              child: Text(
                'No files',
                style: TextStyle(color: context.colors.secondaryFont),
              ),
            )
          : ListView.separated(
              controller: _scroll,
              padding: const EdgeInsets.symmetric(
                horizontal: 16,
                vertical: 12,
              ).copyWith(bottom: 64),
              itemCount: _items.length,
              separatorBuilder: (_, _) =>
                  Divider(color: context.colors.classicBorder, height: 1),
              itemBuilder: (_, i) => _FileRow(item: _items[i]),
            ),
      floatingActionButton: _TransferControls(
        transfer: currentTransfer,
        onPauseResume: () async {
          await pauseResumeTransfer(transferId: currentTransfer.id);
        },
        onCancel: () async {
          // dialog still showing despite the status is finished !
          if (currentTransfer.status != TransferStateDto.finished &&
              currentTransfer.status != TransferStateDto.cancelled) {
            final confirm = await showConfirmDialog(
              context,
              title: 'Cancel transfer',
              message: 'This transfer is not finished.\nProgress will be lost.',
              keepLabel: 'Keep',
              removeLabel: 'Remove',
            );
            if (confirm != true) return;
          }

          await deleteTransfer(transferId: currentTransfer.id);
          if (context.mounted) {
            Navigator.pop(context);
          }
        },
      ),
    );
  }
}

class _TransferControls extends StatefulWidget {
  final TransferMetaDto transfer;
  final VoidCallback onPauseResume;
  final VoidCallback onCancel;
  const _TransferControls({
    required this.transfer,
    required this.onPauseResume,
    required this.onCancel,
  });

  @override
  State<_TransferControls> createState() => _TransferControlsState();
}

class _FailedTransferItemsList extends StatefulWidget {
  final TransferMetaDto transfer;

  const _FailedTransferItemsList({required this.transfer});

  @override
  State<_FailedTransferItemsList> createState() =>
      _FailedTransferItemsListState();
}

class _FailedTransferItemsListState extends State<_FailedTransferItemsList> {
  static const _pageSize = 30;
  final _items = <TransferItemDto>[];
  final _scroll = ScrollController();
  bool _loading = false;
  bool _hasMore = true;

  @override
  void initState() {
    super.initState();
    _loadMore();
    _scroll.addListener(() {
      if (_scroll.position.pixels > _scroll.position.maxScrollExtent - 200) {
        _loadMore();
      }
    });
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _loadMore() async {
    if (_loading || !_hasMore) return;
    setState(() => _loading = true);
    final batch = await getFailedTransferItems(
      transferId: widget.transfer.id,
      offset: BigInt.from(_items.length),
      limit: BigInt.from(_pageSize),
    );

    setState(() {
      _items.addAll(batch);
      _hasMore = batch.length == _pageSize;
      _loading = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        elevation: 0,
        leading: IconButton(
          icon: const Icon(Icons.chevron_left),
          onPressed: () => Navigator.of(context).pop(),
          iconSize: 20,
          padding: EdgeInsets.zero,
        ),
        titleSpacing: 0,
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              "Failed items",
              style: TextStyle(
                fontFamily: 'Rubik',
                fontWeight: FontWeight.w600,
                fontSize: 20,
                color: context.colors.primaryFont,
              ),
            ),
            Text(
              widget.transfer.failedEntries.toString(),
              style: TextStyle(
                fontSize: 11,
                color: context.colors.secondaryFont,
              ),
            ),
          ],
        ),
      ),
      // can be transformed to widget
      body: _items.isEmpty && _loading
          ? const Center(child: Spinner())
          : _items.isEmpty
          ? Center(
              child: Text(
                'No failed items',
                style: TextStyle(color: context.colors.secondaryFont),
              ),
            )
          : ListView.separated(
              controller: _scroll,
              padding: const EdgeInsets.symmetric(
                horizontal: 16,
                vertical: 12,
              ).copyWith(bottom: 64),
              itemCount: _items.length,
              separatorBuilder: (_, _) =>
                  Divider(color: context.colors.classicBorder, height: 1),
              itemBuilder: (_, i) => _FileRow(item: _items[i]),
            ),
    );
  }
}

class _TransferControlsState extends State<_TransferControls> {
  bool _debouncing = false;

  Future<void> _handlePauseResume() async {
    if (_debouncing) return;
    setState(() => _debouncing = true);
    widget.onPauseResume();
    await Future.delayed(const Duration(seconds: 2));
    if (mounted) setState(() => _debouncing = false);
  }

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        // delete
        FloatingActionButton(
          heroTag: 'cancel',
          onPressed: widget.onCancel,
          backgroundColor: context.colors.primaryFont,
          foregroundColor: context.colors.primaryCol,
          child: const Icon(Icons.delete_rounded, size: 18),
        ),
        if (widget.transfer.failedEntries.toInt() > 0) ...[
          const SizedBox(width: 12),
          FloatingActionButton(
            heroTag: 'show failed items',
            onPressed: () {
              Navigator.of(context, rootNavigator: true).push(
                FastRoute(
                  builder: (_) =>
                      _FailedTransferItemsList(transfer: widget.transfer),
                ),
              );
            },
            backgroundColor: Color(0xffF3523F),
            foregroundColor: _debouncing
                ? context.colors.thirdFont
                : context.colors.primaryCol,
            child: Icon(Icons.warning_rounded, color: Colors.white, size: 26),
          ),
        ],
        // const SizedBox(width: 12),
        // if (widget.transfer.status == TransferStateDto.suspended) ...[
        //   const SizedBox(width: 12),
        //   FloatingActionButton(
        //     heroTag: 'pause',
        //     onPressed: _debouncing ? null : _handlePauseResume,
        //     backgroundColor: _debouncing
        //         ? context.colors.thirdCol
        //         : context.colors.primaryFont,
        //     foregroundColor: _debouncing
        //         ? context.colors.thirdFont
        //         : context.colors.primaryCol,
        //     child: const Icon(Icons.play_arrow_rounded),
        //   ),
        // ] else
        if (widget.transfer.status == TransferStateDto.processing) ...[
          const SizedBox(width: 12),
          FloatingActionButton(
            heroTag: 'pause',
            onPressed: _debouncing ? null : _handlePauseResume,
            backgroundColor: _debouncing
                ? context.colors.thirdCol
                : context.colors.primaryFont,
            foregroundColor: _debouncing
                ? context.colors.thirdFont
                : context.colors.primaryCol,
            child: const Icon(Icons.pause_rounded),
          ),
        ],
      ],
    );
  }
}

bool _isImage(String name) {
  final ext = name.split('.').last.toLowerCase();
  return ['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic'].contains(ext);
}

bool _isVideo(String name) {
  final ext = name.split('.').last.toLowerCase();
  return ['mp4', 'mov', 'avi', 'mkv'].contains(ext);
}

IconData iconForFile(String name) {
  final ext = name.split('.').last.toLowerCase();
  if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic'].contains(ext)) {
    return Icons.image_outlined;
  }
  if (['mp4', 'mov', 'avi', 'mkv'].contains(ext)) {
    return Icons.videocam_outlined;
  }
  if (ext == 'pdf') return Icons.picture_as_pdf_outlined;
  if (['zip', 'tar', 'gz', 'rar'].contains(ext)) {
    return Icons.archive_outlined;
  }
  return Icons.insert_drive_file_outlined;
}

String sizeOf(BigInt bytes) {
  final double bytesDouble = bytes.toDouble();
  final kb = bytesDouble / 1024;
  if (kb < 1024) return '${kb.toStringAsFixed(1)} KB';
  final mb = kb / 1024;
  if (mb < 1024) return '${mb.toStringAsFixed(1)} MB';
  final gb = mb / 1024;
  return '${gb.toStringAsFixed(1)} GB';
}

String getSendItemError(SendItemErrorDto error) {
  switch (error) {
    case SendItemErrorDto.fileAlreadyExisted:
      return 'File already exsists.';
    case SendItemErrorDto.fileIoError:
      return 'Failed to read file.';
    case SendItemErrorDto.fileOpenPermissionDenied:
      return 'No permission.';
    case SendItemErrorDto.fileNotFound:
      return 'File missing.';
    case SendItemErrorDto.streamError:
      return 'Send fail.';
    case SendItemErrorDto.openFail:
      return 'Open fail.';
    case SendItemErrorDto.terminated:
      return 'Cancelled.';
  }
}

class _FileRowSimple extends StatelessWidget {
  final String path;
  final BigInt size;
  final bool isDir;
  const _FileRowSimple({
    required this.path,
    required this.size,
    this.isDir = false,
  });

  String get _name => path.split('/').last;

  Widget _iconBox(BuildContext context, IconData icon) => Container(
    width: 40,
    height: 40,
    decoration: BoxDecoration(
      color: context.colors.secondaryCol,
      borderRadius: BorderRadius.circular(10),
    ),
    child: Icon(icon, color: context.colors.thirdFont, size: 26),
  );

  void _openPreview(BuildContext context) {
    debugPrint("🟦 Opening preview.");
    Navigator.of(
      context,
      rootNavigator: true,
    ).push(FastRoute(builder: (_) => _FullImageScreen(filePath: path)));
  }

  @override
  Widget build(BuildContext context) {
    final isImg = !isDir && _isImage(_name);
    final isVid = !isDir && _isVideo(_name);

    Widget preview;
    if (isDir) {
      preview = _iconBox(context, Icons.folder_outlined);
    } else if (isImg) {
      preview = ClipRRect(
        borderRadius: BorderRadius.circular(10),
        child: Image(
          image: ResizeImage(FileImage(File(path)), width: 80, height: 80),
          width: 40,
          height: 40,
          fit: BoxFit.cover,
          frameBuilder: (ctx, child, frame, _) =>
              frame == null ? _iconBox(context, Icons.image_outlined) : child,
          errorBuilder: (_, _, _) =>
              _iconBox(context, Icons.broken_image_outlined),
        ),
      );
    } else if (isVid) {
      preview = Stack(
        children: [
          _iconBox(context, Icons.videocam_outlined),
          Positioned.fill(
            child: Center(
              child: Icon(
                Icons.play_circle_outline,
                color: context.colors.thirdFont,
                size: 24,
              ),
            ),
          ),
        ],
      );
    } else {
      preview = _iconBox(context, iconForFile(_name));
    }

    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () {
        if (isImg) _openPreview(context);
      },
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 10),
        child: Row(
          children: [
            SizedBox(width: 40, height: 40, child: preview),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    _name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: context.colors.primaryFont,
                      fontFamily: "Inter",
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                  if (!isDir) ...[
                    const SizedBox(height: 2),
                    Text(
                      sizeOf(size),
                      style: TextStyle(
                        color: context.colors.thirdFont,
                        fontFamily: "Inter",
                        fontSize: 12,
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _FileRow extends StatelessWidget {
  final TransferItemDto item;
  const _FileRow({required this.item});

  Widget _iconBox(BuildContext context, IconData icon) => Container(
    width: 40,
    height: 40,
    decoration: BoxDecoration(
      color: context.colors.secondaryCol,
      borderRadius: BorderRadius.circular(10),
    ),
    child: Icon(icon, color: context.colors.thirdFont, size: 26),
  );

  Widget _errorBox(BuildContext context) => Container(
    width: 40,
    height: 40,
    decoration: BoxDecoration(
      color: context.colors.secondaryCol,
      borderRadius: BorderRadius.circular(10),
    ),
    child: Icon(Icons.warning_rounded, color: Color(0xffF3523F), size: 26),
  );

  void _openPreview(BuildContext context) {
    debugPrint("🟦 Opening preview.");

    Navigator.of(
      context,
      rootNavigator: true,
    ).push(FastRoute(builder: (_) => _FullImageScreen(filePath: item.path)));
  }

  @override
  Widget build(BuildContext context) {
    final isImg = _isImage(item.name);
    final isVid = _isVideo(item.name);

    Widget preview;
    if (isImg) {
      preview = ClipRRect(
        borderRadius: BorderRadius.circular(10),
        child: Image(
          image: ResizeImage(FileImage(File(item.path)), width: 80, height: 80),
          width: 40,
          height: 40,
          fit: BoxFit.cover,
          frameBuilder: (ctx, child, frame, _) => frame == null
              ? _iconBox(
                  context,
                  Icons.image_outlined,
                ) // show icon while loading
              : child,
          errorBuilder: (_, _, _) =>
              _iconBox(context, Icons.broken_image_outlined),
        ),
      );
    } else if (isVid) {
      preview = Stack(
        children: [
          _iconBox(context, Icons.videocam_outlined),
          Positioned.fill(
            child: Center(
              child: Icon(
                Icons.play_circle_outline,
                color: context.colors.thirdFont,
                size: 24,
              ),
            ),
          ),
        ],
      );
    } else {
      preview = _iconBox(context, iconForFile(item.name));
    }
    final isError =
        item.error == SendItemErrorDto.fileNotFound ||
        item.error == SendItemErrorDto.fileOpenPermissionDenied ||
        item.error == SendItemErrorDto.fileIoError ||
        item.error == SendItemErrorDto.fileAlreadyExisted ||
        item.error == SendItemErrorDto.openFail;
    if (isError) {
      preview = _errorBox(context);
    }
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () async {
        if (isError) {
          Navigator.of(
            context,
            rootNavigator: true,
          ).push(FastRoute(builder: (_) => _SendItemErrorScreen(item: item)));
        } else if (isImg) {
          _openPreview(context);
        }
      },
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 10),
        child: Row(
          children: [
            SizedBox(width: 40, height: 40, child: preview),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    item.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: context.colors.primaryFont,
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                  const SizedBox(height: 2),
                  if (isError) ...[
                    Text(
                      getSendItemError(item.error!),
                      style: const TextStyle(
                        color: Color(0xffF3523F),
                        fontSize: 12,
                      ),
                    ),
                  ] else ...[
                    Text(
                      sizeOf(item.size),
                      style: TextStyle(
                        color: context.colors.thirdFont,
                        fontSize: 12,
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _FullImageScreen extends StatefulWidget {
  final String filePath;
  const _FullImageScreen({required this.filePath});

  @override
  State<_FullImageScreen> createState() => _FullImageScreenState();
}

class _FullImageScreenState extends State<_FullImageScreen> {
  bool _showUi = true;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      body: GestureDetector(
        onTap: () => setState(() => _showUi = !_showUi),
        child: Stack(
          children: [
            Center(
              child: InteractiveViewer(
                minScale: 0.5,
                maxScale: 6.0,
                clipBehavior: Clip.none,
                child: SizedBox.expand(
                  child: Image.file(
                    File(widget.filePath),
                    fit: BoxFit.contain,
                    filterQuality: FilterQuality.high,
                    errorBuilder: (_, _, _) => const Icon(
                      Icons.broken_image,
                      color: Colors.white38,
                      size: 64,
                    ),
                  ),
                ),
              ),
            ),
            if (_showUi)
              SafeArea(
                child: Padding(
                  padding: const EdgeInsets.all(8),
                  child: IconButton(
                    onPressed: () => Navigator.of(context).pop(),
                    icon: Icon(
                      Icons.chevron_left,
                      color: context.colors.primaryCol,
                    ),
                    iconSize: 28,
                    style: IconButton.styleFrom(
                      backgroundColor: context.colors.primaryFont,
                      shape: const CircleBorder(),
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

class TransferOfferDialog extends StatefulWidget {
  final TransferOfferDto offer;
  const TransferOfferDialog({required this.offer, super.key});

  @override
  State<TransferOfferDialog> createState() => _TransferOfferDialogState();
}

class _TransferOfferDialogState extends State<TransferOfferDialog> {
  final _scroll = ScrollController();
  bool responded = false;
  bool defaultLocation = true;
  String? _selectedDir;

  Future<void> _initDefaultDir() async {
    late String path;

    try {
      if (Platform.isAndroid) {
        path = '/storage/emulated/0/Download';
      } else if (Platform.isIOS) {
        final dir = await getApplicationDocumentsDirectory();
        path = dir.path;
      } else {
        final dir = await getDownloadsDirectory();
        path = dir?.path ?? '';
      }
    } catch (e) {
      debugPrint("Failed to get def dir. $e");
    }
    setState(() {
      _selectedDir = '$path/${widget.offer.name}';
      defaultLocation = false;
    });
  }

  Future<void> _pickDirectory() async {
    final dir = await FilePicker.platform.getDirectoryPath(
      dialogTitle: 'Choose download folder',
    );
    debugPrint("🟧 Selected directory: $dir");
    if (dir != null) setState(() => _selectedDir = '$dir/${widget.offer.name}');
  }

  @override
  void initState() {
    super.initState();
    _initDefaultDir();
  }

  @override
  void dispose() {
    if (responded == false) {
      respondToTransferOffer(accept: false, destination: "");
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final transferSize = widget.offer.size;
    final filesNum = widget.offer.filesNum;

    return Dialog.fullscreen(
      child: Scaffold(
        backgroundColor: context.colors.primaryCol,
        body: SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Accept transfer',
                  style: TextStyle(
                    color: context.colors.primaryFont,
                    fontSize: 30,
                    fontWeight: FontWeight.w600,
                    fontFamily: "Rubik",
                  ),
                ),
                const SizedBox(height: 24),

                Text(
                  "Transfer name",
                  style: TextStyle(
                    fontSize: 13,
                    fontFamily: "Rubik",
                    color: context.colors.secondaryFont,
                  ),
                ),
                Text(
                  widget.offer.name,
                  style: TextStyle(
                    fontSize: 20,
                    fontWeight: FontWeight.w600,
                    fontFamily: 'Rubik',
                    color: context.colors.primaryFont,
                  ),
                ),
                const SizedBox(height: 12),
                Text(
                  "From",
                  style: TextStyle(
                    fontSize: 13,
                    color: context.colors.secondaryFont,
                  ),
                ),
                Text(
                  widget.offer.senderDeviceName,
                  style: TextStyle(
                    fontSize: 20,
                    fontWeight: FontWeight.w600,
                    fontFamily: 'Rubik',
                    color: context.colors.primaryFont,
                  ),
                ),
                const SizedBox(height: 12),

                Text(
                  '${sizeOf(transferSize)}   $filesNum items',
                  style: TextStyle(
                    fontSize: 15,
                    fontWeight: FontWeight.w500,
                    fontFamily: "Rubik",
                    color: context.colors.primaryFont,
                  ),
                ),
                const SizedBox(height: 16),
                Text(
                  "Save to",
                  style: TextStyle(
                    fontSize: 13,
                    fontFamily: "Rubik",
                    color: context.colors.secondaryFont,
                  ),
                ),
                const SizedBox(height: 8),

                GestureDetector(
                  onTap: _pickDirectory,
                  child: Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 14,
                      vertical: 12,
                    ),
                    decoration: BoxDecoration(
                      color: context.colors.secondaryCol,
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: context.colors.classicBorder),
                    ),
                    child: Row(
                      children: [
                        Icon(
                          Icons.folder_open_outlined,
                          color: context.colors.secondaryFont,
                          size: 20,
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              SizedBox(
                                height: 20,
                                child: defaultLocation == false
                                    ? Text(
                                        _selectedDir!.split('/').last,
                                        style: TextStyle(
                                          fontSize: 15,
                                          fontWeight: FontWeight.w500,
                                          color: context.colors.primaryFont,
                                          fontFamily: 'Rubik',
                                        ),
                                      )
                                    : Text(
                                        'Default location',
                                        style: TextStyle(
                                          fontSize: 15,
                                          fontWeight: FontWeight.w500,
                                          color: context.colors.primaryFont,
                                          fontFamily: 'Rubik',
                                        ),
                                      ),
                              ),
                              if (defaultLocation == false) ...[
                                SizedBox(
                                  height: 16,
                                  child: Marquee(
                                    text: _selectedDir ?? 'Default location',
                                    style: TextStyle(
                                      fontSize: 11,
                                      fontFamily: "Rubik",
                                      color: context.colors.secondaryFont,
                                    ),
                                    velocity: 30,
                                    blankSpace: 60,
                                    pauseAfterRound: const Duration(seconds: 2),
                                    startAfter: const Duration(seconds: 1),
                                  ),
                                ),
                              ],
                            ],
                          ),
                        ),
                        Icon(
                          Icons.chevron_right,
                          color: context.colors.thirdFont,
                          size: 20,
                        ),
                      ],
                    ),
                  ),
                ),

                if (true) ...[
                  const SizedBox(height: 16),

                  Text(
                    "${widget.offer.filesNum} items",
                    style: TextStyle(
                      fontFamily: "Rubik",
                      fontSize: 13,
                      color: context.colors.secondaryFont,
                    ),
                  ),

                  const SizedBox(height: 4),
                  Expanded(
                    child: ListView.separated(
                      controller: _scroll,
                      padding: EdgeInsets.zero,
                      itemCount: widget.offer.filesNum.toInt(),
                      separatorBuilder: (_, _) => Divider(
                        color: context.colors.classicBorder,
                        height: 1,
                      ),
                      itemBuilder: (_, i) {
                        return _FileRowSimple(
                          path: widget.offer.files[i].$1,
                          size: widget.offer.files[i].$2,
                          isDir: widget.offer.files[i].$3,
                        );
                      },
                    ),
                  ),
                ],

                const SizedBox(height: 16),

                Row(
                  children: [
                    Expanded(
                      child: PrimaryWideButton(
                        label: "Reject",
                        destructive: true,
                        onPressed: () => {
                          respondToTransferOffer(
                            accept: false,
                            destination: "",
                          ),
                          setState(() {
                            responded = true;
                          }),
                          Navigator.of(context).pop(false),
                        },
                      ),
                    ),
                    const SizedBox(width: 16),
                    Expanded(
                      child: PrimaryWideButton(
                        label: "Accept",
                        onPressed: () async {
                          await requestStoragePermissions();
                          respondToTransferOffer(
                            accept: true,
                            destination: _selectedDir!,
                          );
                          setState(() {
                            responded = true;
                          });
                          if (context.mounted) {
                            Navigator.of(context).pop(false);
                          }
                        },
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
