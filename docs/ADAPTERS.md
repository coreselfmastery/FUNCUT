# Adapters: motion made elsewhere, read here

Two formats carry motion between tools, and neither of them is ours: Lottie (the
bodymovin JSON After Effects exports) is what a designer hands over, and GSAP (the
timeline a web build animates with) is what a motion developer already knows how to
write. The `geneva-adapter` crate (`src/crates/geneva-adapter`) is the honest bridge
between them and the comp format the engines already bake — and *honest* is the
whole design: **nothing converts silently**. A feature the comp vocabulary has no
answer for is skipped with a note that says what and where; an approximation is
applied with a note that says what was done instead. A conversion with no notes
lost nothing the source document said.

The adapters are not a player. A `.lottie` file already plays as an asset through
the Lottie engine; the adapter is for when the motion should become *document* —
editable, lintable, fillable by a slot, baked by whichever engine the project
prefers.

## The command line

```text
geneva-adapt lottie animation.json -o comp.json
geneva-adapt gsap comp.json timeline.json -o comp.json
```

The comp goes to `-o` (or stdout), the report — the notes, the count of skips, the
comp's shape — to stderr as one JSON line. The exit code is non-zero when the
document is not the format the verb reads.

## The Lottie adapter

| Lottie | Comp |
| --- | --- |
| the root (`fr`, `ip`, `op`, `w`, `h`) | the comp's `fps`, `duration`, `width`, `height` |
| a solid layer (`ty: 1`) | a `rect` node, `sc` its fill |
| an image layer (`ty: 2`) | an `image` node, the file registered as an asset |
| a shape layer (`ty: 4`) | a `group` node carrying the layer's `ks`, its shapes as children |
| a shape group (`gr`) | a `group` node carrying the group's own `tr` |
| `el` / `rc` / `sh` | `ellipse` / `rect` / `path` nodes |
| `fl` / `st` | the shapes' fill and stroke (opacity folded into the colour) |
| a precomp layer (`ty: 0`) | the precomp's layers, nested under a group, ids prefixed |
| a null layer (`ty: 3`) | a `group` node that draws nothing |
| `ks` — position, anchor, scale, rotation, opacity | the node's transform, its changes as tracks |
| keyframes, with `o`/`i` easing and `h` holds | track keys with bezier curves and `steps()` |
| `parent` (layer parenting) | the node's `parent`, by the node the layer became |
| `ip`/`op` (in and out points) | the node's `start` and `duration`, in seconds |

Two order rules, both the ones the reference players render by: the first layer of
a `layers` array is the front (After Effects writes its timeline top-first), and
within a shape list later items paint on top (a `fl` paints the geometry
accumulated before it in the same group — the `[el, fl, tr]` pattern every file
writes).

What does not convert — a text layer, a matte, a mask, a trim path, a gradient
paint, a repeater, an effect, a 3D layer, a hidden layer — is skipped with a note
naming the layer and the feature. What converts nearly:

- a non-uniform scale takes the mean of the two axes (the comp's scale is one
  number), and says so;
- a curved motion path (`to`/`ti` tangents) moves in a straight line between keys;
- an animated shape size, roundness, paint colour or stroke width is held at its
  first value;
- a group paint over several shapes is split per shape (two half-opaque shapes
  over each other darken more than one union would);
- a group's own anchor is approximated by its place (a group has no box to be a
  fraction of);
- paths are re-based to their own bounding box, the box placed where the source
  drew it — and are drawn by the native engine only, as any hand-written path is.

Precomps nest up to eight deep, with cycle detection by that same limit; a precomp
with its own frame rate or start offset is read on the comp's clock and says so.
A document whose every layer is skipped is refused with the comp's own words — a
comp with no nodes draws nothing — because an empty comp is not a conversion.

## The GSAP adapter

The document is an array of tweens, the shape a `gsap.to()` call would leave
behind:

```json
[
  { "targets": ["headline", "sub"],
    "from": { "opacity": 0, "y": 24 },
    "to":   { "opacity": 1, "y": 0 },
    "duration": 0.6, "ease": "power2.out", "stagger": 0.08 },
  { "targets": "mark", "to": { "rotation": 8 },
    "duration": 0.4, "delay": 0.2, "repeat": 1, "yoyo": true }
]
```

A tween carries `targets` (one node id or an array), `from` and `to` objects over
`x`, `y`, `scale`, `rotation`, `opacity` and `blur`, `duration` and `delay` in
seconds, `stagger` between array targets, `repeat` (a count; `-1` is refused
rather than guessed at), `yoyo`, and an `ease` by name. The named eases are the
families the comp's curves answer to — `power1..4`, `sine`, `expo`, `circ` and
`back`, each as `.in`, `.out` and `.inOut`, plus `none`, `linear`, `steps(n)` and
`elastic.out` (read as the library's pop spring, and says so).

The semantics are the comp's, stated plainly: a tween's window is its `delay` to
`delay + duration × (repeat + 1)`, keyed as fractions of the target node's own
span; `x` and `y` are one position track, written together the way a position
moves; a tween on a property replaces the track that property carried; and the
value the tween starts from is the `from` value when there is one, the node's
static value when there is not. A tween whose window falls outside a node's span
is skipped with a note naming the node, and cycles that outrun the node are
dropped and said.

## The library

Both adapters are plain functions for the code that wants them without a process:

```rust
use geneva_adapter::{lottie, gsap};

let import = lottie::import(&document)?;          // Import { comp, notes }
let (comp, notes) = gsap::apply(&comp, &timeline)?; // the timeline, written on
```

The output of either validates by the same rules a hand-written comp does, and
the crate's tests hold it to the library's own lint on every fixture: the
adapters do not ship a document the engines would refuse.
