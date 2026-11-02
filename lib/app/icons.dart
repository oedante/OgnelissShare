// ignore_for_file: constant_identifier_names
// Значки — Material Symbols Rounded, как в Ogneliss Mail (lib/core/theme/
// app_icons.dart там): один стиль в линейке и одно место для замены.
// Толщина линий — [weight] (тема приложения задаёт её всем значкам).

import 'package:flutter/widgets.dart';
import 'package:material_symbols_icons/symbols.dart';

abstract final class AppIcons {
  /// Тонкие линии «мягкого» стиля Mail.
  static const double weight = 300;

  static const IconData add_link = Symbols.add_link_rounded;
  static const IconData block = Symbols.block_rounded;
  static const IconData check_circle = Symbols.check_circle_rounded;
  static const IconData clear = Symbols.clear_rounded;
  static const IconData close = Symbols.close_rounded;
  static const IconData code = Symbols.code_rounded;
  static const IconData computer = Symbols.computer_rounded;
  static const IconData content_copy = Symbols.content_copy_rounded;
  static const IconData content_paste = Symbols.content_paste_rounded;
  static const IconData delete = Symbols.delete_rounded;
  static const IconData desktop_windows = Symbols.desktop_windows_rounded;
  static const IconData devices = Symbols.devices_rounded;
  static const IconData devices_other = Symbols.devices_other_rounded;
  static const IconData edit = Symbols.edit_rounded;
  static const IconData error = Symbols.error_rounded;
  static const IconData folder = Symbols.folder_rounded;
  static const IconData folder_open = Symbols.folder_open_rounded;
  static const IconData group = Symbols.group_rounded;
  static const IconData groups = Symbols.groups_rounded;
  static const IconData hub = Symbols.hub_rounded;
  static const IconData insert_drive_file = Symbols.insert_drive_file_rounded;
  static const IconData laptop_mac = Symbols.laptop_mac_rounded;
  static const IconData link = Symbols.link_rounded;
  static const IconData link_off = Symbols.link_off_rounded;
  static const IconData mail = Symbols.mail_rounded;
  static const IconData north_east = Symbols.north_east_rounded;
  static const IconData notes = Symbols.notes_rounded;
  static const IconData open_in_new = Symbols.open_in_new_rounded;
  static const IconData phone_android = Symbols.phone_android_rounded;
  static const IconData phone_iphone = Symbols.phone_iphone_rounded;
  static const IconData photo_library = Symbols.photo_library_rounded;
  static const IconData public = Symbols.public_rounded;
  static const IconData qr_code_2 = Symbols.qr_code_2_rounded;
  static const IconData qr_code_scanner = Symbols.qr_code_scanner_rounded;
  static const IconData receipt_long = Symbols.receipt_long_rounded;
  static const IconData refresh = Symbols.refresh_rounded;
  static const IconData send = Symbols.send_rounded;
  static const IconData settings = Symbols.settings_rounded;
  static const IconData south_west = Symbols.south_west_rounded;
  static const IconData swap_horiz = Symbols.swap_horiz_rounded;
  static const IconData timer_off = Symbols.timer_off_rounded;
  static const IconData wifi_off = Symbols.wifi_off_rounded;
}
