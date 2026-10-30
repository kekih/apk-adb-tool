# apk-adb-tool

Тёмная GUI-утилита на Rust (egui): ищет установленные приложения на Android через ADB, скачивает APK и переустанавливает.

## Возможности

- Тёмная тема
- Поиск пакетов по части имени
- Фильтр сторонних приложений
- Выбор устройства
- Download APK / Install
- Двойной клик = сразу скачать
- Лог действий

## Скачать бинарник

После каждого push в `main` GitHub Actions сам собирает:

**Actions → последний Build → Artifacts**

Там будут:
- `apk-adb-tool.exe` (Windows)
- `apk-adb-tool-linux`
- `apk-adb-tool-macos`

При создании тега `v*` дополнительно появляется [Release](https://github.com/kekih/apk-adb-tool/releases).

## Требования

- [Platform Tools](https://developer.android.com/tools/releases/platform-tools) (`adb` в PATH)
- USB-отладка на телефоне

## Сборка локально

```bash
git clone https://github.com/kekih/apk-adb-tool.git
cd apk-adb-tool
cargo run --release
```

На Linux может понадобиться:
```bash
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libgtk-3-dev
```

## Использование

1. Подключи телефон
2. Введи часть package name (`telegram`, `youtube`…)
3. Search → выбери пакет
4. **Download APK** → **Install / Reinstall**

## Лицензия

MIT
