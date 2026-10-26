// Общие мелкие виджеты.

import 'package:flutter/material.dart';

import '../app/theme.dart';

/// Значок платформы в круге с точкой «в сети».
class PeerAvatar extends StatelessWidget {
  const PeerAvatar({super.key, required this.platform, this.online, this.size = 44});

  final String platform;

  /// null — без точки.
  final bool? online;
  final double size;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final dot = size * 0.3;
    return SizedBox(
      width: size,
      height: size,
      child: Stack(
        clipBehavior: Clip.none,
        children: [
          Container(
            width: size,
            height: size,
            decoration: BoxDecoration(color: scheme.secondaryContainer, shape: BoxShape.circle),
            child: Icon(platformIcon(platform), size: size * 0.5, color: scheme.onSecondaryContainer),
          ),
          if (online != null)
            Positioned(
              right: -1,
              bottom: -1,
              child: Container(
                width: dot,
                height: dot,
                decoration: BoxDecoration(
                  color: online! ? Brand.ok : scheme.outline,
                  shape: BoxShape.circle,
                  border: Border.all(color: scheme.surface, width: 2),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// Короткий код устройства в рамке: «AB12».
class CodeChip extends StatelessWidget {
  const CodeChip(this.code, {super.key});

  final String code;

  @override
  Widget build(BuildContext context) {
    if (code.isEmpty) return const SizedBox.shrink();
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: Text(
        code,
        style: TextStyle(
          fontSize: 11,
          letterSpacing: 1,
          fontFeatures: const [FontFeature.tabularFigures()],
          color: scheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

/// Небольшая плашка («разовый»).
class Tag extends StatelessWidget {
  const Tag(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(color: scheme.tertiaryContainer, borderRadius: BorderRadius.circular(6)),
      child: Text(text, style: TextStyle(fontSize: 11, color: scheme.onTertiaryContainer)),
    );
  }
}

/// Заголовок раздела списка.
class SectionHeader extends StatelessWidget {
  const SectionHeader(this.title, {super.key, this.trailing});

  final String title;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(4, 20, 4, 8),
      child: Row(
        children: [
          Expanded(
            child: Text(
              title,
              style: theme.textTheme.titleSmall?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          if (trailing != null) trailing!,
        ],
      ),
    );
  }
}

/// Знак Ogneliss Share (кольцо обмена с огненной лисой).
class BrandMark extends StatelessWidget {
  const BrandMark({super.key, this.size = 96});

  final double size;

  @override
  Widget build(BuildContext context) {
    return Image.asset('assets/brand/mark.png', width: size, height: size, filterQuality: FilterQuality.medium);
  }
}

/// Ограничение ширины содержимого на широких экранах.
class Narrow extends StatelessWidget {
  const Narrow({super.key, required this.child, this.maxWidth = 720});

  final Widget child;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: ConstrainedBox(constraints: BoxConstraints(maxWidth: maxWidth), child: child),
    );
  }
}
