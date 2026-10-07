# Showreel

A 15-second motion graphics video of Traduko (1920×1080, 60 fps): the mascot,
the three first-run screens, a translation as it is typed, the languages and
the colours.

| File | Sound |
| --- | --- |
| `traduko-showreel.mp4` | Airy and bright: pad and arpeggio |
| `traduko-showreel-sound-b.mp4` | Laid-back swung groove |
| `traduko-showreel-sound-c.mp4` | Upbeat disco-house |

The picture is the same in all three.

## How it is made

Everything is in `source/`.

- **Mascot**: the real one. `mascot.txt` is a script of events (moods, clicks,
  drags, colours) that the `reel` example of `traduko-blob` plays on the
  mascot, writing every frame's outlines:

  ```bash
  cargo run -p traduko-blob --example reel -- showreel/source/mascot.txt mascot.json 15 60
  (printf 'window.MASCOT='; cat mascot.json; printf ';') > showreel/source/mascot.js
  ```

- **Windows**: `cap/` holds captures of the app's own windows, taken with the
  glass turned off so that they are see-through. The scene puts a glass back
  under them, which blurs the background as they move. The typing is a reveal
  of the captured text; the translations are what the app gave for those words.
  The time a translation took is painted out: the captures come from a debug
  build, which is slow.

- **Scene**: `reel.html` draws any moment of the video with `seek(t)`.
  `render.js` saves the 900 frames with Chrome (`npm i puppeteer-core`, then
  `node render.js all`), and `ffmpeg` joins them:

  ```bash
  ffmpeg -framerate 60 -i frames/f%04d.jpg -c:v libx264 -preset slow -crf 19 -pix_fmt yuv420p out.mp4
  ```

- **Background**: `wall_full.jpg`, which is not in the repository: it is the
  desktop picture of the Mac the video was made on. Any picture of 3840 pixels
  or so will do, for example the current one:

  ```bash
  sips -s format jpeg -Z 3840 "<desktop picture>" --out showreel/source/wall_full.jpg
  ```

- **Sound**: `sound.py`, `sound_b.py` and `sound_c.py` each synthesise one
  soundtrack with numpy (`python3 sound_b.py out.wav`), with the effects on the
  times of the picture.
