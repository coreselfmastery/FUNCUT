# YouTube package: the publishing half of AUTO EDIT

AUTO EDIT already cuts for retention: it finds the hook, the turn, the
payoff, the pattern breaks and the mood. The YouTube package is the same
plan's publishing half. It names what the finished timeline can honestly
publish, from measurements the edit already made.

It is not a second editor and it does not invent a video. It is a
`youtube` object on the plan `auto_edit` writes, before the cut is
applied.

## What the plan carries

| Field | What it is |
| --- | --- |
| `mode` | `short` for a 9:16 delivery, `longform` over 60 seconds, `reel` otherwise. |
| `editMode` | The story skeleton's own mode (`longform` or `reel`). |
| `profile` | The creative profile the narration's words support: `finance`, `education`, `storytelling`, `technology`, `news`, `motivation` or `general`. |
| `profileWhy` | The words that chose the profile. |
| `titles` | Five title directions, each with `title`, `angle`, `promise`, `basis` and `honest`. `promise` is the brief's payoff or thesis, never a new claim. |
| `thumbnails` | Three concepts, each with `frameAt`, `subject`, `text`, `composition` and `reason`. |
| `shorts` | Up to three transcript windows, each with `start`, `end`, `duration`, `hook`, `payoff`, `score`, `why` and `text`. |
| `retentionMap` | One entry per story section: `role`, `start`, `end`, `risk`, `score`, `reason` and the one `fix` the edit vocabulary can apply. |
| `publishingChecklist` | The checks a person still owes before publishing. |

`frameAt` and every Short time are seconds on the finished timeline. The
plan does not render a thumbnail. Sample the frame with `preview_frame`
at `thumbnails[i].frameAt`, then apply the named text and composition.

## How a Short is chosen

The finder slides a window over whole transcript lines. It stops at the
target length when the line ends a thought, and scores what the run
already measured:

- whether the opening line asks a question;
- how strong the delivery reads at the window's start;
- whether the last line lands a payoff (`that's why`, `the result`,
  `which means`);
- how much of the window is actually spoken rather than silence;
- whether the duration sits in the 20-55 second band.

It keeps the top three and drops a candidate whose start is within five
seconds of one already kept. The score and the `why` string are the
audit: a window is not sold as viral, only as measured.

## What the profile changes

The profile is a lens on the same measured edit, not a new pipeline:

- **finance** leans on stakes, numbers and a payoff title;
- **education** leans on the how-to title and a clean middle frame;
- **storytelling** leans on the opening question and the turn;
- **technology** leans on the system and the demonstration frame;
- **news** leans on the report and the latest frame;
- **motivation** leans on the mindset promise;
- **general** is the honest fallback when the narration names no subject.

Pacing, B-roll queries and motion still come from the measured direction,
emotion and voice signals. The profile never overrides a measurement.

## Limits

- Titles are directions, not guarantees. Check the chosen title against
  the video's actual payoff before publishing.
- Thumbnail text is a starting composition. It is not a designed image.
- Short scores rank candidates inside one run. They are not a prediction
  of views, retention or click-through rate.
- Real YouTube Studio retention can only improve this later. No plan
  claims to have seen it.
- The package is additional data on the plan. The canonical plan schema
  accepts it; `plan_validate` does not require it, so a hand-written plan
  remains valid without one.
