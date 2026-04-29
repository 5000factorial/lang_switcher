# lang-switcher

`lang-switcher` — это небольшое приложение для Fedora GNOME, которое исправляет текст, набранный в неправильной раскладке.

Главная идея простая:
- нажал `Shift` два раза подряд;
- если текст выделен, приложение пытается исправить выделение;
- если выделения нет, исправляется последнее набранное слово;
- после исправления текущая раскладка GNOME тоже переключается.

Проект рассчитан в первую очередь на:
- Fedora 44
- GNOME 50
- Wayland
- раскладки `us` и `ru`

## Как это работает

Приложение использует несколько способов по очереди:

1. Пытается прочитать выделенный текст через `AT-SPI`.
2. Если объект выделения в Wayland уже исчез, использует `PRIMARY` selection через `wl-paste --primary`.
3. Если выделения нет, исправляет последнее слово из буфера клавиатуры.

Это значит:
- обычные GTK-поля ввода и редакторы работают лучше всего;
- терминалы и нестандартные приложения могут иногда откатываться к сценарию "исправить последнее слово".

## Что понадобится

Нужно, чтобы в системе были:
- `rust` и `cargo` — для сборки
- `wl-clipboard` — для работы с выделением в Wayland
- доступ к `/dev/input` и `/dev/uinput`

Если чего-то нет, установи:

```bash
sudo dnf install rust cargo wl-clipboard
```

## Установка

Ниже самый простой порядок действий.

### 1. Проверь раскладки в GNOME

В настройках GNOME должны быть добавлены обе раскладки:
- `English (US)`
- `Russian`

Приложение ожидает пару `us` и `ru`.

### 2. Открой папку проекта

Перейди в каталог с проектом:

```bash
cd /home/user/Documents/lang_switcher
```

### 3. Собери программу

```bash
cargo build --release
```

После сборки готовый бинарь будет здесь:

```bash
./target/release/lang-switcher
```

### 4. Включи `uinput`

```bash
sudo modprobe uinput
```

### 5. Добавь пользователя в группу `input`

Это нужно, чтобы приложение могло читать события клавиатуры.

```bash
getent group input || sudo groupadd input
sudo usermod -aG input $USER
```

### 6. Установи udev rules

```bash
./target/release/lang-switcher install --print-udev-rules | sudo tee /etc/udev/rules.d/99-lang-switcher.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### 7. Выйди из сессии GNOME и зайди снова

Это важный шаг.

После добавления пользователя в группу `input` права применятся только после нового входа в систему.

Проще всего:
- выйти из текущей GNOME-сессии;
- войти снова.

Если удобнее, можно просто перезагрузить компьютер.

### 8. Установи приложение как пользовательский сервис

Снова зайди в папку проекта и выполни:

```bash
cd /home/user/Documents/lang_switcher
./target/release/lang-switcher install
systemctl --user daemon-reload
systemctl --user enable --now lang-switcher.service
```

После этого приложение будет запускаться автоматически в твоей пользовательской сессии.

### 9. Проверь, что все запустилось

```bash
systemctl --user status lang-switcher.service
~/.local/bin/lang-switcher doctor
~/.local/bin/lang-switcher status
```

Если сервис работает, статус должен быть `active (running)`.

## Быстрая проверка

### Проверка последнего слова

Если активна английская раскладка:

1. Набери `ghbdtn`
2. Нажми `Shift` два раза быстро
3. Должно стать `привет`

Если активна русская раскладка:

1. Набери `руддщ`
2. Нажми `Shift` два раза быстро
3. Должно стать `hello`

### Проверка выделенного текста

Лучше всего проверять в `gedit` или другом обычном GTK-редакторе:

1. Напечатай текст
2. Выдели его мышкой
3. Нажми `Shift` два раза быстро

Текст должен поменяться на вариант в другой раскладке.

## Настройка

Конфиг хранится здесь:

```bash
~/.config/lang-switcher/config.toml
```

Посмотреть путь:

```bash
~/.local/bin/lang-switcher config path
```

Посмотреть текущее значение:

```bash
~/.local/bin/lang-switcher config get double_shift_timeout_ms
```

Изменить таймаут двойного `Shift`:

```bash
~/.local/bin/lang-switcher config set double_shift_timeout_ms 320
```

Отключить обработку выделенного текста и оставить только исправление последнего слова:

```bash
~/.local/bin/lang-switcher config set enable_selected_text false
systemctl --user restart lang-switcher.service
```

Вернуть обратно:

```bash
~/.local/bin/lang-switcher config set enable_selected_text true
systemctl --user restart lang-switcher.service
```

Если раньше включался подробный лог, вернуть обычный уровень:

```bash
~/.local/bin/lang-switcher config set log_level info
systemctl --user restart lang-switcher.service
```

## Как выключить переключение

### Временно выключить

Если хочешь просто остановить приложение до следующего ручного запуска:

```bash
systemctl --user stop lang-switcher.service
```

Включить обратно:

```bash
systemctl --user start lang-switcher.service
```

### Полностью выключить автозапуск

Если хочешь, чтобы приложение не запускалось вместе с сессией:

```bash
systemctl --user disable --now lang-switcher.service
```

Включить обратно:

```bash
systemctl --user enable --now lang-switcher.service
```

## Полезные команды

Запустить вручную в текущем терминале:

```bash
./target/release/lang-switcher run
```

Показать состояние:

```bash
~/.local/bin/lang-switcher status
```

Показать диагностику:

```bash
~/.local/bin/lang-switcher doctor
```

Посмотреть последние логи:

```bash
journalctl --user -u lang-switcher.service -n 50 --no-pager
```

Следить за логами в реальном времени:

```bash
journalctl --user -u lang-switcher.service -f
```

## Если что-то не работает

Проверь по порядку:

1. Сервис запущен:

```bash
systemctl --user status lang-switcher.service
```

2. `doctor` показывает доступ к устройствам:

```bash
~/.local/bin/lang-switcher doctor
```

Там важно видеть:
- `input_access: present`
- `uinput_access: present`

3. В GNOME действительно активны `us` и `ru`.

4. Ты уже выходил из GNOME-сессии после добавления себя в группу `input`.

5. В системе установлен `wl-clipboard`.

Если после этого все еще есть проблема, полезнее всего сразу посмотреть лог:

```bash
journalctl --user -u lang-switcher.service -n 100 --no-pager
```

## Ограничения

- Приложение ориентировано на GNOME Wayland, а не на X11.
- Выделенный текст работает по best-effort схеме: `AT-SPI`, затем `PRIMARY` selection.
- Некоторые терминалы, нестандартные редакторы и сложные приложения могут не дать стабильный доступ к выделению.
- В таких случаях обычно остается рабочим сценарий с последним словом.
