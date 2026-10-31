"""emotion.py — the emotional read of the voiceover.

The director pipeline had eyes (`sense.py`) and hands (the MCP tools), but
the edit was driven by structure alone: sections, pacing floors, variety
laws. This module adds the missing sense — what each line of the voiceover
FEELS like, measured from the words themselves, and what that feeling
demands of every layer downstream:

    line of narration -> emotion, intensity, act, hormone
                      -> easing curve, colour temperature, sfx register,
                         BGM duck depth, TTS delivery, pacing hint,
                         stock-query vocabulary
    all lines         -> the arc (shape, variety, score, warnings)

Two engines, both honest about what they are:

- `lexicon` (always available, zero dependencies): a curated stem lexicon
  over the seven edit labels, with negation windows, amplifiers,
  hedges, caps and punctuation intensity. English-only - confidence 0.55.
  A bag of words is a real signal, not a model.
- `model` (free tier, when `FUNCUT_EMOTION_API_KEY` / `HF_TOKEN` is set):
  the `tabularisai/multilingual-emotion-classification` text classifier
  (XLM-RoBERTa, 23 languages, 11 labels folded onto the edit's seven) via
  the HuggingFace Inference API - milliseconds per line, no local install.
  When it answers, it carries 75% of the weight and the lexicon 25%;
  confidence 0.85.

The "hormone" field is a creative shorthand (adrenaline, cortisol,
dopamine, serotonin, oxytocin, endorphin) for the state a beat puts the
viewer in. It is a mnemonic for pacing and sound decisions, not physiology,
and the docs say so.

Commands (same `--spec` contract as sense.py, one JSON object on stdout):

    extract   lines -> per-line emotion report + the arc
    brand     validate a brand guide and apply it to lines/segments
    gate      placement gate: does a stock candidate match emotion/act/brand?
    verify    does the exported render's transcript match the edit?
    template  a ready-to-edit brand guide
    selftest  deterministic assertions, no fixtures, no network

Run directly (`python3 emotion.py extract --spec '{...}'`) or through
sense.py's `emotion` command, which is how the MCP tool reaches it.
"""

import json
import os
import re
import sys
import urllib.error
import urllib.request

# ---------------------------------------------------------------------------
# The seven labels the edit speaks: every layer downstream (the PROFILE,
# the plan schema, the gate's registers) is written in these, so the hosted
# model's eleven labels are folded onto them before anything else sees a
# score.
# ---------------------------------------------------------------------------

EMOTIONS = ("anger", "disgust", "fear", "joy", "neutral", "sadness", "surprise")

ACTS = ("hook", "setup", "tension", "turn", "reveal", "proof", "cta")

HORMONES = ("adrenaline", "cortisol", "dopamine", "endorphin", "oxytocin", "serotonin")

# The multilingual model: XLM-RoBERTa fine-tuned across 23 languages
# (English, Chinese, Spanish, Hindi, Arabic, Bengali, Portuguese, Russian,
# Japanese, German, Indonesian, Tamil, Vietnamese, Korean, French, Turkish,
# Italian, Polish, Ukrainian, Urdu, Dutch, Punjabi, Swahili), 11 labels.
# It reads the narration in whatever language it was actually written in -
# an English-only model was the fork's blind spot.
DEFAULT_MODEL = "tabularisai/multilingual-emotion-classification"

# The label set the hosted model emits, and where each folds onto the
# edit's seven. The four extras are shades, not new registers: contempt
# rides the disgust grade, frustration the anger cut, gratitude and love
# the joy lift (love at calm arousal lands on oxytocin via _hormone_for,
# which is the state the edit wants anyway).
MODEL_LABELS = (
    "anger", "contempt", "disgust", "fear", "frustration",
    "gratitude", "joy", "love", "neutral", "sadness", "surprise",
)
MODEL_LABEL_MAP = {
    "anger": "anger",
    "contempt": "disgust",
    "disgust": "disgust",
    "fear": "fear",
    "frustration": "anger",
    "gratitude": "joy",
    "joy": "joy",
    "love": "joy",
    "neutral": "neutral",
    "sadness": "sadness",
    "surprise": "surprise",
}

# The hosted call batches lines; past this many one POST grows heavy for
# the free tier, so the extract walks the transcript in chunks.
MODEL_BATCH = 64

# Free-tier endpoints for the model, in try order. The router is the current
# form; the legacy api-inference host still answers for text-classification
# and costs one retry when a region has not migrated.
HF_ENDPOINTS = (
    "https://router.huggingface.co/hf-inference/models/{model}",
    "https://api-inference.huggingface.co/models/{model}",
)

# ---------------------------------------------------------------------------
# The lexicon. Stems, not words: "furi" covers fury/furious/furiously, and a
# stem only matches a token of at least four characters so short stems do not
# eat unrelated words. Phrases are matched against the normalised token
# stream with t-contractions merged ("wont believe" - "won't" becomes one
# token, so the verb "won" is never mistaken for a negation).
# ---------------------------------------------------------------------------

_LEXICON = {
    "anger": """
        anger angry angers rage raging rages fury furious furiously outrag
        outraged outrageous mad madder irate livid hostile hate hated hating
        hateful despise despised resentful resentment bitter bitterly spite
        vengeful revenge wrath seething indignant tantrum pissed annoy
        annoying annoyed irritating irritated irritate infuriating infuriated
        aggravating aggravated betray betrayed treacher treachery corrupt
        corruptd greed greedy unjust unfair rigged scam fraud liar lying cheat
        cheating stolen steal
    """,
    "disgust": """
        disgust disgusted disgusting disgusts gross grossed revolting revolts
        repulsive nauseating nauseated nauseous sickening sickened icky yuck
        eww grimy filthy foul putrid vile crude distasteful repugnant revulsion
        revolting spoil spoilt rotted rotten decay maggot slimy sludge squalid
        squalor vomit puke retch stench stinky smelly unsanitary unhygienic
    """,
    "fear": """
        fear fears feared afraid scared scary frighten frightened frightening
        terrifying terrified terror terrors horror horrified horrifying panic
        panicked panicking dread dreaded dreading anxious anxiety anxiously
        worried worrying worry worries nervous nervously uneasy unease
        apprehensive alarmed alarming startled startling spooked spooky
        paranoid paranoia threat threatened threatening threaten threatens
        danger dangers dangerous dangerously risky risk risks risking creepy
        creeped chilling unsettle unsettling unnerve unnerving nightmare
        nightmares phobia phobias tremble trembling trembled shaky quiver
        quivering desperado desperat desperate hopeless trapped trap stalked
        stalking lurking lurker drown drowned drowning fall fell falling
        crash crashed crashing collaps collapses collapsed collapsing burning
        burned alive
    """,
    "joy": """
        joy joyful joys happy happier happiest happiness delight delighted
        delightful delights glad gladly thrilled thrilling thrillingly love
        loved loving lovely loves adore adores adored wonderful wonderfully
        amazing amazingly awesome awesomely great greatest fantastic
        fantastical incredible incredibly brilliant brilliantly fabulous
        euphoric ecstatic ecstasy bliss blissful cheerful cheer cheerful
        cheering cheered celebrate celebrating celebration celebrated laugh
        laughs laughing laughed hilarious hilarity fun funny funnier smiling
        smile smiles grinned grin enjoy enjoys enjoyed enjoying gratitude
        grateful thankfully thankful proud pride proudly satisfied
        satisfaction satisfied hopeful hopefully hope bright brighter sunnier
        sunny sweet sweeter cozy comfy beautiful beautifully gorgeous
        magnificen magnificent magnificent winning win wins won triumph
        triumphed triumphal succeed success successful succeeded succeed
        glorious heartwarming wholesome magic magical
    """,
    "sadness": """
        sad sadder saddest sadness unhappy unhappily sorrow sorrowful
        sorrowfully grief grieving grieved mourn mourning mourned mournful
        heartbroken heartbreak heartbroke depressed depressing depression
        lonely loneliness alone lonelier isolated isolating empty emptiness
        emptying despair despairing despaired miserable misery miseries
        crying cries cried cry tears tear weep weeping wept anguish anguished
        gloom gloomy melancholy blue blues regret regrets regretful
        regretting disappointed disappointing disappoints disappoint lost
        loss losses losing hurt hurts hurting aching ached ache broken breaks
        defeat defeated defeatedly resigned resigning resignation sigh sighed
        sighing fade fading faded forgotten forget forgot misses missed
        missing gone goodbye goodbyes farewell
    """,
    "surprise": """
        surprise surprised surprises surprising shocked shocking shocks
        astonished astonishing astounded astounding stunned stun stuns
        unexpected unexpectantly suddenly sudden abrupt abruptly whoa wow
        unbelievable unbelieveably incredulous disbelief jolt jolted jolting
        plot twist twists reveal revealed revealing revelation revealed
        bam boom wham gasp gasped gasping freakish freaked odder odd
        strangely strange curiously curious mysterious mysteriously nowhere
        startlingly wait holds believing believe unbeknownst secretly
        secret turns turn turned flip flipped flipside underdog upsets upset
    """,
}

