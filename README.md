# apk-adb-tool

Простая CLI-утилита на Rust: ищет установленные приложения на Android-телефоне по названию/части package name через ADB, показывает список, позволяет выбрать, **скачивает (pull) APK** на компьютер и (по желанию) переустанавливает его обратно через `adb install -r`.

## Требования

- Установленный [Android Platform Tools](https://developer.android.com/tools/releases/platform-tools) (`adb` в PATH)
- Телефон с включённой отладкой по USB и разрешением на этом компьютере
- Rust 1.70+ (для сборки из исходников)

## Установка

### Скачать готовый бинарник (рекомендуется)

Смотри [Releases](https://github.com/kekih/apk-adb-tool/releases) — там будут `.exe` для Windows и бинарники для Linux/macOS (собираются автоматически через GitHub Actions).

### Собрать самому

```bash
git clone https://github.com/kekih/apk-adb-tool.git
cd apk-adb-tool
cargo build --release
# бинарник: target/release/apk-adb-tool  (или .exe на Windows)
```

## Использование

```bash
# Интерактивный поиск
apk-adb-tool

# Сразу с поиском
apk-adb-tool -s youtube
# или
apk-adb-tool --search com.google

# Только сторонние приложения
apk-adb-tool -3 --search telegram

# Только скачать APK, не устанавливать
apk-adb-tool --pull-only --search whatsapp

# Указать устройство (если несколько)
apk-adb-tool -s SERIALNUMBER --search settings

# Куда сохранять APK
apk-adb-tool -o ./apks --search chrome
```

### Что происходит

1. Проверяет наличие `adb` и подключённого устройства
2. Ищет packages, содержащие строку поиска (без учёта регистра)
3. Показывает интерактивный список (стрелки + Enter)
4. Берёт путь к APK через `pm path`
5. Делает `adb pull` → сохраняет `package_name.apk`
6. Спрашивает подтверждение и делает `adb install -r`

## Примеры вывода

```
✓ ADB found: /usr/bin/adb
✓ Device(s) online: 1

→ Found 3 matching package(s) for 'telegram':
? Select package to download APK + install
> org.telegram.messenger
  org.telegram.messenger.web
  ...
```

## Примечания

- Для системных приложений pull может требовать root (зависит от устройства и Android-версии).
- Флаг `-3` / `--third-party` сильно ускоряет и показывает только user-apps.
- Инструмент **не** ищет APK в интернете — только уже установленные на телефоне.

## Лицензия

MIT
