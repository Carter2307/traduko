# Traduko landing page

A React + TypeScript landing page built with Vite and Tailwind CSS. The page introduces Traduko's floating desktop companion and its local translation features.

The header contains only the Traduko brand and an inert download button at every screen size. The hero headline is “Translate anytime with a little companion.”

## Development

Requires Node.js 22.13+ in the 22.x release line, or Node.js 24 and later. Validated with Node.js 22.19.

Run these commands from this `landing` directory:

```sh
npm install
npm run dev
```

Vite prints the local preview URL. To check and build the page:

```sh
npm run lint
npm run build
npm run preview
```

`build` runs TypeScript checking before creating the production files in `dist/`. `preview` serves that build locally.

## Worktree

This page is developed on the `codex/traduko-landing` branch in a separate Git worktree.

The original macOS application is under `crates/app`; the website is contained in `landing`.

## Native app images

Product screenshots are real Retina captures of the running Traduko macOS app. Original JPEG captures and cropped PNGs are stored in `public/screenshots/` and retain their native capture resolution. The interface shown in the screenshots is the app's own interface.

The page uses the light native translation panel capture, `traduko-light.jpg` (952 × 1240), and light onboarding screens. The original dark capture is retained as a source asset. The onboarding captures are 1024 × 1424:

| File | Native screen |
| --- | --- |
| `01-welcome.jpg` | Bonjour, I'm Traduko |
| `02-models.jpg` | Download a model |
| `03-login.jpg` | Keep me around |

Files with the `-cropped` suffix are derivatives with window margins removed for page composition. Full-resolution originals are retained as source captures.

The feature section imports its light translation screenshot from `src/assets/traduko-light-cropped.png`, allowing Vite to validate the file and emit a versioned asset URL for production.

The hero uses the supplied `Traduko-sound-2.mp4` in place of the onboarding image row. The 15-second, 1920 × 1080, 60 fps video retains its original H.264 video and AAC audio, with the MP4 metadata at the front for progressive playback. Vite bundles it from `src/assets/traduko-sound-2.mp4` with a poster extracted from this version of the showreel. It starts muted and plays inline with native video controls always hidden. A bottom-left icon button toggles sound and starts playback when unmuting a paused video. Reduced motion displays a still poster instead of automatic playback.

The feature section presents the light translation and welcome screenshots inside two rounded cards, followed by three smaller benefit cards. The benefit cards enter in sequence with 100 ms between each reveal. Cards stack on mobile, and the page background remains white. The separate onboarding gallery, How it works section, hero companion link, and closing-card caption have been removed; all original captures are retained as source assets.

Capture the onboarding from a separate copied app and support profile so the user's regular preferences stay intact. The app's development aids include `TRADUKO_SUPPORT_DIR`, `TRADUKO_PANEL=1`, `TRADUKO_APPEARANCE=light|dark`, `TRADUKO_TEXT`, and `TRADUKO_QUIT_AFTER`. The running app reports native window numbers and rectangles for window captures.

The native first run contains Welcome and Models, followed by Login only when permission is pending. `TRADUKO_LOGIN_ITEM=ask` presents that pending state for a capture; `allowed` omits the Login step, and `approval` presents the System Settings approval state. This variable changes the reported permission, so capture the screen without pressing its Allow button. A separate Languages screen is available later through **More languages…** in either language menu.

## Download and product copy

Every **Download for Mac** button is intentionally inert. There is no attached binary, download navigation, checkout, or release endpoint. The page does not claim App Store availability or a published release.

Translation runs locally after the initial model downloads. French and English are the starting languages, with additional language models available from the app. American and British English are selectable. Product copy is based on the native application's README and implementation.

## Typography and motion

Typography uses self-hosted Geist through `@fontsource-variable/geist`. Vite bundles the WOFF2 files with the site, covering the page's regular through extra-bold weights without synthetic bold. Its SIL Open Font License 1.1 and copyright notice are included in `public/fonts/LICENSE-Geist.txt`; retain that file when distributing the site.

The page respects `prefers-reduced-motion`: entrance and decorative animations are disabled, content stays visible, and in-page scrolling does not animate. The sound toggle stays usable without animation.

The hero's interactive mascot reuses the native app's speech-bubble outline and white pill eyes. Its eyes follow the pointer with bounded, smoothed movement. The mascot floats and blinks at rest, and a click or keyboard activation triggers a short greeting. Reduced motion keeps the mascot still.

## Hero shader

The hero, closing card, and footer share a real WebGPU pipeline and a procedural WGSL fragment shader for soft, dappled shapes in Traduko's primary orange (`#F45A1C`), warm peach, and cream highlights. The shapes flow downward from the top with organic side-to-side sway, while pointer movement adds a small, smoothed parallax. The footer mirrors the canvas vertically so its shapes rise from the bottom and applies a linear CSS mask to fade progressively upward; it leaves extra space below the footer links. Rendering is capped at 30 frames per second and a device pixel ratio of 1.5; each instance pauses when it leaves the viewport or the document is hidden. Reduced motion renders a still frame. Unsupported browsers and GPU failures use a static CSS wash in the same warm palette.

The shader wrapper exposes `data-renderer`, `data-motion`, and a short `data-error` reason for development inspection. The renderer was verified as `webgpu` in the local preview. The page uses no tracking or third-party runtime requests.