# Multi-word phrases per emotion, matched on the normalised stream.
_PHRASES = {
    "anger": ("cant stand", "sick of", "fed up", "enough is enough", "how dare"),
    "disgust": ("gross out", "makes me sick", "turns your stomach",),
    "fear": ("scared to death", "keeps me up", "worried sick", "heart racing",
             "hold your breath", "on edge"),
    "joy": ("cant wait", "best ever", "love it", "on top of the world",
            "over the moon", "dream come true", "thank you"),
    "sadness": ("miss you", "miss them", "miss him", "miss her", "left behind",
                "never came back", "all alone", "gone forever", "no one left"),
    "surprise": ("cant believe", "wont believe", "no way", "plot twist",
                 "turns out", "here s the thing", "guess what",
                 "who would have thought", "hard to believe",
                 "believe it or not", "out of nowhere",
                 "hold on", "wait for it", "the truth is"),
}

# Words that raise or lower the felt intensity of whatever surrounds them.
_AMPLIFIERS = """
    very really extremely incredibly absolutely totally completely utterly
    super insanely massively hugely phenomenally unimaginably unfathomably
    ridiculously absurdly impossibly devastatingly painfully hyper mega
""".split()

# Words that cool a line down regardless of its label: a calm, grateful
# morning is low-arousal joy even with a clean lexical hit. They damp the
# arousal estimate directly (never below 0.4 of it), because intensity and
# arousal are different axes - a whisper can be certain.
_CALMERS = """
    calm calmly quiet quietly peace peaceful peacefully serene serenity
    gentle gently soft softly slow slowly slower still stillness mellow
    soothing soothingly relaxed relaxing unwind resting rested hush
""".split()

_HEDGES = """
    maybe perhaps might could seems seem appears somewhat slightly kind sorta
    arguably possibly likely apparently reportedly supposedly relatively
    fairly reasonably moderately a bit
""".split()

# Contractions whose split form collides with a real word: normalised text
# turns "won't" into "won t", where "won" is both the verb (a joy hit) and
# a negation fragment. The stream merge makes each a single token again.
_CONTRACTION_TAILS = frozenset((
    "won", "can", "don", "doesn", "didn", "isn", "aren", "wasn", "weren",
    "wouldn", "couldn", "shouldn", "hasn", "haven", "ain", "mustn", "needn",
    "daren", "shan",
))


# One negation form per contraction, as the merged stream spells it. No bare
# fragments ("won", "don", "t"): after the merge they no longer occur, and
# kept, they would negate real words ("they won, believe me").
_NEGATIONS = """
    not no never none cannot cant wont dont doesnt didnt isnt arent wasnt
    werent wouldnt couldnt shouldnt hasnt havent aint without hardly barely
    nothing nobody nowhere
""".split()

# ---------------------------------------------------------------------------
# What each emotion demands of the edit. `easing` carries BOTH a name a
# person reads and the exact authoring-JSON form concat-motion's `curve`
# field takes (see curve.rs: serde tag "kind", kebab-case). `colour` is a
# direction plus the stock LUT-family hint, not a claim about any package.
# ---------------------------------------------------------------------------

_EASINGS = {
    "ease-in": {"kind": "bezier", "x1": 0.42, "y1": 0.0, "x2": 1.0, "y2": 1.0},
    "ease-out": {"kind": "bezier", "x1": 0.0, "y1": 0.0, "x2": 0.58, "y2": 1.0},
    "ease-in-out": {"kind": "bezier", "x1": 0.42, "y1": 0.0, "x2": 0.58, "y2": 1.0},
    "emphasis": {"kind": "bezier", "x1": 0.16, "y1": 1.0, "x2": 0.3, "y2": 1.0},
    "snap-in": {"kind": "bezier", "x1": 0.7, "y1": 0.0, "x2": 1.0, "y2": 0.3},
    "spring": {"kind": "spring", "stiffness": 170.0, "damping": 26.0, "mass": 1.0},
    "spring-pop": {"kind": "spring", "stiffness": 260.0, "damping": 15.0, "mass": 1.0},
}

PROFILE = {
    "anger": {
        "valence": -0.7, "arousal": 0.9,
        "easing": "snap-in", "colour": {"temperature": "hot", "gradeHint": "pushed contrast, amber-red bias, crushed blacks"},
        "sfxType": "impact-heavy (hard hits, distorted risers)",
        "delivery": "urgent", "pacing": "energetic 16-24 cuts/min",
        "vocabulary": ("fire storm clash confrontation smoke shattered red heat",
                       "protest crowd impact explosion burning"),
    },
    "disgust": {
        "valence": -0.6, "arousal": 0.5,
        "easing": "ease-in", "colour": {"temperature": "sickly", "gradeHint": "green-yellow cast, low saturation"},
        "sfxType": "texture (squelch, wet, close-mic)",
        "delivery": "conversational", "pacing": "standard 10-16 cuts/min",
        "vocabulary": ("murky swamp sludge grimy texture",
                       "landfill trash polluted rotting decay"),
    },
    "fear": {
        "valence": -0.7, "arousal": 0.85,
        "easing": "ease-in", "colour": {"temperature": "cold", "gradeHint": "teal-blue bias, crushed blacks, low key"},
        "sfxType": "low drone, creaks, reversed swells",
        "delivery": "urgent", "pacing": "energetic 16-24 cuts/min",
        "vocabulary": ("dark shadow night fog maze alone",
                       "abyss cliff heights empty corridor dense forest"),
    },
    "joy": {
        "valence": 0.8, "arousal": 0.7,
        "easing": "ease-out", "colour": {"temperature": "warm", "gradeHint": "golden hour bias, lifted blacks, saturated highs"},
        "sfxType": "bright chime, pop, uplifting swell",
        "delivery": "conversational", "pacing": "standard 10-16 cuts/min",
        "vocabulary": ("sunlight sunrise meadow bloom celebration",
                       "confetti smiles friends golden beach dancing"),
    },
    "neutral": {
        "valence": 0.0, "arousal": 0.3,
        "easing": "ease-in-out", "colour": {"temperature": "balanced", "gradeHint": "the brand guide's base grade, no extra push"},
        "sfxType": "minimal tick/click (data sounds only)",
        "delivery": "authoritative", "pacing": "standard 10-16 cuts/min",
        "vocabulary": ("clean minimal desk diagram aerial",
                       "wide landscape city data screen"),
    },
    "sadness": {
        "valence": -0.6, "arousal": 0.25,
        "easing": "ease-out", "colour": {"temperature": "cool", "gradeHint": "desaturated blue-grey, soft roll-off"},
        "sfxType": "soft pad, single piano note, room tone",
        "delivery": "calm", "pacing": "calm 6-10 cuts/min",
        "vocabulary": ("rain window grey autumn wilted",
                       "empty street single figure faded cold mist"),
    },
    "surprise": {
        "valence": 0.2, "arousal": 0.8,
        "easing": "spring-pop", "colour": {"temperature": "split", "gradeHint": "teal-orange pushed, contrast pop on the reveal"},
        "sfxType": "hit + beat of silence (stinger)",
        "delivery": "awed", "pacing": "hype 24+ cuts/min on the burst, then hold",
        "vocabulary": ("burst flash reveal door opening light beam",
                       "zoom pop double-take confetti"),
    },
}

