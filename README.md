# lang-switcher

`lang-switcher` is a small Fedora GNOME app that fixes text typed with the wrong keyboard layout.

The basic idea is simple:
- press `Shift` twice quickly;
- if some text is selected, the app tries to fix the selection;
- if nothing is selected, it fixes the last typed word;
- after the fix, the current GNOME layout is switched too.

This project is primarily built for:
- Fedora 44
- GNOME 50
- Wayland
- `us` and `ru` layouts

## How It Works

The app tries several methods in order:

1. It tries to read the selected text through `AT-SPI`.
2. If the selected object has already disappeared under Wayland, it falls back to `PRIMARY` selection via `wl-paste --primary`.
3. If there is no selection, it fixes the last word from the keyboard buffer.

What this means in practice:
- regular GTK input fields and editors work best;
- terminals and unusual apps may sometimes fall back to the "fix the last word" behavior.

## Requirements

You need:
- `rust` and `cargo` to build the project
- `wl-clipboard` for selected text handling on Wayland
- access to `/dev/input` and `/dev/uinput`

If something is missing, install it with:

```bash
sudo dnf install rust cargo wl-clipboard
```

## Installation

This is the simplest setup flow.

### 1. Check your GNOME layouts

Both layouts must be added in GNOME Settings:
- `English (US)`
- `Russian`

The app expects the `us` / `ru` pair.

### 2. Open the project directory

```bash
cd /home/user/Documents/lang_switcher
```

### 3. Build the app

```bash
cargo build --release
```

After the build, the binary will be here:

```bash
./target/release/lang-switcher
```

### 4. Enable `uinput`

```bash
sudo modprobe uinput
```

### 5. Add your user to the `input` group

This is required so the app can read keyboard events.

```bash
getent group input || sudo groupadd input
sudo usermod -aG input $USER
```

### 6. Install the udev rules

```bash
./target/release/lang-switcher install --print-udev-rules | sudo tee /etc/udev/rules.d/99-lang-switcher.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### 7. Log out of GNOME and log back in

This step is important.

After adding your user to the `input` group, the new permissions only take effect after a new login session.

The easiest option:
- log out of your current GNOME session;
- log back in.

You can also just reboot the machine.

### 8. Install the app as a user service

Go back to the project directory and run:

```bash
cd /home/user/Documents/lang_switcher
./target/release/lang-switcher install
```

The installer now tries to do this automatically:
- install the binary to `~/.local/bin`
- create the `systemd --user` service
- enable autostart
- start the service right away

If automatic enabling fails for some reason, run:

```bash
systemctl --user daemon-reload
systemctl --user enable --now lang-switcher.service
```

After that, the app will start automatically every time you log into GNOME.

### 9. Verify that it started

```bash
systemctl --user status lang-switcher.service
~/.local/bin/lang-switcher doctor
~/.local/bin/lang-switcher status
```

If everything is fine, the service status should be `active (running)`.

The app is designed to run as a background user service. If it ever exits unexpectedly, `systemd` will try to start it again automatically.

## Quick Test

### Last word test

If the active layout is English:

1. Type `ghbdtn`
2. Press `Shift` twice quickly
3. It should become `привет`

If the active layout is Russian:

1. Type `руддщ`
2. Press `Shift` twice quickly
3. It should become `hello`

### Selected text test

It is best to test this in `gedit` or another normal GTK editor:

1. Type some text
2. Select it with the mouse
3. Press `Shift` twice quickly

The text should be converted to the other layout.

## Configuration

The config file is stored here:

```bash
~/.config/lang-switcher/config.toml
```

Show the config path:

```bash
~/.local/bin/lang-switcher config path
```

Show the current value:

```bash
~/.local/bin/lang-switcher config get double_shift_timeout_ms
```

Change the double-`Shift` timeout:

```bash
~/.local/bin/lang-switcher config set double_shift_timeout_ms 320
```

Disable selected text handling and keep only last-word fixing:

```bash
~/.local/bin/lang-switcher config set enable_selected_text false
systemctl --user restart lang-switcher.service
```

Enable it again:

```bash
~/.local/bin/lang-switcher config set enable_selected_text true
systemctl --user restart lang-switcher.service
```

If debug logging was enabled earlier, switch back to normal logs:

```bash
~/.local/bin/lang-switcher config set log_level info
systemctl --user restart lang-switcher.service
```

## How To Disable Switching

### Temporarily stop it

If you just want to stop the app until the next manual start:

```bash
systemctl --user stop lang-switcher.service
```

Start it again:

```bash
systemctl --user start lang-switcher.service
```

### Disable autostart completely

If you do not want it to start together with your session:

```bash
systemctl --user disable --now lang-switcher.service
```

Enable it again:

```bash
systemctl --user enable --now lang-switcher.service
```

## Useful Commands

Run manually in the current terminal:

```bash
./target/release/lang-switcher run
```

Show current status:

```bash
~/.local/bin/lang-switcher status
```

Show diagnostics:

```bash
~/.local/bin/lang-switcher doctor
```

Show the latest logs:

```bash
journalctl --user -u lang-switcher.service -n 50 --no-pager
```

Follow logs in real time:

```bash
journalctl --user -u lang-switcher.service -f
```

## If Something Does Not Work

Check these items in order:

1. The service is running:

```bash
systemctl --user status lang-switcher.service
```

2. `doctor` shows device access:

```bash
~/.local/bin/lang-switcher doctor
```

These lines are important:
- `input_access: present`
- `uinput_access: present`

3. `us` and `ru` are actually enabled in GNOME.

4. You already logged out of GNOME after adding yourself to the `input` group.

5. `wl-clipboard` is installed on the system.

If it still does not work after that, the most useful next step is to inspect the log:

```bash
journalctl --user -u lang-switcher.service -n 100 --no-pager
```

## Limitations

- The app targets GNOME Wayland, not X11.
- Selected text works on a best-effort basis: `AT-SPI` first, then `PRIMARY` selection.
- Some terminals, unusual editors, and more complex apps may not expose stable access to the selection.
- In those cases, the last-word scenario usually still works.
