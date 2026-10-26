#
# Ядро Ogneliss Share на Rust. Cargokit собирает статическую библиотеку
# libogneliss_core.a перед компиляцией модуля; -force_load вкладывает её
# целиком в фреймворк ogneliss_core, откуда Dart берёт функции osh_*.
#
Pod::Spec.new do |s|
  s.name             = 'ogneliss_core'
  s.version          = '0.1.0'
  s.summary          = 'Ogneliss Share core (Rust)'
  s.description      = 'Direct encrypted file transfer core for Ogneliss Share, built from ../rust with Cargokit.'
  s.homepage         = 'https://github.com/oedante/OgnelissShare'
  s.license          = { :type => 'Apache-2.0' }
  s.author           = { 'Ermishin Oleg' => 'oedante@gmail.com' }

  # Пустой Classes/dummy_file.c — чтобы CocoaPods собрал фреймворк.
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  s.dependency 'Flutter'
  s.platform = :ios, '13.0'
  s.swift_version = '5.0'
  # Системные библиотеки, которые нужны ядру (rustc --print native-static-libs):
  # сеть и DNS — Network, SystemConfiguration; TLS — Security.
  s.frameworks = 'Network', 'SystemConfiguration', 'Security', 'CoreFoundation', 'Foundation'
  s.libraries = 'iconv', 'objc'

  s.script_phase = {
    :name => 'Build Rust library',
    # Путь к каталогу rust относительно этого каталога и имя библиотеки.
    :script => 'sh "$PODS_TARGET_SRCROOT/../cargokit/build_pod.sh" ../../rust ogneliss_core',
    :execution_position => :before_compile,
    :input_files => ['${BUILT_PRODUCTS_DIR}/cargokit_phony'],
    # Xcode должен знать, что библиотеку из -force_load создаёт этот шаг.
    :output_files => ["${BUILT_PRODUCTS_DIR}/libogneliss_core.a"],
  }
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    # Во Flutter.framework нет среза i386.
    'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386',
    'OTHER_LDFLAGS' => '-force_load ${BUILT_PRODUCTS_DIR}/libogneliss_core.a',
  }
end
