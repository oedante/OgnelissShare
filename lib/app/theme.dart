// Оформление — общее с Ogneliss Mail (lib/core/theme/app_theme.dart там):
// глубокий синий основной цвет, светло-серо-голубой фон, белые карточки без
// рамки («воздушный» вид), подписи разделов синим, кнопки и поля со
// скруглением 12–16, Roboto из приложения и значки Material Symbols Rounded
// толщиной 300. Тёмная тема — те же оттенки, что у Mail. Золото и сланец
// остались у знака приложения и QR-кода.

import 'package:flutter/cupertino.dart' show CupertinoPageTransitionsBuilder;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show SystemUiOverlayStyle;

import 'icons.dart';

class Brand {
  Brand._();

  /// Основной цвет линейки (как brandBlue в Ogneliss Mail).
  static const blue = Color(0xFF1D4F91);
  static const slate = Color(0xFF3A4450);
  static const slateDark = Color(0xFF232A32);
  static const gold = Color(0xFFCCAA6D);

  /// «В сети», «готово»: приглушённый зелёный, как successGreen в Mail.
  static const ok = Color(0xFF2E8B57);
}

/// Отступы и скругления — те же шаги, что в Ogneliss Mail (tokens.dart).
abstract final class Space {
  static const xxs = 2.0;
  static const xs = 4.0;
  static const s = 8.0;
  static const m = 12.0;
  static const l = 16.0;
  static const xl = 24.0;
  static const xxl = 32.0;
}

abstract final class Radii {
  static const chip = 12.0;
  static const card = 20.0;
  static const panel = 28.0;
  static const cardAll = BorderRadius.all(Radius.circular(card));
}

class AppTheme {
  AppTheme._();

  static const fontFamily = 'AppRoboto';

  /// Переходы между экранами: без анимации на Android и компьютерах, на
  /// iPhone и iPad — системный сдвиг (с ним работает свайп «назад»).
  static const _transitions = PageTransitionsTheme(builders: {
    TargetPlatform.android: NoPageTransitionsBuilder(),
    TargetPlatform.iOS: CupertinoPageTransitionsBuilder(),
    TargetPlatform.macOS: NoPageTransitionsBuilder(),
    TargetPlatform.windows: NoPageTransitionsBuilder(),
    TargetPlatform.linux: NoPageTransitionsBuilder(),
  });

  static TextSelectionThemeData _selection(Brightness b) {
    final c = b == Brightness.dark ? const Color(0xFF8AB1E6) : const Color(0xFF2F6DB5);
    return TextSelectionThemeData(cursorColor: c, selectionColor: c.withValues(alpha: 0.28), selectionHandleColor: c);
  }

  static ThemeData light() {
    final scheme = ColorScheme.fromSeed(seedColor: Brand.blue).copyWith(
      primary: Brand.blue,
      onPrimary: Colors.white,
      primaryContainer: const Color(0xFFD9E5F6),
      onPrimaryContainer: const Color(0xFF0B2545),
      secondary: const Color(0xFF4A5A70),
      onSecondary: Colors.white,
      secondaryContainer: const Color(0xFFDCE6F3),
      onSecondaryContainer: const Color(0xFF13263F),
      tertiary: const Color(0xFFA35A0B),
      onTertiary: Colors.white,
      tertiaryContainer: const Color(0xFFF5E6CC),
      onTertiaryContainer: const Color(0xFF7A4E0E),
      surface: Colors.white,
      onSurface: const Color(0xFF242A33),
      onSurfaceVariant: const Color(0xFF5A6472),
      surfaceContainerLowest: Colors.white,
      surfaceContainerLow: const Color(0xFFF2F5F9),
      surfaceContainer: const Color(0xFFF0F3F7),
      surfaceContainerHigh: const Color(0xFFEAEEF3),
      surfaceContainerHighest: const Color(0xFFE3E8EF),
      outline: const Color(0xFF8A94A3),
      outlineVariant: const Color(0xFFE1E6EC),
    );
    return _base(
      scheme,
      background: const Color(0xFFF2F5F9),
      card: Colors.white,
      field: const Color(0xFFE9EDF2),
      divider: const Color(0xFFEBEEF2),
      overlay: SystemUiOverlayStyle.dark,
    );
  }

  static ThemeData dark() {
    final scheme = ColorScheme.fromSeed(seedColor: Brand.blue, brightness: Brightness.dark).copyWith(
      primary: const Color(0xFF9DBDEB),
      onPrimary: const Color(0xFF0B2545),
      primaryContainer: const Color(0xFF233E63),
      onPrimaryContainer: const Color(0xFFD9E5F6),
      secondary: const Color(0xFFB4C1D3),
      onSecondary: const Color(0xFF1B2533),
      secondaryContainer: const Color(0xFF2A3A50),
      onSecondaryContainer: const Color(0xFFDCE6F3),
      tertiary: const Color(0xFFF0B866),
      onTertiary: const Color(0xFF3A2200),
      tertiaryContainer: const Color(0xFF45351E),
      onTertiaryContainer: const Color(0xFFEAC48A),
      surface: const Color(0xFF161A20),
      onSurface: const Color(0xFFDCE1E8),
      onSurfaceVariant: const Color(0xFFA3ACB9),
      surfaceContainerLowest: const Color(0xFF101318),
      surfaceContainerLow: const Color(0xFF181D24),
      surfaceContainer: const Color(0xFF1D232B),
      surfaceContainerHigh: const Color(0xFF242B34),
      surfaceContainerHighest: const Color(0xFF2C343E),
      outline: const Color(0xFF7D8796),
      outlineVariant: const Color(0xFF2D343E),
    );
    return _base(
      scheme,
      background: const Color(0xFF12161B),
      card: const Color(0xFF1D232B),
      field: const Color(0xFF232A33),
      divider: const Color(0xFF262C35),
      overlay: SystemUiOverlayStyle.light,
    );
  }

