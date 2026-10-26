// Строка передачи: что, кому/от кого, ход и кнопки.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app/app_model.dart';
import '../app/format.dart';
import '../app/strings.dart';
import '../app/theme.dart';
import '../core/models.dart';
import '../core/native.dart';
import '../platform/files.dart';
import 'shell.dart';

class TransferTile extends StatelessWidget {
  const TransferTile({super.key, required this.item});

  final TransferItem item;

  String _title() {
    if (item.isText) {
      final t = (item.text ?? '').replaceAll(RegExp(r'\s+'), ' ').trim();
      return t.isEmpty ? tr.textItem : '${tr.textItem}: «$t»';
    }
    return formatNames(item.names, item.count);
  }

  String _status() {
    switch (item.status) {
      case 'preparing':
        return tr.statusPreparing;
      case 'connecting':
        return tr.statusConnecting;
      case 'waiting_peer':
        return tr.statusWaitingPeer;
      case 'waiting':
        return tr.statusWaiting;
      case 'asking':
        return tr.statusAsking;
      case 'retrying':
        return tr.statusRetrying;
      case 'active':
        return tr.progress(
          formatBytes(item.done),
          formatBytes(item.total),
          formatRate(item.rate),
          formatLeft(item.done, item.total, item.rate),
        );
      case 'done':
        final base = item.outgoing ? tr.statusSent : tr.statusReceived;
        return item.isText ? base : '$base · ${formatBytes(item.total)}';
      case 'declined':
        return tr.statusDeclined;
      case 'cancelled':
        if (!item.byPeer) return tr.statusCancelled;
        return item.outgoing ? tr.statusCancelledByReceiver : tr.statusCancelledBySender;
      case 'failed':
        final text = tr.error(item.error);
        return item.failedFiles > 0 ? '$text (${tr.failedFiles(item.failedFiles)})' : text;
    }
    return item.status;
  }

  @override
  Widget build(BuildContext context) {
    final model = AppScope.read(context);
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final failed = item.status == 'failed' || item.status == 'declined';
    final IconData icon;
    if (item.isText) {
      icon = Icons.notes_rounded;
    } else if (item.outgoing) {
      icon = Icons.north_east_rounded;
    } else {
      icon = Icons.south_west_rounded;
    }
    final Color iconColor;
    if (failed) {
      iconColor = scheme.error;
    } else if (item.isDone) {
      iconColor = Brand.ok;
    } else {
      iconColor = scheme.primary;
    }
    final direction = item.outgoing ? '→ ${item.peerName}' : '← ${item.peerName}';
    final showProgress = item.isActive && !item.isText;
    final indeterminate = item.status != 'active';

    return Card(
      child: InkWell(
        borderRadius: BorderRadius.circular(16),
        onTap: () => _open(context, model),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 8, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Icon(icon, color: iconColor),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(_title(), maxLines: 1, overflow: TextOverflow.ellipsis, style: theme.textTheme.bodyLarge),
                        const SizedBox(height: 2),
                        Text(
                          '$direction · ${_status()}',
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: theme.textTheme.bodySmall?.copyWith(
                            color: failed ? scheme.error : scheme.onSurfaceVariant,
                          ),
                        ),
                      ],
                    ),
                  ),
                  ..._actions(context, model),
                ],
              ),
              if (showProgress)
                Padding(
                  padding: const EdgeInsets.only(top: 10, right: 8),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(4),
                    child: LinearProgressIndicator(
                      value: indeterminate ? null : item.progress,
                      minHeight: 6,
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  List<Widget> _actions(BuildContext context, AppModel model) {
    final out = <Widget>[];
    if (item.isActive) {
      out.add(IconButton(
        tooltip: tr.cancel,
        icon: const Icon(Icons.close_rounded),
        onPressed: () => model.cancelTransfer(item.id),
      ));
      return out;
    }
    if (item.canRetry) {
      out.add(IconButton(
        tooltip: tr.retry,
        icon: const Icon(Icons.refresh_rounded),
        onPressed: () {
          try {
            model.retryTransfer(item.id);
          } on CoreError catch (e) {
            showSnack(tr.error(e.code));
          }
        },
      ));
    }
    if (!item.outgoing && item.isDone && item.isText) {
      out.add(IconButton(
        tooltip: tr.copy,
        icon: const Icon(Icons.copy_rounded),
        onPressed: () => _copyText(model),
      ));
    } else if (!item.outgoing && item.isDone && item.savedTo != null) {
      out.add(IconButton(
        tooltip: Files.isDirectory(item.savedTo!) ? tr.openFolder : tr.open,
        icon: Icon(Files.isDirectory(item.savedTo!) ? Icons.folder_open_rounded : Icons.open_in_new_rounded),
        onPressed: () => Files.open(item.savedTo!),
      ));
    }
    out.add(IconButton(
      tooltip: tr.close,
      icon: const Icon(Icons.clear_rounded, size: 20),
      onPressed: () => model.dismissTransfer(item.id),
    ));
    return out;
  }

  void _copyText(AppModel model) {
    final text = model.fullText(item.id) ?? item.text ?? '';
    Clipboard.setData(ClipboardData(text: text));
    showSnack(tr.copied, duration: const Duration(seconds: 2));
  }

  Future<void> _open(BuildContext context, AppModel model) async {
    if (item.isText) {
      final text = model.fullText(item.id) ?? item.text ?? '';
      await showDialog<void>(
        context: context,
        builder: (_) => TextDialog(
          title: item.outgoing ? tr.sendTo(item.peerName) : tr.receivedText(item.peerName),
          text: text,
        ),
      );
      return;
    }
    final saved = item.savedTo;
    if (!item.outgoing && item.isDone && saved != null) {
      await Files.open(saved);
    }
  }
}
