# The motion gallery: twenty effects as clips

docs/EFFECTS-GALLERY.md shows every picture package as a still, before and after.
This page shows twenty of them **in motion, at speed**: each clip is the same
reference plate drifting under the package's own shader, drawn frame by frame on
the GPU by `concat-cli clip` (`scripts/shader-clips.sh` records them) - so the
grain dances, the pulse breathes, the embers climb, and a static look is shown
with the knob named in its row swept floor to ceiling. The plate carries no
audio, and neither do the clips: the gallery shows the look, not the mix.

A reviewer can watch every clip below without building the project.

| Effect | Category | Clip | What it does | Needs tuning |
| --- | --- | --- | --- | --- |
| **Film** | Cinematic | [▶ Film](clips/film.mp4) (sweeping `grain`) | A film curve with a real toe and shoulder, warm halation around the lights, and grain that moves. Print stock, not a filter. | grain at 100 is a stock of its own; most plates want 30-50 |
| **VHS** | Retro | [▶ VHS](clips/vhs.mp4) (sweeping `bleed`) | Colour bleeding sideways, scan lines, softness, tape noise and too much saturation, with the slight vertical smear of a worn head. | runs at defaults; bleed past 80 smears small text |
| **Teal & Orange** | Cinematic | [▶ Teal & Orange](clips/teal-orange.mp4) (sweeping `amount`) | Teal in the shadows, orange in the highlights, skin held as skin, a film curve over it. The blockbuster grade, and the most requested look there is. | skin is held by design; push `skin` past 70 on close-ups |
| **Film Grain** | Stylize | [▶ Film Grain](clips/film-grain.mp4) | Dancing grain, the texture of film stock. | runs at defaults; the grain is the whole package, so pick its amount for the platform |
| **Fire Flicker** | Particles | [▶ Fire Flicker](clips/fire-flicker.mp4) | Warm uneven light rolling across the frame, firelight without the fire. | wants a warm-lit plate to flicker; neutral daylight reads as noise |
| **Film Scratch** | Retro & Tape | [▶ Film Scratch](clips/film-scratch.mp4) | Hair-thin scratches that come and go, running up the frame. | runs at defaults; pair with film-grain or it reads as interference |
| **Bloom Pulse** | Light | [▶ Bloom Pulse](clips/bloom-pulse.mp4) | A soft bloom over the highlights that breathes in and out. | wants bright highlights to breathe on; a flat plate shows nothing |
| **Chromatic Pulse** | Color Play | [▶ Chromatic Pulse](clips/chromatic-pulse.mp4) | The channels breathe against each other, red against blue. | runs at defaults; keep clips short or the breathing reads as a codec fault |
| **Earthquake** | Distort | [▶ Earthquake](clips/earthquake.mp4) | Two heavy jolts with the brightness slamming each time. | two jolts with brightness slams: trim the clip to the hit, not around it |
| **Embers Rise** | Particles | [▶ Embers Rise](clips/embers-rise.mp4) | Warm embers floating up, flickering as they climb. | reads best over dark plates; embers vanish into a bright sky |
| **Fireflies** | Particles | [▶ Fireflies](clips/fireflies.mp4) | Green-yellow points wandering and blinking, a summer dusk. | reads best over dark plates; a summer dusk, not an office |
| **Dust Motes** | Particles | [▶ Dust Motes](clips/dust-motes.mp4) | Slow grey specks in a shaft of light. | wants a bright shaft in the plate to hang the specks in |
| **Falling Dust** | Retro | [▶ Falling Dust](clips/falling-dust.mp4) | A warm fade with dust drifting down through it and a light leak in one corner. The CPU shows the fade and a whisper of noise. | the CPU path draws the fade and a whisper of noise; the dust is GPU-only |
| **Fisheye** | Distort | [▶ Fisheye](clips/fisheye.mp4) (sweeping `strength`) | The bulge of a very wide lens. | the bulge eats the edges: leave headroom at the sides of the plate |
| **Anime Speed Focus** | Stylize | [▶ Anime Speed Focus](clips/anime-speed-focus.mp4) | Warm speed lines focused hard on the middle - the anime rush at sunset. | wants a centred subject; off-centre compositions drift out of the focus |
| **Beat Pulse** | Speed & Motion | [▶ Beat Pulse](clips/beat-pulse.mp4) | Brightness that lands on a 120 bpm beat, the edit's own metronome. | lands on a fixed 120 bpm - retime the edit to it or it fights the music |
| **Cyberpunk** | Cinematic | [▶ Cyberpunk](clips/cyberpunk.mp4) (sweeping `intensity`) | Magenta into the shadows, cyan into the light, pushed hard and neon-bright. | reads on night streets and signage; heavy on skin - keep faces lit neutral |
| **Noir** | Cinematic | [▶ Noir](clips/noir.mp4) (sweeping `intensity`) | Desaturated and heavy, with a cold cast into the shadows and the corners dimmed. | the corners dim hard: keep titles inside the middle two-thirds |
| **Dreamy** | Vibe | [▶ Dreamy](clips/dreamy.mp4) (sweeping `intensity`) | A soft, hazy bloom over lifted pastel blacks. | wants pastel sources; it muddies saturated reds |
| **Neon** | Stylised | [▶ Neon](clips/neon.mp4) (sweeping `glow`) | Magenta in the shadows, cyan in the highlights, hard contrast, colour turned up, and the lights themselves blooming. Night streets and signage. | signage and lights bloom while faces stay clean; glow past 80 haloes everything |

The `needs tuning` column is the honest note: what a designer should know before
committing the look, grounded in what the package itself declares - a fixed beat,
a one-shot shape, a look that wants a certain plate. `runs at defaults` means the
package's own defaults are the look, and the clip shows exactly that.

To record them again (a new package, a new still, a new plate drift):

```sh
scripts/shader-clips.sh           # draws every clip on the GPU
python3 scripts/shader-gallery.py # regenerates this page from the manifests
```

One clip, by hand, with a knob swept and the plate held still:

```sh
cargo build -p concat-cli --bin concat-cli
./src/target/debug/concat-cli clip concat.vhs --out docs/clips/vhs.mp4 \
    --sweep bleed --drift 0
```
