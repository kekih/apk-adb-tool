# apk-adb-tool

Красивая тёмная GUI-утилита на Rust (egui): ищет установленные приложения на Android-телефоне по названию через ADB, показывает список, скачивает APK и переустанавливает.

![Dark theme GUI](https://img.shields.io/badge/theme-dark-111118)

## Возможности

- Тёмная тема, современный интерфейс
- Поиск пакетов по части имени (регистронезависимый)
- Фильтр только сторонних приложений (`-3`)
- Выбор устройства (если подключено несколько)
- Скачивание APK (`adb pull`)
- Переустановка (`adb install -r`)
- Двойной клик по пакету = сразу скачать
- Лог действий внизу
- Выбор папки для сохранения APK

## Требования

- [Android Platform Tools](https://developer.android.com/tools/releases/platform-tools) (`adb` в PATH)
- Телефон с USB-отладкой
- Для сборки: Rust 1.75+

## Скачать готовый `.exe`

Смотри [Releases](https://github.com/kekih/apk-adb-tool/releases) — Windows `.exe`, Linux и macOS бинарники собираются через GitHub Actions.

Или запусти workflow вручную: **Actions → Release → Run workflow**.

## Сборка

```bash
git clone https://github.com/kekih/apk-adb-tool.git
cd apk-adb-tool
cargo run --release
```

## Использование

1. Подключи телефон, разреши отладку.
2. Введи часть имени пакета (например `telegram`, `youtube`).
3. Нажми **Search** или Enter.
4. Выбери приложение в списке.
5. **Download APK** — сохранит файл.
6. **Install / Reinstall** — поставит обратно на телефон.

## Примечания

- Ищет только уже установленные приложения (не в интернете).
- Системные APK иногда требуют root для pull.
- Рекомендуется включать «Only third-party apps».

## Лицензия

MIT