  static ThemeData _base(
    ColorScheme s, {
    required Color background,
    required Color card,
    required Color field,
    required Color divider,
    required SystemUiOverlayStyle overlay,
  }) {
    const r12 = RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(12)));
    const r16 = RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(16)));
    return ThemeData(
      useMaterial3: true,
      colorScheme: s,
      scaffoldBackgroundColor: background,
      fontFamily: fontFamily,
      // Один вид на всех платформах: плотность и «волна» не зависят от ОС.
      visualDensity: VisualDensity.standard,
      materialTapTargetSize: MaterialTapTargetSize.padded,
      splashFactory: InkRipple.splashFactory,
      pageTransitionsTheme: _transitions,
      iconTheme: IconThemeData(color: s.onSurfaceVariant, weight: AppIcons.weight),
      appBarTheme: AppBarTheme(
        centerTitle: false,
        elevation: 0,
        scrolledUnderElevation: 0,
        surfaceTintColor: Colors.transparent,
        // Шапка цвета поверхности (белая в светлой теме), как в Mail.
        backgroundColor: s.surface,
        foregroundColor: s.onSurface,
        systemOverlayStyle: overlay,
        iconTheme: IconThemeData(color: s.onSurface, size: 24, weight: AppIcons.weight),
        actionsIconTheme: IconThemeData(color: s.onSurfaceVariant, size: 24, weight: AppIcons.weight),
      ),
      // Карточка отделяется тоном, без рамки («воздушный» вид Mail).
      cardTheme: CardThemeData(
        elevation: 0,
        margin: EdgeInsets.zero,
        color: card,
        shape: const RoundedRectangleBorder(borderRadius: Radii.cardAll),
      ),
      dialogTheme: DialogThemeData(backgroundColor: card, surfaceTintColor: Colors.transparent),
      bottomSheetTheme: BottomSheetThemeData(
        backgroundColor: background,
        surfaceTintColor: Colors.transparent,
        showDragHandle: true,
        shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(Radii.panel))),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: field,
        border: const OutlineInputBorder(borderRadius: BorderRadius.all(Radius.circular(16)), borderSide: BorderSide.none),
        contentPadding: const EdgeInsets.symmetric(horizontal: Space.l, vertical: 14),
      ),
      chipTheme: const ChipThemeData(shape: r12),
      dividerTheme: DividerThemeData(space: 1, thickness: 1, color: divider),
      listTileTheme: ListTileThemeData(
        shape: r12,
        contentPadding: const EdgeInsets.symmetric(horizontal: Space.l),
        iconColor: s.primary,
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(shape: r12, minimumSize: const Size(64, 44)),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(shape: r12, minimumSize: const Size(64, 44)),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(shape: r12, minimumSize: const Size(64, 44)),
      ),
      segmentedButtonTheme: SegmentedButtonThemeData(
        style: SegmentedButton.styleFrom(
          selectedBackgroundColor: s.secondaryContainer,
          selectedForegroundColor: s.onSecondaryContainer,
          minimumSize: const Size(0, 44),
        ),
      ),
      floatingActionButtonTheme: FloatingActionButtonThemeData(
        shape: const RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(20))),
        backgroundColor: s.primaryContainer,
        foregroundColor: s.onPrimaryContainer,
        elevation: 1,
      ),
      popupMenuTheme: PopupMenuThemeData(shape: r16, color: card, surfaceTintColor: Colors.transparent),
      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(color: Colors.black.withValues(alpha: 0.85), borderRadius: BorderRadius.circular(8)),
        textStyle: const TextStyle(color: Colors.white, fontSize: 12),
        waitDuration: const Duration(milliseconds: 500),
      ),
      progressIndicatorTheme: const ProgressIndicatorThemeData(strokeWidth: 3),
      snackBarTheme: const SnackBarThemeData(behavior: SnackBarBehavior.floating, shape: r16),
      textSelectionTheme: _selection(s.brightness),
    );
  }
}

/// Значок платформы устройства.
IconData platformIcon(String platform) => switch (platform) {
      'android' => AppIcons.phone_android,
      'ios' => AppIcons.phone_iphone,
      'macos' => AppIcons.laptop_mac,
      'windows' => AppIcons.desktop_windows,
      'linux' => AppIcons.computer,
      _ => AppIcons.devices_other,
    };
