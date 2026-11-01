# FUNCUT routing

One paragraph, so no request wastes a turn picking the wrong skill:

- **Making or editing a video in this repo — `funcut-director`.** It is the
  only skill that executes the pipeline end to end. Everything else is a
  shelf it routes to, on its own phases.
- `maestro` and `clip-skills` are read-only judgment shelves the director
  pulls from phase by phase (design/motion judgment; cutting craft). They
  are not entry points: a request that arrives at one of them should be
  handed to `funcut-director` rather than answered with an interview.
- `motion` (Geneva) is the motion engine's own manual — read it when
  composing or debugging a comp, a reel, kinetic captions or a pulse.
- `run-concat` is dev-only: building and testing the editor itself.

English or not, small or overnight queue: the entry point is the same.
