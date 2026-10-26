// Оформление в цветах линейки Ogneliss: сланцевый, светлый знак, золото.

import 'package:flutter/material.dart';

class Brand {
  Brand._();

  static const slate = Color(0xFF3A4450);
  static const slateDark = Color(0xFF232A32);
  static const glyph = Color(0xFFD3D8DC);
  static const gold = Color(0xFFCCAA6D);
  static const goldDeep = Color(0xFF8C6A2C);
  static const fox = Color(0xFFF2F4F6);
  static const ok = Color(0xFF3FA66B);
  static const warn = Color(0xFFD08A2E);
}

class AppTheme {
  AppTheme._();

  static ThemeData light() {
    final scheme = ColorScheme.fromSeed(seedColor: Brand.slate, brightness: Brightness.light).copyWith(
      primary: Brand.slate,
      onPrimary: Colors.white,
      secondary: Brand.goldDeep,
      onSecondary: Colors.white,
      tertiary: Brand.gold,
      surface: Colors.white,
    );
    return _base(scheme).copyWith(scaffoldBackgroundColor: const Color(0xFFF3F4F6));
  }

  static ThemeData dark() {
    final scheme = ColorScheme.fromSeed(seedColor: Brand.slate, brightness: Brightness.dark).copyWith(
      primary: Brand.gold,
      onPrimary: const Color(0xFF241B0C),
      secondary: Brand.gold,
      onSecondary: const Color(0xFF241B0C),
      tertiary: Brand.gold,
      surface: const Color(0xFF1E242B),
    );
    return _base(scheme).copyWith(scaffoldBackgroundColor: const Color(0xFF161A1F));
  }

  static ThemeData _base(ColorScheme scheme) {
    return ThemeData(
      colorScheme: scheme,
      useMaterial3: true,
      visualDensity: VisualDensity.standard,
      appBarTheme: AppBarTheme(
        centerTitle: false,
        elevation: 0,
        scrolledUnderElevation: 1,
        backgroundColor: scheme.surface,
        foregroundColor: scheme.onSurface,
      ),
      cardTheme: CardThemeData(
        elevation: 0,
        margin: EdgeInsets.zero,
        color: scheme.surface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(16),
          side: BorderSide(color: scheme.outlineVariant.withValues(alpha: 0.6)),
        ),
      ),
      listTileTheme: const ListTileThemeData(contentPadding: EdgeInsets.symmetric(horizontal: 16)),
      snackBarTheme: const SnackBarThemeData(behavior: SnackBarBehavior.floating),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          minimumSize: const Size(0, 48),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          minimumSize: const Size(0, 48),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        ),
      ),
    );
  }
}

/// Значок платформы устройства.
IconData platformIcon(String platform) => switch (platform) {
      'android' => Icons.phone_android,
      'ios' => Icons.phone_iphone,
      'macos' => Icons.laptop_mac,
      'windows' => Icons.desktop_windows,
      'linux' => Icons.computer,
      _ => Icons.devices_other,
    };
