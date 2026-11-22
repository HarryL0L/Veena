Veena Music for Linux (x86_64)

Install:    ./install.sh      (for your user; no root needed)
Run:        from your app menu, or `veena-desktop`
Terminal:   `veena --help` for the terminal player
Remove:     ./uninstall.sh

Works on X11 and Wayland. Needs these common libraries, already on most
desktops: ALSA (libasound2), fontconfig, xkbcommon and OpenGL.
On Debian/Ubuntu, if one is missing:
  sudo apt install libasound2t64 libfontconfig1 libxkbcommon0 libgl1

Bricks (the parts that fetch music) update on their own, signed, once a day.