# How each act prefers to be shot. The gate checks stock candidates against
# these registers; the plan uses them as query scaffolding.
ACT_PROFILE = {
    "hook": {
        "shot": ("extreme close-up", "dramatic reveal", "striking high-contrast"),
        "duckDb": -15.0, "note": "the voice is naked; the hook must be heard",
    },
    "setup": {
        "shot": ("wide establishing", "slow pan", "context", "map"),
        "duckDb": -12.0, "note": "standard duck under speech",
    },
    "tension": {
        "shot": ("tight abstract", "shallow focus", "motion blur", "hand-held"),
        "duckDb": -12.0, "note": "tight duck; clarity over mood",
    },
    "turn": {
        "shot": ("pivot", "crossroads", "split screen", "mirror"),
        "duckDb": -12.0, "note": "the music can lean in as the argument turns",
    },
    "reveal": {
        "shot": ("bright clear hero", "macro detail", "sunrise", "lit subject"),
        "duckDb": -9.0, "note": "let the bed ride: the reveal is felt, not narrated",
    },
    "proof": {
        "shot": ("chart", "data screen", "lab", "document", "demonstration"),
        "duckDb": -12.0, "note": "standard duck; numbers must be legible",
    },
    "cta": {
        "shot": ("direct address", "smiling person", "gesture", "bright simple"),
        "duckDb": -10.0, "note": "bed comes up behind the ask",
    },
}

# Lexical markers for the act classifier, strongest first: an explicit CTA
# beats position, and an explicit reveal beats a mid-piece default.
_ACT_MARKERS = (
    ("cta", ("subscribe", "follow", "comment", "link in bio", "link below",
             "check the", "share this", "next video", "see you in")),
    ("reveal", ("turns out", "the truth is", "here s the thing", "the answer",
                "what they found", "that s why", "so that s", "the reason is",
                "it was there all along", "the secret")),
    ("turn", ("but", "however", "until", "then one day", "everything changed",
              "that changed", "nobody expected", "what if", "instead")),
    ("proof", ("study", "studies", "data", "research", "numbers", "percent",
               "scientists", "experiment", "measured", "survey", "report",
               "in fact", "the evidence")),
    ("setup", ("imagine", "think about", "consider", "picture this",
               "to understand", "it starts with", "first", "it began",
               "back in", "every day", "here s how")),
)

# The hormone lens: a creative shorthand for the viewer's state, not
# physiology. Priority order matters - laughter outranks everything, and a
# positive reveal is dopamine even when the line reads surprised.
def _hormone_for(emotion, intensity, arousal, act, text):
    tokens = set(_tokens(text))
    if tokens & {"laugh", "laughs", "laughing", "hilarious", "joke", "lol"}:
        return "endorphin"
    if act in ("reveal", "turn") and (emotion in ("joy", "surprise")):
        return "dopamine"
    if emotion in ("fear", "surprise") and arousal >= 0.65:
        return "adrenaline"
    if emotion == "anger" or (act == "tension" and arousal >= 0.6):
        return "cortisol"
    if emotion == "sadness" and intensity >= 0.4:
        return "oxytocin"
    if emotion == "joy" and arousal < 0.5:
        return "serotonin"
    if emotion == "surprise":
        return "dopamine"
    if emotion == "sadness":
        return "oxytocin"
    if arousal >= 0.6:
        return "adrenaline"
    return "serotonin"


# ---------------------------------------------------------------------------
# Tokenising and the lexicon engine.
# ---------------------------------------------------------------------------

def _normalise(text):
    """Lowercase, apostrophes to spaces, keep letters and digits only."""
    return re.sub(r"[^a-z0-9]+", " ", (text or "").lower()).strip()


def _tokens(text):
    return _normalise(text).split()


def _stream(text):
    """The normalised token stream with t-contractions merged back into
    single tokens ("won t" -> "wont"), so a negation is one token and the
    verb "won" cannot masquerade as one. Phrases and markers match here."""
    tokens = _normalise(text).split()
    merged = []
    skip_next = False
    for index, token in enumerate(tokens):
        if skip_next:
            skip_next = False
            continue
        if (token in _CONTRACTION_TAILS and index + 1 < len(tokens)
                and tokens[index + 1] == "t"):
            merged.append(token + "t")
            skip_next = True
        else:
            merged.append(token)
    return " ".join(merged)


def _calm_factor(text):
    """How much the calm words in a line cool its arousal: 1.0 untouched,
    0.7 for one, 0.4 at three or more. Intensity is a different axis and
    stays as measured - a whisper can be certain."""
    hits = sum(1 for token in _tokens(text) if token in _CALMERS)
    return max(0.4, 1.0 - 0.3 * min(3, hits))


def _lexicon_scores(text):
    """Raw emotion evidence in one line: stem hits, phrase hits, negation
    flips, amplifier and hedge weighting. Returns unnormalised weights."""
    stream = _stream(text)
    tokens = stream.split()
    scores = {emotion: 0.0 for emotion in EMOTIONS}
    for i, token in enumerate(tokens):
        weight = 1.0
        # An amplifier up to two places before a hit counts once per hit.
        window = tokens[max(0, i - 2):i]
        weight += 0.5 * sum(1 for w in window if w in _AMPLIFIERS)
        weight -= 0.5 * sum(1 for w in window if w in _HEDGES)
        if weight <= 0:
            continue
        # Negation within three places before flips a hit into its opposite
        # valence: "not happy" is evidence of a negative line, not a joyful
        # one. The flipped score lands on the negative side (0.4), never a
        # specific emotion - the lexicon is not sarcastic.
        negated = any(w in _NEGATIONS for w in tokens[max(0, i - 3):i])
        for emotion in EMOTIONS:
            if emotion == "neutral":
                continue
            stems = _LEXICON[emotion].split()
            hit = any(len(stem) >= 4 and token.startswith(stem) or token == stem
                      for stem in stems)
            if hit:
                if negated and emotion in ("joy", "surprise"):
                    scores["sadness"] += 0.4 * weight
                    scores[emotion] = max(0.0, scores[emotion] - 0.1)
                elif negated:
                    scores[emotion] += 0.4 * weight
                else:
                    scores[emotion] += weight
    for emotion, phrases in _PHRASES.items():
        for phrase in phrases:
            if phrase in stream:
                scores[emotion] += 1.6
    return scores


def _intensity_signals(text):
    """Felt intensity from the page: caps, bangs, ellipses. Returns a delta
    in roughly -0.25..+0.45 to add to the score-based base."""
    raw = text or ""
    letters = [c for c in raw if c.isalpha()]
    caps_ratio = (sum(1 for c in letters if c.isupper()) / len(letters)) if letters else 0.0
    delta = 0.0
    if caps_ratio > 0.6 and len(letters) >= 4:
        delta += 0.20
    elif caps_ratio > 0.3 and len(letters) >= 6:
        delta += 0.10
    bangs = raw.count("!")
    delta += min(0.16, 0.08 * bangs)
    if "?" in raw:
        delta += 0.05
    if "..." in raw or "…" in raw:
        delta -= 0.06
    tokens = _tokens(raw)
    delta += 0.06 * min(3, sum(1 for t in tokens if t in _AMPLIFIERS))
    delta -= 0.06 * min(3, sum(1 for t in tokens if t in _HEDGES))
    return delta


# ---------------------------------------------------------------------------
# The hosted engine: tabularisai/multilingual-emotion-classification on the
# HuggingFace Inference API, free tier, batched POSTs for the lines.
# ---------------------------------------------------------------------------

def _emotion_api_key(spec):
    for source in (spec.get("apiKey"), os.environ.get("FUNCUT_EMOTION_API_KEY"),
                   os.environ.get("HF_TOKEN"), os.environ.get("HUGGINGFACE_API_KEY")):
        if source and str(source).strip():
            return str(source).strip()
    return ""


def _mask(key):
    return key[:3] + "..." + key[-2:] if len(key) > 8 else "***"


def _fold_model_scores(ranking):
    """One model answer (its 11 labels with scores, sigmoid-independent)
    folded onto the edit's 7. Scores need not sum to 1 - the model is
    multi-label, so each label carries its own probability; the fold adds
    them into their target and the normaliser downstream turns the pile
    into a share. Unknown labels are ignored rather than fatal: a model
    swap on the hosted side must degrade to neutral, never to an error."""
    folded = {emotion: 0.0 for emotion in EMOTIONS}
    if isinstance(ranking, dict):
        ranking = [ranking]
    if not isinstance(ranking, list):
        raise RuntimeError("the emotion model returned an unexpected shape")
    for entry in ranking:
        if not isinstance(entry, dict):
            raise RuntimeError("the emotion model returned an unexpected shape")
        label = str(entry.get("label", "")).lower()
        target = MODEL_LABEL_MAP.get(label)
        if target is not None:
            # A score that is not a finite number - inf, nan, a string -
            # is no evidence at all rather than an error... and never a
            # NaN that escapes into json.dumps as invalid JSON the Rust
            # side then refuses to parse.
            try:
                score = float(entry.get("score", 0.0))
            except (TypeError, ValueError):
                score = 0.0
            if not (score == score and score not in (float("inf"), float("-inf"))):
                score = 0.0
            folded[target] += max(0.0, score)
    return folded


def _hf_scores(lines, key, model, timeout=30.0):
    """Per-line label->score dicts (the edit's 7) from the hosted model, or
    raises with the honest reason. Text-classification accepts a list of
    inputs and answers a list of per-input label rankings; long transcripts
    go in chunks so one POST stays light on the free tier."""
    model = model or DEFAULT_MODEL
    scores = []
    for start in range(0, len(lines), MODEL_BATCH):
        chunk = lines[start:start + MODEL_BATCH]
        body = json.dumps({"inputs": chunk}).encode("utf-8")
        payload = None
        last_error = "no endpoint tried"
        for template in HF_ENDPOINTS:
            url = template.format(model=model)
            request = urllib.request.Request(
                url, data=body, method="POST",
                headers={"Authorization": f"Bearer {key}",
                         "Content-Type": "application/json"},
            )
            try:
                with urllib.request.urlopen(request, timeout=timeout) as reply:
                    payload = json.loads(reply.read().decode("utf-8"))
                break
            except urllib.error.HTTPError as exc:
                detail = exc.read().decode("utf-8", "replace")[:200]
                last_error = f"HTTP {exc.code} from {url.split('/models/')[0]}: {detail}"
                if exc.code in (401, 403):
                    raise RuntimeError(f"the emotion API key was refused ({last_error})")
            except Exception as exc:  # network down, timeout, DNS: fall back
                last_error = f"{type(exc).__name__}: {exc}"
        if payload is None:
            raise RuntimeError(last_error)
        # A single-line call may answer one flat ranking instead of the
        # list-of-rankings a batch always gets.
        if len(chunk) == 1 and isinstance(payload, list) and payload \
                and isinstance(payload[0], dict):
            payload = [payload]
        if not isinstance(payload, list) or len(payload) != len(chunk):
            raise RuntimeError("the emotion model returned an unexpected shape")
        scores.extend(_fold_model_scores(ranking) for ranking in payload)
    return scores


def _merge_scores(model, lexicon):
    """The hosted model carries 75% when it answers; the lexicon keeps 25%
    because it sees punctuation and caps the model never will. The model is
    the read now - it is multilingual and semantic; the lexicon is the
    marginal signal, not the peer it was beside the English-only model."""
    merged = {}
    for emotion in EMOTIONS:
        merged[emotion] = 0.75 * (model or {}).get(emotion, 0.0) + 0.25 * (lexicon or {}).get(emotion, 0.0)
    return merged


def _normalise_distribution(scores):
    """Scores to a distribution summing to 1 so `top` is a share of evidence.
    No evidence at all is the definition of neutral: a uniform 1/7 would
    hand the line to whatever label sorts first, which is alphabet, not
    psychology."""
    # Non-finite scores are dropped rather than divided through: inf/inf
    # is NaN, and NaN printed by json.dumps is invalid JSON the Rust side
    # reads as "the tool returned no JSON" - the whole call lost to one
    # stray model score.
    finite = {
        emotion: value
        for emotion, value in scores.items()
        if value == value and value not in (float("inf"), float("-inf"))
    }
    total = sum(v for v in finite.values() if v > 0)
    if total <= 0 or not (total == total):
        return {emotion: (1.0 if emotion == "neutral" else 0.0) for emotion in EMOTIONS}
    return {emotion: max(0.0, finite.get(emotion, 0.0)) / total for emotion in EMOTIONS}


# ---------------------------------------------------------------------------
# The act classifier: dramaturgy from position, markers and emotion shifts.
# ---------------------------------------------------------------------------

def _classify_act(index, total, emotion, previous, text, intensity,
                  sections=None, line_start=None):
    """One act per line, deterministic. Priority: an explicit section role
    the line's own start time falls inside (NOT sections[index] - that is
    the section at the line's position in the list, which has nothing to do
    with when the line is spoken), then an explicit CTA marker (unmistakable
    wherever it lands), then the hook position (the first line IS the hook
    unless it says otherwise - a stray 'first' must not steal it), then the
    remaining story markers, then an emotion U-turn mid-piece (a turn), then
    the three-act default."""
    stream = _stream(text)
    if sections and line_start is not None:
        for section in sections:
            if section.get("start", 0.0) <= line_start < section.get("end", 1e9):
                role = str(section.get("role", ""))
                if role in ("hook", "cta", "outro"):
                    return "cta" if role == "outro" else role
                break
    cta_markers = _ACT_MARKERS[0][1]
    if any(marker in stream for marker in cta_markers):
        return "cta"
    if total > 2 and index == 0:
        return "hook"
    for act, markers in _ACT_MARKERS[1:]:
        for marker in markers:
            if marker in stream:
                return act
    if total > 3 and index >= total - 1:
        return "cta"
    # An emotional U-turn between consecutive lines is the story turning.
    if previous and previous != emotion and previous != "neutral" and emotion != "neutral":
        previous_valence = PROFILE[previous]["valence"]
        valence = PROFILE[emotion]["valence"]
        if previous_valence * valence < 0 or abs(valence - previous_valence) >= 0.6:
            return "turn"
    third = index / max(1, total - 1) if total > 1 else 0.0
    if third < 0.3:
        return "setup"
    if third < 0.75:
        return "tension"
    return "proof"


# ---------------------------------------------------------------------------
# The arc: what the whole narration does to the viewer over time.
# ---------------------------------------------------------------------------

def _arc_analysis(lines, brief_arc=None):
    valences = [line["valence"] for line in lines]
    arousals = [line["arousal"] for line in lines]
    emotions = [line["emotion"] for line in lines]
    distinct = len(set(emotions))
    longest_run = 1
    run = 1
    for pair in zip(emotions, emotions[1:]):
        run = run + 1 if pair[0] == pair[1] else 1
        longest_run = max(longest_run, run)

    # Shape from the valence means of each half plus the sign changes.
    midpoint = len(valences) // 2
    first_half = sum(valences[:midpoint]) / max(1, midpoint)
    second_half = sum(valences[midpoint:]) / max(1, len(valences) - midpoint)
    spread = max(valences) - min(valences) if valences else 0.0
    crossings = sum(1 for pair in zip(valences, valences[1:])
                    if pair[0] * pair[1] < 0)
    if spread < 0.25:
        shape = "flat"
    elif crossings >= 2 or (spread >= 0.5 and crossings >= 1):
        shape = "rollercoaster"
    elif second_half - first_half >= 0.3:
        shape = "rise"
    elif first_half - second_half >= 0.3:
        shape = "fall"
    elif min(valences[:midpoint] or [0]) < -0.3 and second_half > first_half:
        shape = "dip-rise"
    else:
        shape = "drift"

    warnings = []
    score = 10.0
    if shape == "flat":
        score -= 3.0
        warnings.append(
            "the arc is flat: every line lands in the same emotional register; "
            "rewrite or re-deliver at least one section to move the viewer")
    if distinct < (2 if len(lines) <= 6 else 3):
        score -= 1.5
        warnings.append(
            f"only {distinct} distinct emotions across {len(lines)} lines; "
            "the voiceover visits too few states")
    if longest_run > max(2, len(lines) // 3):
        score -= 1.0
        warnings.append(
            f"{longest_run} consecutive lines sit in the same emotion; "
            "insert a contrast beat (a turn, a question, a reveal)")
    if lines and lines[0]["arousal"] < 0.45:
        score -= 1.0
        warnings.append(
            "the opening line is low-arousal; the hook should be surprise, "
            "joy or tension inside the first three seconds")
    has_reveal = any(line["act"] in ("reveal", "turn") for line in lines)
    if lines and not has_reveal and len(lines) >= 4:
        score -= 1.0
        warnings.append(
            "no reveal or turn anywhere: the piece states, it never turns; "
            "the payoff needs a 'turns out' moment")

    # When the brief declares its emotional arc, check the traversal: the
    # plan's lines should actually visit the beats the brief promised.
    matched_beats = []
    if brief_arc:
        beat_emotions = []
        for beat in brief_arc:
            beat_scores = _lexicon_scores(str(beat))
            beat_emotions.append(max(beat_scores, key=lambda e: beat_scores[e])
                                 if max(beat_scores.values()) > 0 else "neutral")
        for beat, beat_emotion in zip(brief_arc, beat_emotions):
            if any(emotion == beat_emotion for emotion in emotions):
                matched_beats.append(beat)
        if len(matched_beats) < max(1, len(brief_arc) - 1):
            score -= 1.0
            warnings.append(
                f"the brief's emotionalArc promised {brief_arc} but the lines "
                f"only clearly visit {matched_beats or ['none']}; re-read or rewrite")
    score = max(0.0, min(10.0, score))
    return {
        "arcShape": shape,
        "valenceCurve": [round(v, 3) for v in valences],
        "arousalCurve": [round(a, 3) for a in arousals],
        "distinctEmotions": distinct,
        "longestRun": longest_run,
        "firstHalfValence": round(first_half, 3),
        "secondHalfValence": round(second_half, 3),
        "briefBeatsMatched": matched_beats,
        "arcScore": round(score, 2),
        "warnings": warnings,
    }


# ---------------------------------------------------------------------------
# The extract command.
# ---------------------------------------------------------------------------

def _latin_share(texts):
    """The share of letters that are plain a-z. Under ~0.7 the English stem
    lexicon is reading a translation, not the text: accented Latin mostly
    still matches, non-Latin scripts (Bengali, Arabic, Chinese, ...) match
    nothing at all. The hosted model reads all of them natively - the note
    says so instead of quietly answering neutral."""
    letters = 0
    ascii_letters = 0
    for text in texts:
        for char in text or "":
            if char.isalpha():
                letters += 1
                if char.isascii():
                    ascii_letters += 1
    return (ascii_letters / letters) if letters else 1.0


def _collect_lines(spec):
    """Lines from a transcript job, a bare list, or raw script text."""
    if isinstance(spec.get("lines"), list) and spec["lines"]:
        lines = []
        for entry in spec["lines"]:
            if isinstance(entry, str):
                lines.append({"text": entry})
            elif isinstance(entry, dict) and str(entry.get("text") or "").strip():
                # `or ""`, not a default: a `{"text": null}` line used to
                # read as the literal string "None" and be analysed as a
                # spoken line.
                lines.append(entry)
        return lines
    transcript = spec.get("transcript")
    if isinstance(transcript, dict) and isinstance(transcript.get("segments"), list):
        segments = [s for s in transcript["segments"]
                    if isinstance(s, dict) and str(s.get("text") or "").strip()]
        if segments:
            return segments
    for key in ("script", "text"):
        if isinstance(spec.get(key), str) and spec[key].strip():
            # Script text: split on sentence enders and newlines; a line is a
            # sentence, because emotion and delivery are per-sentence facts.
            pieces = re.split(r"(?<=[.!?])\s+|\n+", spec[key].strip())
            return [{"text": piece.strip()} for piece in pieces if piece.strip()]
    return []


def extract(spec):
    lines = _collect_lines(spec)
    if not lines:
        return {"ok": False,
                "error": "no lines: pass transcript (a transcribe job's output), "
                         "lines [{text, start, end}], or script (raw text)"}
    texts = [str(line.get("text", "")) for line in lines]
    brief = spec.get("brief") or {}
    brief_arc = brief.get("emotionalArc") if isinstance(brief, dict) else None

    key = _emotion_api_key(spec)
    hf_rows = None
    engine_note = ""
    if key:
        model = spec.get("model") or os.environ.get("FUNCUT_EMOTION_MODEL") or DEFAULT_MODEL
        try:
            hf_rows = _hf_scores(texts, key, model)
            engine = "model+lexicon"
            confidence = 0.85
        except Exception as exc:
            engine_note = f" (hosted model unavailable: {exc}; fell back to the lexicon)"
    if hf_rows is None:
        engine = "lexicon"
        confidence = 0.55
        # The lexicon is English-only; a narration in another script would
        # read as neutral line after line, silently. Say it: the hosted
        # model is multilingual, the key is free, and the read is the edit's
        # driver - the director deserves to know what it is driving on.
        # (When the hosted call itself failed, both facts travel: why it
        # fell back, and what the fallback cannot read.)
        if _latin_share(texts) < 0.7:
            non_english = (" (the stem lexicon reads English only and this text "
                           "is mostly not; set FUNCUT_EMOTION_API_KEY for the "
                           f"multilingual model {DEFAULT_MODEL})")
            engine_note = (engine_note + non_english) if engine_note else non_english

    decorated = []
    previous_emotion = None
    sections = spec.get("sections") if isinstance(spec.get("sections"), list) else None
    for index, (line, text) in enumerate(zip(lines, texts)):
        raw_start = line.get("start")
        line_start = raw_start if isinstance(raw_start, (int, float)) else None
        lexicon = _lexicon_scores(text)
        if hf_rows is not None:
            scores = _normalise_distribution(_merge_scores(hf_rows[index], lexicon))
        else:
            scores = _normalise_distribution(lexicon)
        ranked = sorted(scores.items(), key=lambda kv: -kv[1])
        emotion, top = ranked[0]
        runner_up, second = ranked[1]
        margin = top - second
        # Neutral is a claim like any other: a thin win over neutral means
        # the line is informational, and the plan should treat it as such.
        if emotion != "neutral" and margin < 0.08 and scores["neutral"] > 0.45:
            emotion, top = "neutral", scores["neutral"]
        intensity = max(0.1, min(1.0, 0.45 + 0.8 * margin + _intensity_signals(text))) \
            if emotion != "neutral" else max(0.1, min(0.6, 0.3 + _intensity_signals(text)))
        profile = PROFILE[emotion]
        valence = profile["valence"]
        arousal = round(min(1.0, profile["arousal"] * (0.6 + 0.4 * intensity)
                            * _calm_factor(text)), 3)
        act = _classify_act(index, len(lines), emotion, previous_emotion, text,
                            intensity, sections, line_start)
        hormone = _hormone_for(emotion, intensity, arousal, act, text)
        easing_name = profile["easing"]
        # Intensity bends the curve, not just the volume: a mild surprise is
        # a settle (emphasis), a burst is a spring-pop. The authoring JSON is
        # the exact shape concat-motion's `curve` field takes.
        if emotion == "surprise" and intensity < 0.5:
            easing_name = "emphasis"
        if emotion == "joy" and intensity > 0.75 and act in ("hook", "reveal"):
            easing_name = "spring-pop"
        duck = ACT_PROFILE[act]["duckDb"]
        row = {
            "index": index,
            "text": text,
            "emotion": emotion,
            "intensity": round(intensity, 3),
            "scores": {e: round(scores[e], 4) for e in EMOTIONS},
            "runnerUp": runner_up,
            "margin": round(margin, 4),
            "valence": valence,
            "arousal": arousal,
            "act": act,
            "hormone": hormone,
            "delivery": profile["delivery"],
            "speed": round(1.0 + (0.08 if intensity > 0.7 and arousal >= 0.7
                                  else (-0.05 if arousal < 0.35 else 0.0)), 3),
            "easing": {"name": easing_name, "curve": _EASINGS[easing_name]},
            "colourTemp": dict(profile["colour"]),
            "sfxType": profile["sfxType"],
            "duckDb": duck,
            "pacing": profile["pacing"],
            "vocabulary": list(profile["vocabulary"]),
            "queries": _queries(profile, text, act),
            "start": line.get("start"),
            "end": line.get("end"),
        }
        decorated.append(row)
        previous_emotion = emotion

    result = {
        "ok": True,
        "engine": engine,
        "confidence": confidence,
        "lines": decorated,
        "arc": _arc_analysis(decorated, brief_arc),
        "note": "emotions are measured from the words ("
                + (f"hosted multilingual model + lexicon: {model}"
                   if engine == "model+lexicon"
                   else "stem lexicon; set FUNCUT_EMOTION_API_KEY for the hosted "
                        f"multilingual model {DEFAULT_MODEL}")
                + "); the hormone field is a creative shorthand for the viewer's "
                  "state, not physiology" + engine_note,
    }
    if key:
        result["apiKey"] = _mask(key)
    if engine == "model+lexicon":
        result["model"] = model
    if isinstance(spec.get("brand"), dict):
        result["brand"] = brand_apply(spec["brand"], decorated)
    return result


def _queries(profile, text, act):
    """Two-or-three-word stock queries per the broll_matching law: the
    emotion's visual register crossed with the act's shot preference."""
    register = profile["vocabulary"][0].split()[:3]
    shot = ACT_PROFILE[act]["shot"][0].split()[:1]
    noun = next((t for t in _tokens(text) if len(t) > 4
                 and t not in _AMPLIFIERS and t not in _HEDGES), "")
    queries = []
    if noun:
        queries.append(" ".join([noun] + register[:1]))
    queries.append(" ".join(register[:2]))
    queries.append(" ".join(shot + register[:1]))
    return queries[:3]


# ---------------------------------------------------------------------------
# The brand guide: one channel's non-negotiables, validated and applied.
# ---------------------------------------------------------------------------

BRAND_REQUIRED = ("id", "name", "vibe", "palette", "type", "grade", "taboos")

# The ready-to-edit example a director starts from. It is returned by the
# `template` action (and referenced by every validation error) so the guide
# travels with the binary - no file to find, nothing to download.
BRAND_TEMPLATE = {
    "id": "science-shorts",
    "name": "Science Shorts",
    "vibe": "bright conspiratorial science: curious, fast, never cynical",
    "palette": {"background": "#0B1220", "ink": "#F5F7FA", "accent": "#4CC9F0"},
    "type": {"family": "Archivo", "weight": 700,
             "note": "a grotesk that stays loud at caption sizes"},
    "motion": {"easing": "auto",
               "note": "auto = the emotion decides (tense ease-in, warm "
                       "ease-out, a burst pops); a fixed name overrides "
                       "NEUTRAL beats only"},
    "grade": {"base": "concat.teal-orange", "amount": 25,
              "note": "per-act colour temp from the emotion read pushes "
                      "on top of this base"},
    "sfx": {"palette": ["impact-*", "whoosh-*", "riser-*"],
            "note": "register from the emotion read: impacts for anger, "
                    "drones for fear, chimes for joy"},
    "bgm": {"genres": ["ambient electronic"], "duckDb": -12,
             "note": "duckDb here is the default; the act's duck wins"},
    "taboos": ["lens flare", "beaker stock footage", "flame particle"],
    "voice": {"reference": "curious, assured, warm; never shouty",
              "deliveries": ["conversational", "awed", "urgent"]},
}


def brand_template():
    """The example brand guide, with every field a guide may carry."""
    return {
        "ok": True,
        "template": BRAND_TEMPLATE,
        "required": list(BRAND_REQUIRED),
        "note": "copy the template, make it yours, pass it as `brand` (or "
                "store it as brandKit in creative_brief and give `project`); "
                "the emotion read applies it and the gate enforces it",
    }


def brand_validate(brand):
    """Structural validation with actionable messages; returns errors."""
    if not isinstance(brand, dict):
        return ["the brand guide must be a JSON object"]
    errors = []
    for key in BRAND_REQUIRED:
        if key not in brand or brand[key] in (None, "", [], {}):
            errors.append(f"brand.{key} is required (action 'template' returns a ready-to-edit example)")
    palette = brand.get("palette")
    if isinstance(palette, dict):
        for key in ("background", "ink", "accent"):
            value = str(palette.get(key, ""))
            if value and not re.fullmatch(r"#[0-9a-fA-F]{6}", value):
                errors.append(f"brand.palette.{key} should be a #RRGGBB hex colour, got {value!r}")
    elif palette is not None:
        errors.append("brand.palette must be an object with background, ink, accent (hex)")
    taboos = brand.get("taboos")
    if taboos is not None and not isinstance(taboos, list):
        errors.append("brand.taboos must be a list of strings the edit must never contain")
    return errors


def brand_apply(brand, lines):
    """Validate a brand guide, then apply it: per-act constraints the edit
    must satisfy and any taboo violations in the lines' queries/texts."""
    errors = brand_validate(brand)
    if errors:
        return {"ok": False, "errors": errors}
    per_act = {
        act: {
            "duckDb": profile["duckDb"],
            "shot": list(profile["shot"]),
            "note": profile["note"],
        }
        for act, profile in ACT_PROFILE.items()
    }
    motion = brand.get("motion") or {}
    if motion.get("easing") not in (None, "auto"):
        # A brand that fixes its easing overrides the emotion's default for
        # NEUTRAL beats only - the user's rule (tense = ease-in, warm =
        # ease-out) is the craft law; the brand constraint applies to
        # everything else.
        per_act["brandEasing"] = motion.get("easing")
    taboos = [str(t).lower() for t in (brand.get("taboos") or [])]
    violations = []
    for line in lines:
        haystack = " ".join(
            [str(line.get("text", ""))] + [str(q) for q in line.get("queries", [])]
        ).lower()
        for taboo in taboos:
            if taboo and taboo in haystack:
                violations.append({"taboo": taboo,
                                   "line": line.get("index"),
                                   "text": line.get("text", "")[:120]})
    return {
        "ok": True,
        "id": brand.get("id"),
        "perAct": per_act,
        "grade": brand.get("grade"),
        "sfx": brand.get("sfx"),
        "bgm": brand.get("bgm"),
        "taboos": brand.get("taboos"),
        "violations": violations,
        "violationsNote": ("a taboo in a planned query or caption is a blocker: "
                           "replace the query, never the taboo")
        if violations else "no taboo violations in the supplied lines",
    }


# ---------------------------------------------------------------------------
# The placement gate: does a stock candidate belong in this beat?
# ---------------------------------------------------------------------------

def _candidate_text(candidate):
    parts = [candidate.get(k) for k in ("caption", "keywords", "text", "label", "query")]
    return " ".join(str(p) for p in parts if p)


def gate(spec):
    """Per candidate: three checks (emotion, act, brand). 0-1 fails passes,
    2+ fails rejects the candidate; when every candidate is rejected the
    recommendation is to DROP THE BEAT - never pad it with a bad match."""
    segment = spec.get("segment") or {}
    emotion = str(segment.get("emotion", "neutral")).lower()
    act = str(segment.get("act", "setup")).lower()
    if emotion not in EMOTIONS:
        return {"ok": False, "error": f"segment.emotion must be one of {list(EMOTIONS)}"}
    if act not in ACT_PROFILE:
        return {"ok": False, "error": f"segment.act must be one of {list(ACTS)}"}
    candidates = spec.get("candidates")
    if not isinstance(candidates, list) or not candidates:
        return {"ok": False, "error": "candidates must be a non-empty list (openvid_search / stock_contact_sheet results)"}
    brand = spec.get("brand") if isinstance(spec.get("brand"), dict) else None
    taboos = [str(t).lower() for t in ((brand or {}).get("taboos") or [])]
    brand_errors = brand_validate(brand) if brand else []
    if brand_errors:
        return {"ok": False, "errors": brand_errors}

    # The registers a matching candidate's caption/keywords should come from:
    # the emotion's visual vocabulary plus the emotion's own word stems.
    emotion_register = set()
    for entry in PROFILE[emotion]["vocabulary"]:
        emotion_register.update(entry.split())
    emotion_register.update(t for t in _LEXICON[emotion].split() if len(t) >= 4)
    act_register = {word for entry in ACT_PROFILE[act]["shot"] for word in entry.split()}

    verdicts = []
    for position, candidate in enumerate(candidates):
        if not isinstance(candidate, dict):
            return {"ok": False, "error": f"candidates[{position}] must be an object"}
        blob = _candidate_text(candidate).lower()
        tokens = set(blob.split())
        failed = []
        matched = []
        emotion_hits = tokens & emotion_register
        if emotion_hits:
            matched.append({"check": "emotion", "via": sorted(emotion_hits)[:5]})
        else:
            failed.append("emotion: the caption/keywords carry none of this beat's "
                          f"{emotion} visual register")
        act_hits = tokens & act_register
        if act_hits:
            matched.append({"check": "act", "via": sorted(act_hits)[:5]})
        else:
            failed.append(f"act: no {act} shot semantics "
                          f"({', '.join(ACT_PROFILE[act]['shot'])})")
        taboo_hits = [t for t in taboos if t and t in blob]
        if taboo_hits:
            failed.append(f"brand: taboo {taboo_hits} in the candidate")
        else:
            matched.append({"check": "brand"})
        verdicts.append({
            "index": candidate.get("index", position),
            "id": candidate.get("id"),
            "failed": len(failed),
            "failedChecks": failed,
            "matched": matched,
            "verdict": "pass" if len(failed) <= 1 else "reject",
        })
    passing = [v for v in verdicts if v["verdict"] == "pass"]
    passing.sort(key=lambda v: -len(v["matched"]))
    return {
        "ok": True,
        "segment": {"emotion": emotion, "act": act},
        "candidates": verdicts,
        "order": [v["index"] for v in passing],
        "recommendation": (
            f"use candidate {passing[0]['index']} (ranked by matches)"
            if passing else
            "every candidate failed 2+ checks: DROP THE BEAT - cut the segment, "
            "tighten around it, never pad with a bad match"
        ),
        "note": "the gate is lexical (caption/keywords); confirm the passing "
                "candidate's pixels with stock_contact_sheet before placing it",
    }


# ---------------------------------------------------------------------------
# The export verify: does the render's transcript match the edit?
# ---------------------------------------------------------------------------

def _word_stream(payload):
    """Words from a transcribe job's output: {segments: [{words: [...]}]} or
    a bare word list."""
    if isinstance(payload, dict):
        words = []
        for segment in payload.get("segments") or []:
            if isinstance(segment, dict):
                words.extend(segment.get("words") or [])
        return words
    if isinstance(payload, list):
        return payload
    return []


def verify(spec):
    """Align the expected words (the plan's, from the pre-edit transcript)
    against the exported file's re-transcribed words: what a render loses,
    duplicates or shifts is a defect the eye cannot see."""
    expected = _word_stream(spec.get("expected"))
    actual = _word_stream(spec.get("actual"))
    if not expected or not actual:
        return {"ok": False,
                "error": "expected and actual must each carry words "
                         "([{text,start,end}] or a transcribe job's output)"}
    tolerance = float(spec.get("toleranceMs", 150.0)) / 1000.0

    def key(word):
        # None-safe: the lookahead below hands it the word past the end.
        return _normalise(str(word.get("text", ""))) if isinstance(word, dict) else ""

    # A greedy in-order alignment with a bounded lookahead: transcripts are
    # near-sorted, and an editing defect is a run of missing/duplicated
    # words, not a shuffle. On a mismatch the lookahead decides which side
    # owns the gap: the edit's word appearing ahead in the render means the
    # words between are duplicated render audio; the render's word appearing
    # ahead in the edit means the edit's words in between never made the
    # render. Without the resync, one deletion unmoors the rest and every
    # later word reads as both missing and extra.
    matched = []
    missing = []
    extra = []
    i = j = 0
    lookahead = 4
    while i < len(expected) and j < len(actual):
        if key(expected[i]) == key(actual[j]):
            matched.append((expected[i], actual[j]))
            i += 1
            j += 1
            continue
        if not key(expected[i]):
            i += 1
            continue
        if not key(actual[j]):
            j += 1
            continue
        ahead = next((d for d in range(1, lookahead + 1)
                      if j + d < len(actual) and key(expected[i]) == key(actual[j + d])),
                     None)
        behind = next((d for d in range(1, lookahead + 1)
                       if i + d < len(expected) and key(actual[j]) == key(expected[i + d])),
                      None)
        if ahead is not None and (behind is None or ahead <= behind):
            extra.extend(actual[j:j + ahead])
            j += ahead
        elif behind is not None:
            missing.extend(expected[i:i + behind])
            i += behind
        else:
            missing.append(expected[i])
            extra.append(actual[j])
            i += 1
            j += 1
    missing.extend(w for w in expected[i:] if key(w))
    extra.extend(w for w in actual[j:] if key(w))

    drifts = []
    for want, got in matched:
        try:
            drifts.append(abs(float(got.get("start")) - float(want.get("start"))))
        except (TypeError, ValueError):
            pass
    match_rate = len(matched) / max(1, len(expected))
    max_drift = max(drifts) if drifts else 0.0
    mean_drift = (sum(drifts) / len(drifts)) if drifts else 0.0
    verdict = "pass" if (match_rate >= 0.9 and max_drift <= tolerance
                         and len(extra) <= max(1, int(0.05 * len(expected)))) else "revise"
    problems = []
    if match_rate < 0.9:
        problems.append(f"{len(missing)} of {len(expected)} words are missing from the "
                        "render: the export clipped speech (check clip ends and trims)")
    if max_drift > tolerance:
        problems.append(f"the worst word starts {max_drift * 1000:.0f} ms from where the "
                        "edit placed it: a clip boundary or speed change moved the audio")
    if len(extra) > max(1, int(0.05 * len(expected))):
        problems.append(f"{len(extra)} words appear in the render that the edit never "
                        "placed: duplicated or un-trimmed audio")
    return {
        "ok": True,
        "verdict": verdict,
        "matchRate": round(match_rate, 4),
        "matched": len(matched),
        "missing": len(missing),
        "extra": len(extra),
        "maxDriftMs": round(max_drift * 1000, 1),
        "meanDriftMs": round(mean_drift * 1000, 1),
        "toleranceMs": round(tolerance * 1000, 1),
        "problems": problems,
        "missingWords": [str(w.get("text", "")) for w in missing[:20]],
    }


# ---------------------------------------------------------------------------
# Selftest: deterministic, no fixtures, no network.
# ---------------------------------------------------------------------------

def selftest():
    out = {}

    def extract_lines(script, **kwargs):
        spec = dict(script=script, **kwargs)
        return extract(spec)

    # The engine core: a surprised line, an angry line, a sad line, a joyful
    # line, all recognised from the words alone.
    report = extract_lines(
        "You won't believe what they found down there. "
        "This is outrageous, and they knew all along. "
        "I miss you every single day since you left. "
        "The whole town came out to celebrate, and everyone was smiling. "
        "First, the rain dissolves the rock.")
    lines = report["lines"]
    assert lines[0]["emotion"] == "surprise", lines[0]
    assert lines[1]["emotion"] == "anger", lines[1]
    assert lines[2]["emotion"] == "sadness", lines[2]
    assert lines[3]["emotion"] == "joy", lines[3]
    assert report["engine"] == "lexicon" and report["confidence"] == 0.55, report["engine"]
    out["emotions"] = "ok"

    # Intensity: caps and bangs beat a hedged, trailed-off read.
    loud = extract_lines("WOW, they actually did it!!")
    quiet = extract_lines("hmm, maybe they sort of did it...")
    assert loud["lines"][0]["intensity"] > quiet["lines"][0]["intensity"], \
        (loud["lines"][0]["intensity"], quiet["lines"][0]["intensity"])
    out["intensity"] = "ok"

    # Negation: "never loved" must not read as joy.
    negated = _lexicon_scores("he never loved any of it")
    assert negated["joy"] == 0.0 and negated["sadness"] > 0, negated
    out["negation"] = "ok"

    # Acts: hook by position, reveal by marker, cta by marker.
    acts = extract_lines(
        "Every drop of the ocean was a mountain first. "
        "To understand that, imagine rain falling on stone. "
        "But rivers carry the dissolved rock to the sea. "
        "Turns out the salt was there all along. "
        "Studies measured every ion in the basin. "
        "Subscribe for one fact like this every week.")
    assert [line["act"] for line in acts["lines"]] == \
        ["hook", "setup", "turn", "reveal", "proof", "cta"], \
        [line["act"] for line in acts["lines"]]
    out["acts"] = "ok"

    # Hormones: a burst of fear is adrenaline, a joyful reveal is dopamine,
    # tender sadness is oxytocin, calm joy is serotonin.
    fear = extract_lines("The whole building is about to collapse, run NOW!!")
    assert fear["lines"][0]["hormone"] == "adrenaline", fear["lines"][0]
    reveal = extract_lines("Turns out the treasure was joy all along, and we found it.")
    assert reveal["lines"][0]["hormone"] == "dopamine", reveal["lines"][0]
    tender = extract_lines("I miss you, and I always will, quietly.")
    assert tender["lines"][0]["hormone"] == "oxytocin", tender["lines"][0]
    calm = extract_lines("A calm, grateful morning with warm tea.")
    assert calm["lines"][0]["hormone"] == "serotonin", calm["lines"][0]
    out["hormones"] = "ok"

    # Easing: the user's law - tense emotions ease in, warm ones ease out,
    # a surprise pops. And the JSON is the authoring layer's own shape.
    assert PROFILE["anger"]["easing"] == "snap-in"
    assert PROFILE["fear"]["easing"] == "ease-in"
    assert PROFILE["joy"]["easing"] == "ease-out"
    assert _EASINGS["ease-in"] == {"kind": "bezier", "x1": 0.42, "y1": 0.0,
                                   "x2": 1.0, "y2": 1.0}
    burst = extract_lines("NO WAY, they actually found it!!")
    assert burst["lines"][0]["easing"]["name"] == "spring-pop", burst["lines"][0]["easing"]
    out["easing"] = "ok"

    # The arc: a flat script warns; a moving script scores higher.
    flat = extract_lines("The report lists the numbers. The table shows the "
                         "values. The appendix has the notes. The data is "
                         "in the file. The summary follows.")
    assert flat["arc"]["arcShape"] == "flat" and flat["arc"]["warnings"], flat["arc"]
    moving = extract_lines(
        "You won't believe what was hiding in the data. "
        "At first, the numbers looked boring. "
        "But then the pattern turned dark, and the team got scared. "
        "Turns out the answer was beautiful, and everyone celebrated. "
        "Subscribe if that made you smile.")
    assert moving["arc"]["arcScore"] > flat["arc"]["arcScore"], \
        (moving["arc"]["arcScore"], flat["arc"]["arcScore"])
    out["arc"] = "ok"

    # The brand template travels with the binary: a validation error can
    # point at it, and a director can start from it in the same session.
    template = brand_template()
    assert template["ok"] and template["template"]["palette"]["accent"] == "#4CC9F0", template
    assert not brand_validate(template["template"]), brand_validate(template["template"])
    out["template"] = "ok"

    # Brand: a broken guide errors; a taboo in a planned query is a violation.
    broken = brand_apply({"id": "x"}, [])
    assert not broken["ok"] and broken["errors"], broken
    brand = {
        "id": "science-shorts", "name": "Science Shorts",
        "vibe": "bright conspiratorial science",
        "palette": {"background": "#0B1220", "ink": "#F5F7FA", "accent": "#4CC9F0"},
        "type": {"family": "Archivo", "weight": 700},
        "grade": {"base": "concat.teal-orange", "amount": 25},
        "sfx": {"palette": ["impact-*", "whoosh-*"]},
        "bgm": {"genres": ["ambient electronic"], "duckDb": -12},
        "taboos": ["lens flare", "beaker stock footage"],
    }
    applied = brand_apply(brand, [
        {"index": 0, "text": "the lab explodes with light",
         "queries": ["lens flare abstract", "clean macro"]},
    ])
    assert applied["ok"] and applied["violations"], applied
    assert any(v["taboo"] == "lens flare" for v in applied["violations"]), applied
    out["brand"] = "ok"

    # The gate: matching candidate passes, a mismatched one is rejected, and
    # when everything fails the beat is dropped, never padded.
    verdict = gate({
        "segment": {"emotion": "joy", "act": "reveal"},
        "brand": brand,
        "candidates": [
            {"index": 0, "id": "a1", "caption": "sunlit meadow celebration sunrise warm"},
            {"index": 1, "id": "a2", "caption": "dark abandoned foggy corridor alone"},
            {"index": 2, "id": "a3", "caption": "lens flare over a stock beaker"},
        ],
    })
    assert verdict["ok"], verdict
    by_index = {v["index"]: v for v in verdict["candidates"]}
    assert by_index[0]["verdict"] == "pass", by_index[0]
    assert by_index[1]["verdict"] == "reject" and by_index[1]["failed"] >= 2, by_index[1]
    assert by_index[2]["verdict"] == "reject" and by_index[2]["failed"] >= 2, by_index[2]
    assert verdict["order"] and verdict["order"][0] == 0, verdict["order"]
    hopeless = gate({
        "segment": {"emotion": "joy", "act": "reveal"},
        "candidates": [
            {"index": 0, "caption": "dark foggy corridor"},
            {"index": 1, "caption": "grimy sludge texture"},
        ],
    })
    assert "DROP THE BEAT" in hopeless["recommendation"], hopeless["recommendation"]
    out["gate"] = "ok"

    # Verify: aligned words pass with drift, lost words fail.
    words = lambda texts, base: [
        {"text": t, "start": base + 0.1 * i, "end": base + 0.1 * i + 0.08}
        for i, t in enumerate(texts)]
    expected = words("the ocean keeps the salt when the water leaves".split(), 0.0)
    shifted = words("the ocean keeps the salt when the water leaves".split(), 0.04)
    good = verify({"expected": expected, "actual": shifted})
    assert good["verdict"] == "pass" and good["matchRate"] == 1.0, good
    assert 35.0 <= good["maxDriftMs"] <= 45.0, good["maxDriftMs"]
    hole = [w for w in expected]
    del hole[3:6]  # the render lost three words
    bad = verify({"expected": expected, "actual": hole})
    assert bad["verdict"] == "revise" and bad["missing"] == 3 and bad["problems"], bad
    out["verify"] = "ok"

    # The model+lexicon merge is pure arithmetic and must stay honest about
    # weights: the model is the read, the lexicon the marginal signal.
    merged = _merge_scores({"joy": 1.0}, {"sadness": 1.0})
    assert abs(merged["joy"] - 0.75) < 1e-9 and abs(merged["sadness"] - 0.25) < 1e-9, merged
    out["merge"] = "ok"

    # The hosted model emits eleven labels; the edit speaks seven. The fold
    # is a mapping, not a guess: contempt rides disgust, frustration rides
    # anger, gratitude and love ride joy - and love plus joy (the multi-label
    # case, where scores need not sum to 1) ADDS, so a tender line reads
    # more joyful than a merely happy one, which it is.
    assert DEFAULT_MODEL == "tabularisai/multilingual-emotion-classification"
    assert set(MODEL_LABELS) == set(MODEL_LABEL_MAP)
    assert set(MODEL_LABEL_MAP.values()) <= set(EMOTIONS)
    folded = _fold_model_scores([
        {"label": "joy", "score": 0.6},
        {"label": "love", "score": 0.4},
        {"label": "contempt", "score": 0.2},
        {"label": "anger", "score": 0.1},
    ])
    assert abs(folded["joy"] - 1.0) < 1e-9, folded
    assert abs(folded["disgust"] - 0.2) < 1e-9 and abs(folded["anger"] - 0.1) < 1e-9, folded
    assert folded["neutral"] == 0.0 and folded["surprise"] == 0.0, folded
    # A flat single-dict answer (one line, some deployments) folds too.
    flat = _fold_model_scores({"label": "sadness", "score": 0.9})
    assert abs(flat["sadness"] - 0.9) < 1e-9, flat
    # Labels the map does not know are ignored, never fatal.
    unknown = _fold_model_scores([{"label": "LABEL_7", "score": 1.0}])
    assert all(value == 0.0 for value in unknown.values()), unknown
    out["fold"] = "ok"

    # The multilingual honesty: an English script is fine for the lexicon,
    # Bengali is not - the note must say so rather than answering neutral.
    assert _latin_share(["The rain dissolves the rock."]) > 0.9
    assert _latin_share(["তুমি কোথায় যাচ্ছ আজ রাতে?"]) < 0.1
    assert _latin_share(["¡Qué alegría verles, señoras y señores!"]) < 0.9
    report = extract({"script": "তোমার জন্য ভালোবাসা। আজ খুব খুশি লাগছে।"})
    assert report["engine"] == "lexicon" and "multilingual" in report["note"], report["note"]
    assert report["lines"][0]["emotion"] == "neutral", report["lines"][0]
    out["multilingual"] = "ok"

    out["selftest"] = True
    return out


# ---------------------------------------------------------------------------
# CLI: the same --spec contract as sense.py.
# ---------------------------------------------------------------------------

def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("cmd", choices=["extract", "brand", "gate", "verify", "selftest", "template"])
    parser.add_argument("--spec", default="{}")
    args = parser.parse_args()
    try:
        raw = sys.stdin.read() if args.spec == "-" else args.spec
        spec = json.loads(raw)
        if not isinstance(spec, dict):
            raise ValueError("spec must be a JSON object")
        if args.cmd == "extract":
            out = extract(spec)
        elif args.cmd == "brand":
            if spec.get("action") == "template" or spec.get("template"):
                out = brand_template()
            else:
                out = brand_apply(spec.get("brand") or spec, spec.get("lines") or [])
        elif args.cmd == "gate":
            out = gate(spec)
        elif args.cmd == "verify":
            out = verify(spec)
        elif args.cmd == "template":
            out = brand_template()
        else:
            out = selftest()
    except Exception as exc:
        out = {"ok": False, "error": f"{type(exc).__name__}: {exc}"}
    print(json.dumps(out, ensure_ascii=False, separators=(",", ":")), flush=True)


if __name__ == "__main__":
    main()
