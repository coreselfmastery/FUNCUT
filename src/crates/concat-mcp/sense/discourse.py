#!/usr/bin/env python3
"""concat-discourse: the script read as language, not just as sound.

Pure Python standard library, same contract as the other sense scripts:
`discourse.py <extract|selftest> --spec -` reads one JSON spec from stdin
(or argv) and prints one JSON object on stdout. The spec names the tool it
wants in `tool`; sense.py calls this module once per family.

THE TEN TOOLS THIS MODULE SERVES

  analyze_discourse         connectives by relation, given-new overlap,
                             signposting, cohesion profile
  segment_topics            lexical-cohesion tiling: where one topic ends
                             and the next begins
  detect_speech_acts        the surface act: question, imperative, promise,
                             warning, quote, conditional...
  classify_speech_acts      the deep act, Searle's five: assertive,
                             directive, commissive, expressive, declaration
  score_readability         Flesch, Flesch-Kincaid, Gunning Fog, passive
                             voice, nominalizations - and the caption pace
                             the grade implies
  extract_claim_graph       claims, evidence, hedges, counters - the graph
                             a fact-checked edit is built on
  score_script_quality      hook, structure, clarity, rhythm, payoff,
                             concision - scored, with the fixes named
  detect_sarcasm            text cues first, and the pitch/energy
                             incongruity cross-check when audio is given
  detect_code_switching     script mixing per line, switch points, the
                             fonts and voices a mixed take needs
  detect_cross_lingual_emotion  the offline emotion read for Bengali,
                             Hindi, Spanish, French and romanized Bengali -
                             the languages the English lexicon cannot hear

Everything is a measurement or a named heuristic: each reading says which
cues fired, and a line with no cues gets "neutral", never a guess.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
import unicodedata
from pathlib import Path

try:  # the runtime directory holds every sense script; standalone too
    import voice_signals as _vs
except ImportError:  # pragma: no cover - direct `python discourse.py` runs
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import voice_signals as _vs

try:
    import emotion as _emo
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    try:
        import emotion as _emo
    except Exception:  # pragma: no cover
        _emo = None

SCHEMA = "funcut.discourse/1"

# ---------------------------------------------------------------------------
# Shared reading helpers
# ---------------------------------------------------------------------------


def _lines(spec):
    """Lines from transcript / lines / script, same order as voice_signals."""
    return _vs._collect_lines(spec)


def _words(line):
    return _vs._words_of(line)


_WORD_RE = re.compile(r"[\w'’\-]+", re.UNICODE)


def _tokens(text):
    """Lowercased word tokens, punctuation out, contractions kept."""
    return _WORD_RE.findall(str(text or "").lower())


_STOP = set("""
a an the and or but if then else of to in on at for with from by as is are
was were be been being am do does did done have has had having i you he she
it we they me him her them my your his its our their this that these those
there here what which who whom when where why how not no nor so too very
can will just should now about into over after before under above out off
up down again further once all any both each few more most other some such
only own same than s t don ll re ve d m o y ain aren couldn didn doesn
hadn hasn haven isn ma mightn mustn needn shan shouldn wasn weren won
wouldn
""".split())


def _content_tokens(tokens):
    return [t for t in tokens if t not in _STOP and len(t) > 2]


def _line_time(line, default=None):
    start = line.get("start")
    end = line.get("end")
    return start, end


def _ok(tool, **fields):
    out = {"ok": True, "schema": SCHEMA, "tool": tool}
    out.update(fields)
    return out


# ---------------------------------------------------------------------------
# 1. analyze_discourse - connectives, given-new, signposting
# ---------------------------------------------------------------------------

# Relation classes follow the discourse tradition (comparison, contingency,
# temporal, expansion - the same families the Penn Discourse Treebank uses),
# plus the signposting class a narration-heavy script leans on.
_MARKERS = {
    "comparison": """
        but however yet whereas while whilst instead instead-contrast
        nevertheless nonetheless conversely rather although though even-though
        despite in-spite unlike on-the-other-hand meanwhile by-contrast
    """,
    "contingency": """
        because since so therefore thus hence consequently as-a-result
        accordingly that's-why thats-why which-is-why leads-to resulted-in
        due-to owing-to for-this-reason otherwise
    """,
    "temporal": """
        then next after before earlier later meanwhile afterwards finally
        eventually subsequently at-first at-last once when whenever until
        soon later-on in-the-end from-there afterwards first second third
    """,
    "expansion": """
        and also moreover plus additionally furthermore in-addition
        what's-more whats-more besides similarly likewise equally on-top
        not-only another
    """,
    "signposting": """
        here's heres look listen imagine consider picture-this think-about
        remember keep-in-mind the-point is-this the-truth bottom-line
        in-this-video today we're-going were-going let's-talk lets-talk
        quick-recap to-be-clear make-no-mistake spoiler turn-out turns-out
    """,
}

_MARKER_INDEX = {}
for _relation, _blob in _MARKERS.items():
    for _phrase in _blob.split():
        _MARKER_INDEX.setdefault(_phrase.replace("-", " "), _relation)


def _phrase_stream(tokens):
    """The token stream as unigrams and bigrams, so 'on the other hand'
    matches its three-word phrase. Phrases are indexed space-joined."""
    out = []
    for i, tok in enumerate(tokens):
        out.append(tok)
        if i + 1 < len(tokens):
            out.append(tok + " " + tokens[i + 1])
        if i + 2 < len(tokens):
            out.append(tok + " " + tokens[i + 1] + " " + tokens[i + 2])
    return out


def _jaccard(a, b):
    if not a or not b:
        return 0.0
    shared = len(a & b)
    return shared / (len(a) + len(b) - shared)


def _analyze_discourse(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}

    total_words = 0
    per_line = []
    prev_content = None
    transitions = []
    for idx, line in enumerate(lines):
        tokens = _tokens(line.get("text"))
        content = set(_content_tokens(tokens))
        total_words += len(tokens)
        markers = []
        seen_spans = []
        for phrase in _phrase_stream(tokens):
            relation = _MARKER_INDEX.get(phrase)
            if not relation:
                continue
            # a phrase that starts inside one already found is the same cue
            if any(phrase.split()[0] in s.split() and s in phrase for s in seen_spans):
                continue
            seen_spans.append(phrase)
            markers.append({
                "phrase": phrase,
                "relation": relation,
                "position": "opening" if tokens.index(phrase.split()[0]) < 2 else "mid",
            })
        overlap = _jaccard(prev_content, content) if prev_content is not None else None
        abrupt = overlap is not None and overlap < 0.05 and not any(
            m["relation"] in ("comparison", "contingency", "temporal") for m in markers
        )
        per_line.append({
            "line": idx,
            "text": str(line.get("text") or "")[:160],
            "start": line.get("start"),
            "end": line.get("end"),
            "markers": markers,
            "givenNewOverlap": round(overlap, 3) if overlap is not None else None,
            "abruptTransition": abrupt,
        })
        if overlap is not None:
            transitions.append({"between": [idx - 1, idx], "overlap": round(overlap, 3),
                                "abrupt": abrupt})
        prev_content = content

    marker_count = sum(len(p["markers"]) for p in per_line)
    density = round(100.0 * marker_count / max(1, total_words), 2)
    by_relation = {}
    for p in per_line:
        for m in p["markers"]:
            by_relation[m["relation"]] = by_relation.get(m["relation"], 0) + 1
    abrupts = [t for t in transitions if t["abrupt"]]
    return _ok(
        "analyze_discourse",
        lines=len(lines),
        words=total_words,
        cohesion={
            "markerDensityPer100Words": density,
            "markersByRelation": by_relation,
            "givenNewMeanOverlap": round(
                sum(t["overlap"] for t in transitions) / max(1, len(transitions)), 3)
            if transitions else None,
            "reading": ("well-connected narration" if density >= 2.0 and len(abrupts) <= max(1, len(lines) // 8)
                        else "loose narration - see abruptTransitions"),
        },
        signposting=[p for p in per_line if any(m["relation"] == "signposting" for m in p["markers"])],
        perLine=per_line,
        transitions=transitions,
        abruptTransitions=abrupts,
        directorHints=[
            "a comparison marker (but / however) is the emotional pivot - the visual should turn with it",
            "each abruptTransition is a place a chapter card, a scene change or a connective line earns its keep",
            "signposting lines are the retention beats - keep their pauses, give them emphasis visuals",
        ] if abrupts or density < 2.0 else ["the narration is well-connected; no connective repairs needed"],
        source="python (marker lexicon, Jaccard given-new overlap)",
    )


# ---------------------------------------------------------------------------
# 2. segment_topics - lexical-cohesion tiling
# ---------------------------------------------------------------------------


def _segment_topics(spec):
    lines = _lines(spec)
    if len(lines) < 3:
        return {"ok": False, "error": "topic segmentation needs at least 3 lines"}

    # Per-line content sets, then a similarity series between adjacent
    # sliding windows (the TextTiling reading: a boundary is a valley).
    sets = [set(_content_tokens(_tokens(line.get("text")))) for line in lines]
    width = 2
    sims = []
    for i in range(1, len(lines)):
        left = set().union(*sets[max(0, i - width):i]) if any(sets[max(0, i - width):i]) else set()
        right = set().union(*sets[i:i + width]) if any(sets[i:i + width]) else set()
        sims.append(_jaccard(left, right))

    # Depth score: how far each valley sits below its neighbouring peaks.
    bounds = []
    for i in range(1, len(sims) - 1):
        if sims[i] < sims[i - 1] and sims[i] <= sims[i + 1]:
            left_peak = max(sims[max(0, i - 3):i + 1] or [0])
            right_peak = max(sims[i + 1:i + 4] or [0])
            depth = (left_peak - sims[i]) + (right_peak - sims[i])
            bounds.append((i + 1, depth))  # boundary sits BETWEEN line i and i+1 (1-based line idx of second side)

    max_depth = max((d for _, d in bounds), default=0.0)
    threshold = max(0.06, 0.35 * max_depth)
    wanted = int(spec.get("maxSegments") or 12)
    chosen = sorted([b for b in bounds if b[1] >= threshold], key=lambda b: -b[1])[:max(1, wanted)]
    chosen.sort()

    # Global term counts for the TF-without-global trick.
    from collections import Counter
    global_counts = Counter()
    for s in sets:
        global_counts.update(s)

    segments = []
    edges = [0] + [b[0] for b in chosen] + [len(lines)]
    for n in range(len(edges) - 1):
        lo, hi = edges[n], edges[n + 1]
        local = Counter()
        for s in sets[lo:hi]:
            local.update(s)
        keywords = [w for w, _ in local.most_common(8) if global_counts[w] < max(2, hi - lo)]
        keywords = keywords or [w for w, _ in local.most_common(5)]
        start_t = lines[lo].get("start")
        end_t = lines[hi - 1].get("end")
        segments.append({
            "index": n,
            "startLine": lo,
            "endLine": hi - 1,
            "start": start_t,
            "end": end_t,
            "keywords": keywords[:6],
            "headline": " ".join(k.capitalize() for k in keywords[:3]) if keywords else f"segment {n + 1}",
            "cohesion": round(sum(sims[lo:hi - 1]) / max(1, hi - 1 - lo), 3) if hi - 1 > lo else None,
        })
    return _ok(
        "segment_topics",
        lines=len(lines),
        boundaries=[{"afterLine": b[0] - 1, "depth": round(b[1], 3)} for b in chosen],
        segments=segments,
        directorHints=[
            "each segment is one visual chapter: its keywords are the b-roll shot list",
            "a boundary is a scene change or a chapter card - cut there, not mid-segment",
            "a segment with low cohesion mixes two ideas; consider splitting its visuals",
        ],
        source="python (sliding-window lexical cohesion, valley boundaries)",
    )


# ---------------------------------------------------------------------------
# 3. detect_speech_acts - the surface act
# ---------------------------------------------------------------------------

_ACT_CUES = [
    ("question", [r"\?$", r"^(who|what|when|where|why|how|which|whose|whom)\b",
                  r"^(do|does|did|is|are|was|were|can|could|will|would|should|have|has|am)\b.*\?$",
                  r"^(Have you ever|Did you know|What if|Why do)"]),
    ("rhetorical_question", [r"\?$", r"(ever wonder|have you ever|why do you think|who cares|guess what)"]),
    ("exclamation", [r"!$"]),
    ("imperative", [r"^(look|listen|imagine|consider|picture|think|watch|see|notice|remember|stop|wait|try|take|put|keep|hold|give|let|don't|do not)\b"]),
    ("promise", [r"\b(i|we)['’]?(ll| will| are going to|'re going to)\b", r"\b(i promise|we promise|guarantee)\b"]),
    ("warning", [r"\b(careful|watch out|be careful|don't fall for|make sure|before you|warning|risk|danger)\b"]),
    ("invitation", [r"\b(let's|lets|join|come along|stick with me|follow along|try it)\b"]),
    ("quote", [r"\b(said|says|told|according to|put it)\b", r'[""„「『]']),
    ("conditional", [r"\bif\b", r"\bif .*, (?:then|you)\b", r"\bwhat if\b"]),
    ("negation", [r"\b(never|not|no one|nobody|nothing|none|isn't|wasn't|don't|doesn't|didn't|won't|can't)\b"]),
    ("enumeration", [r"\b(first|second|third|finally|number (one|two|three)|three things|two reasons)\b"]),
    ("superlative_claim", [r"\b(biggest|smallest|fastest|slowest|best|worst|most|least|never been|first ever)\b"]),
]


def _detect_speech_acts(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    per_line = []
    counts = {}
    for idx, line in enumerate(lines):
        text = str(line.get("text") or "").strip()
        low = " " + text.lower()
        acts = []
        for act, patterns in _ACT_CUES:
            for pat in patterns:
                if re.search(pat, low):
                    acts.append(act)
                    break
        # a question that also names an answer cue later in the same line
        # is rhetorical; a bare question is a real one.
        if "question" in acts and "rhetorical_question" in acts:
            acts.remove("question")
        primary = acts[0] if acts else "statement"
        counts[primary] = counts.get(primary, 0) + 1
        per_line.append({
            "line": idx,
            "text": text[:160],
            "start": line.get("start"),
            "end": line.get("end"),
            "acts": acts or ["statement"],
            "primary": primary,
        })
    questions = [p for p in per_line if p["primary"] in ("question", "rhetorical_question")]
    return _ok(
        "detect_speech_acts",
        lines=len(lines),
        counts=counts,
        perLine=per_line,
        questions=questions,
        directorHints=[
            "a question line deserves a visual that answers late - hold the reveal one beat past the answer",
            "an imperative is the eye-contact moment: direct address caption, tighter frame",
            "a quote line is an attribution lower-third waiting to happen",
        ] if questions else ["mostly declarative narration; visual variety must come from the imagery, not the acts"],
        source="python (punctuation + syntactic cue patterns)",
    )


# ---------------------------------------------------------------------------
# 4. classify_speech_acts - Searle's five
# ---------------------------------------------------------------------------

_SEARLE_CUES = {
    # directive: the words try to make the viewer do something
    "directive": [
        r"^(look|listen|imagine|consider|watch|try|take|put|keep|hold|give|let's|lets|don't|do not|stop|wait|see)\b",
        r"\b(you should|you need to|you have to|you must|make sure|go|click|subscribe|follow|comment|share|link in)\b",
        r"\?$",
    ],
    # commissive: the speaker commits to something
    "commissive": [
        r"\b(i|we)['’]?(ll| will)\b", r"\b(i|we)'?(re| are) going to\b",
        r"\b(i promise|we promise|guarantee|i'll show|we'll see|i am going to)\b",
    ],
    # expressive: the speaker expresses a psychological state
    "expressive": [
        r"\b(i (love|hate|like|adore|miss|fear|can't stand)|i'm (so |really )?(happy|sorry|excited|scared|angry|amazed))\b",
        r"^(wow|whoa|oh my|god|damn|incredible|amazing|unbelievable)\b", r"!$",
    ],
    # declaration: the words change the world by being said
    "declaration": [
        r"\b(i (hereby|now) (declare|name|pronounce)|we're (naming|calling|launching)|this makes it official|i resign)\b",
    ],
    # assertive: the default - the words claim something about the world
    "assertive": [],
}

_ACT_TREATMENT = {
    "assertive": "b-roll that shows the claim; a stat card when numbers ride it",
    "directive": "CTA card or direct-address caption; the moment to ask for the click",
    "commissive": "a promise is a setup for a payoff - plant a visual callback",
    "expressive": "reaction shot, breather, or grade shift; the emotional beat itself",
    "declaration": "title card / full-frame text: the world just changed",
}


def _classify_speech_acts(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    per_line = []
    counts = {}
    for idx, line in enumerate(lines):
        text = str(line.get("text") or "").strip()
        low = " " + text.lower()
        scored = []
        for cls, patterns in _SEARLE_CUES.items():
            cues = [p for p in patterns if re.search(p, low)]
            if cues:
                scored.append((cls, cues))
        if not scored:
            cls, cues = "assertive", ["declarative syntax, no special cue"]
        else:
            # order matters: the rarer, more specific classes outrank the default
            priority = ["declaration", "commissive", "expressive", "directive"]
            scored.sort(key=lambda sc: priority.index(sc[0]) if sc[0] in priority else 9)
            cls, cues = scored[0]
        counts[cls] = counts.get(cls, 0) + 1
        per_line.append({
            "line": idx,
            "text": text[:160],
            "start": line.get("start"),
            "end": line.get("end"),
            "act": cls,
            "cues": [c if len(str(c)) < 60 else str(c)[:57] + "..." for c in cues[:3]],
            "visual": _ACT_TREATMENT[cls],
            "confidence": 0.6 if cls == "assertive" and not scored else 0.75,
        })
    return _ok(
        "classify_speech_acts",
        lines=len(lines),
        counts=counts,
        perLine=per_line,
        balance={cls: round(counts.get(cls, 0) / len(lines), 3) for cls in _SEARLE_CUES},
        directorHints=[
            "a script that is all assertives is a lecture: trade one line in five for a directive or expressive",
            "the CTA only lands on a directive; check the last 10% of lines for one",
        ],
        source="python (Searle taxonomy over syntactic cues)",
    )


# ---------------------------------------------------------------------------
# 5. score_readability
# ---------------------------------------------------------------------------


def _script_kind(text):
    """The dominant Unicode script of a text: latin, bengali, devanagari,
    arabic, cyrillic, cjk, hangul... 'mixed' when two share it."""
    counts = {}
    for ch in str(text or ""):
        if not ch.isalpha():
            continue
        try:
            name = unicodedata.name(ch)
        except ValueError:
            continue
        for script in ("LATIN", "BENGALI", "DEVANAGARI", "ARABIC", "CYRILLIC",
                       "CJK", "HANGUL", "HIRAGANA", "KATAKANA", "THAI", "TAMIL",
                       "TELUGU", "GUJARATI", "GEORGIAN", "GREEK", "HEBREW"):
            if script in name:
                counts[script.lower()] = counts.get(script.lower(), 0) + 1
                break
    if not counts:
        return "latin"
    ranked = sorted(counts.items(), key=lambda kv: -kv[1])
    if len(ranked) > 1 and ranked[1][1] >= 0.25 * ranked[0][1]:
        return "mixed:" + "+".join(r[0] for r in ranked[:2])
    return ranked[0][0]


def _score_readability(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    full_text = "\n".join(str(line.get("text") or "") for line in lines)
    kind = _script_kind(full_text)
    latin = kind in ("latin",) or kind.startswith("mixed:latin")

    words_per_line = []
    sylls_per_line = []
    for line in lines:
        toks = _tokens(line.get("text"))
        words_per_line.append(len(toks))
        if latin:
            sylls_per_line.append(sum(_vs._syllables(t) for t in toks))
    sentences = [l for l in lines if str(l.get("text") or "").strip()]
    total_words = sum(words_per_line) or 1
    total_sylls = sum(sylls_per_line) or 0

    # Passive voice: a be-verb followed by a past participle - a heuristic
    # the fog index tradition accepts as a flag, not a grammar verdict.
    passive = 0
    nominal = 0
    if latin:
        be = re.compile(r"\b(is|are|was|were|be|been|being|am)\b")
        participle = re.compile(r"\b\w+(ed|en)\b")
        nom = re.compile(r"\b\w+(tion|ment|ance|ence|ity|ness|ism)\b")
        for line in sentences:
            text = " " + str(line.get("text") or "").lower() + " "
            for m in be.finditer(text):
                tail = text[m.end():m.end() + 40]
                if participle.match(tail.strip()):
                    passive += 1
                    break
            nominal += len(nom.findall(text))

    wps = total_words / max(1, len(sentences))
    spw = total_sylls / total_words if latin else None
    if latin:
        flesch = 206.835 - 1.015 * wps - 84.6 * spw
        fk_grade = 0.39 * wps + 11.8 * spw - 15.59
        complex_ratio = (sum(1 for line in lines for t in _tokens(line.get("text"))
                              if _vs._syllables(t) >= 3) / total_words)
        fog = 0.4 * (wps + 100.0 * complex_ratio)
        grades = {
            "fleschReadingEase": round(flesch, 1),
            "fleschKincaidGrade": round(fk_grade, 1),
            "gunningFog": round(fog, 1),
            "readingLevel": ("very easy" if flesch >= 80 else "easy" if flesch >= 60 else
                             "plain" if flesch >= 50 else "fairly hard" if flesch >= 30 else "difficult"),
        }
        # Captions read slower than prose: the CPS budget a grade can carry.
        if flesch >= 70:
            cps = 20
        elif flesch >= 55:
            cps = 17
        elif flesch >= 40:
            cps = 14
        else:
            cps = 11
    else:
        grades = {"readingLevel": "n/a (non-Latin script)",
                  "note": "grade formulas are Latin-alphabet instruments; "
                          "word and line stats below still answer"}
        avg_len = sum(len(_tokens(l.get("text"))) for l in lines) / max(1, len(lines))
        cps = 15 if avg_len <= 9 else 12

    return _ok(
        "score_readability",
        script=kind,
        stats={
            "lines": len(lines),
            "words": total_words,
            "wordsPerSentence": round(wps, 2),
            "syllablesPerWord": round(spw, 3) if spw else None,
            "longestLineWords": max(words_per_line) if words_per_line else 0,
            "passiveVoiceHits": passive,
            "passivePer100Words": round(100.0 * passive / total_words, 2) if latin else None,
            "nominalizations": nominal if latin else None,
        },
        grades=grades,
        captionPacing={
            "charsPerSecondBudget": cps,
            "note": "captions read ~25% slower than prose; long lines need splitting, not speeding",
        },
        perLine=[{"line": i, "words": w} for i, w in enumerate(words_per_line)],
        directorHints=[
            "passive constructions hide the doer - an active rewrite is free clarity",
            "nominalizations ('implementation' vs 'we did it') deaden narration; cut where dense",
            f"caption lines over {cps} cps at reading pace will be skipped, not read",
        ],
        source="python (Flesch, Flesch-Kincaid, Gunning Fog; passive/nominalization regex)",
    )


# ---------------------------------------------------------------------------
# 6. extract_claim_graph
# ---------------------------------------------------------------------------

_EVIDENCE_CUES = re.compile(
    r"\b(?:because|since|according to|studies show|research|data|survey|numbers|"
    r"report(?:ed|s)?|found that|shows that|measured|records|documents|proof|"
    r"evidence|percent|%|\$|million|billion|thousand|stats)\b", re.IGNORECASE)
_HEDGE_CUES = re.compile(
    r"\b(might|could|may|possibly|perhaps|seems|appears|i think|probably|"
    r"maybe|kind of|sort of|allegedly|reportedly)\b", re.IGNORECASE)
_COUNTER_CUES = re.compile(
    r"\b(but|however|on the other hand|yet|contrary|critics|skeptics|"
    r"naysayers|pushback)\b", re.IGNORECASE)
_ELABORATION_CUES = re.compile(
    r"\b(which means|in other words|that is|meaning|put simply|so what)\b", re.IGNORECASE)


def _extract_claim_graph(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    nodes = []
    edges = []
    last_claim = None
    for idx, line in enumerate(lines):
        text = str(line.get("text") or "").strip()
        if not text:
            continue
        low = " " + text.lower() + " "
        evidence = _EVIDENCE_CUES.findall(low)
        hedges = _HEDGE_CUES.findall(low)
        counters = _COUNTER_CUES.findall(low)
        elaborates = _ELABORATION_CUES.findall(low)
        # A claim: an assertive line without hedging; strength from its evidence.
        is_claim = not hedges and len(_tokens(text)) >= 4
        node = None
        if is_claim:
            node = {
                "id": f"c{len(nodes)}",
                "kind": "claim",
                "line": idx,
                "text": text[:200],
                "start": line.get("start"),
                "end": line.get("end"),
                "strength": (1.0 if evidence else 0.55) - 0.2 * len(counters),
                "evidenceCues": sorted(set(c.lower() for c in evidence))[:4],
                "hedges": [],
            }
            node["strength"] = round(max(0.1, node["strength"]), 2)
            nodes.append(node)
            if last_claim is not None:
                edges.append({"from": last_claim["id"], "to": node["id"],
                              "relation": "follows"})
        elif evidence and last_claim is not None:
            node = {
                "id": f"e{len(nodes)}",
                "kind": "evidence",
                "line": idx,
                "text": text[:200],
                "start": line.get("start"),
                "end": line.get("end"),
                "cues": sorted(set(c.lower() for c in evidence))[:4],
            }
            nodes.append(node)
            edges.append({"from": last_claim["id"], "to": node["id"],
                          "relation": "supported_by"})
        elif hedges:
            node = {
                "id": f"h{len(nodes)}",
                "kind": "hedged",
                "line": idx,
                "text": text[:200],
                "start": line.get("start"),
                "end": line.get("end"),
                "hedges": sorted(set(h.lower() for h in hedges))[:4],
            }
            nodes.append(node)
        if counters and last_claim is not None and node and node["kind"] == "claim":
            edges.append({"from": last_claim["id"], "to": node["id"],
                          "relation": "countered_by", "cues": [c.lower() for c in counters[:2]]})
        if elaborates and last_claim is not None and node and node["id"] != last_claim["id"]:
            edges.append({"from": last_claim["id"], "to": node["id"],
                          "relation": "elaborates"})
        if is_claim:
            last_claim = node

    claims = [n for n in nodes if n["kind"] == "claim"]
    supported = {e["from"] for e in edges if e["relation"] == "supported_by"}
    uncovered = [n for n in claims if n["id"] not in supported and not n["evidenceCues"]]
    return _ok(
        "extract_claim_graph",
        nodes=nodes,
        edges=edges,
        summary={
            "claims": len(claims),
            "supportedClaims": len([n for n in claims if n["id"] in supported or n["evidenceCues"]]),
            "hedgedLines": len([n for n in nodes if n["kind"] == "hedged"]),
            "evidenceNodes": len([n for n in nodes if n["kind"] == "evidence"]),
            "counters": len([e for e in edges if e["relation"] == "countered_by"]),
        },
        uncoveredClaims=uncovered,
        directorHints=[
            f"{len(uncovered)} claim(s) ride with no evidence: give them proof b-roll, a source lower-third, or soften them",
            "a countered claim is the tension beat - visual contradiction, hard cut on 'but'",
            "hedged lines are weak beats: either commit (rewrite) or make the hedge the point (comedy beat)",
        ],
        source="python (cue grammars over claims/evidence/hedges/counters)",
    )


# ---------------------------------------------------------------------------
# 7. score_script_quality
# ---------------------------------------------------------------------------

_HOOK_CUES = re.compile(
    r"(\?|you |your |nobody|secret|most people|stop |never |why |how |what happens|"
    r"the truth|mistake|wrong |worst|best|biggest|\d+|million|billion|\$)", re.IGNORECASE)
_CTA_CUES = re.compile(
    r"\b(follow|subscribe|comment|share|save this|link in|try |sign up|download|join)\b",
    re.IGNORECASE)
_CURIO_CUES = re.compile(
    r"\b(but|until|turns out|the catch|here's the thing|little did|except|"
    r"what they found|then everything)\b", re.IGNORECASE)


def _score_script_quality(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    texts = [str(line.get("text") or "").strip() for line in lines]
    texts = [t for t in texts if t]
    if not texts:
        return {"ok": False, "error": "no non-empty lines"}
    words = [len(_tokens(t)) for t in texts]
    total = sum(words) or 1

    # 1. Hook (0-25): what the first two lines do.
    hook_lines = " ".join(texts[:2])
    hook_checks = {
        "question": bool(re.search(r"\?", hook_lines)),
        "numberOrMoney": bool(re.search(r"\d|\$", hook_lines)),
        "directAddress": bool(re.search(r"\byou(r)?\b", hook_lines, re.IGNORECASE)),
        "curiosityGap": bool(_CURIO_CUES.search(hook_lines)),
        "boldClaim": bool(re.search(r"\b(never|nobody|most people|stop|worst|best|truth)\b",
                                    hook_lines, re.IGNORECASE)),
        "shortEnough": len(_tokens(hook_lines)) <= 28,
    }
    hook_score = round(25 * sum(hook_checks.values()) / len(hook_checks), 1)

    # 2. Structure (0-25): the seven acts, by cue and by position.
    tail = " ".join(texts[-max(1, len(texts) // 5):])
    structure = {
        "hookPresent": hook_score >= 12,
        "ctaPresent": bool(_CTA_CUES.search(tail)),
        "payoffCue": bool(_CURIO_CUES.search(" ".join(texts[len(texts) // 2:]))),
        "twistCue": bool(re.search(r"\b(but|however|until|then|suddenly)\b",
                                   " ".join(texts[len(texts) // 3:2 * len(texts) // 3]), re.IGNORECASE)),
    }
    structure_score = round(25 * sum(structure.values()) / len(structure), 1)

    # 3. Clarity (0-15): from the readability read.
    mean_wps = total / len(texts)
    latin = _script_kind(" ".join(texts)) in ("latin",) or True
    spw = sum(_vs._syllables(t) for t in _tokens(" ".join(texts))) / total
    flesch = 206.835 - 1.015 * mean_wps - 84.6 * spw
    clarity = {
        "flesch": round(flesch, 1),
        "gradeFit": 50 <= flesch <= 75,
        "sentenceLength": round(mean_wps, 1),
    }
    clarity_score = round(15 * (1.0 if clarity["gradeFit"] else 0.5), 1)

    # 4. Rhythm (0-15): sentence-length variety.
    mean_len = total / len(texts)
    if len(words) > 1:
        var = sum((w - mean_len) ** 2 for w in words) / len(words)
        stdev = math.sqrt(var)
        variety = min(1.0, stdev / max(1.0, mean_len) / 0.7)
    else:
        variety = 0.0
    has_punch = any(w <= 5 for w in words)
    rhythm_score = round(15 * (0.7 * variety + 0.3 * (1.0 if has_punch else 0.0)), 1)

    # 5. Payoff/CTA (0-10).
    cta_lines = [i for i, t in enumerate(texts) if _CTA_CUES.search(t)]
    payoff_score = 10.0 if cta_lines else 0.0
    if cta_lines and cta_lines[-1] < len(texts) - 1:
        payoff_score = 7.0  # CTA too early: it must be the last thing heard

    # 6. Concision (0-10).
    filler_hits = sum(len(_vs._FILLER_RE.findall(" " + t.lower() + " "))
                      for t in texts) if hasattr(_vs, "_FILLER_RE") else 0
    dupes = {}
    for t in texts:
        sig = " ".join(_content_tokens(_tokens(t))[:6])
        if len(sig) > 8:
            dupes[sig] = dupes.get(sig, 0) + 1
    redundant = sum(v - 1 for v in dupes.values() if v > 1)
    concision_score = round(max(0.0, 10 - 2 * filler_hits - 2 * redundant), 1)

    total_score = round(hook_score + structure_score + clarity_score +
                        rhythm_score + payoff_score + concision_score, 1)
    fixes = []
    if not hook_checks["question"] and not hook_checks["curiosityGap"]:
        fixes.append("the opening asks no question and opens no gap: rewrite line 1 as a question or a 'most people get X wrong'")
    if not structure["ctaPresent"]:
        fixes.append("no call to action in the closing lines: name one (follow / try / link)")
    if not structure["twistCue"]:
        fixes.append("the middle has no turn ('but / until / then'): the middle third is flat")
    if not clarity["gradeFit"]:
        fixes.append(f"flesch {clarity['flesch']} sits outside the 50-75 sweet spot: shorten sentences or simplify words")
    if not has_punch:
        fixes.append("no line under 6 words: a short line is the beat that lands the punch")
    if redundant:
        fixes.append(f"{redundant} line(s) repeat an earlier idea: cut or make the callback explicit")
    if filler_hits:
        fixes.append(f"{filler_hits} filler(s): safe removals for a punchy cut")
    duration = round(total / 2.5, 1)  # 150 wpm
    target = spec.get("targetSeconds")
    if target:
        if duration > float(target) * 1.15:
            fixes.append(f"at 150 wpm this reads {duration}s against a {target}s target: cut {int(total - float(target) * 2.5)} words")
        elif duration < float(target) * 0.7:
            fixes.append(f"at 150 wpm this reads {duration}s against a {target}s target: room to breathe")

    return _ok(
        "score_script_quality",
        score=total_score,
        verdict=("strong" if total_score >= 75 else "workable" if total_score >= 55 else "needs a rewrite"),
        subScores={
            "hook": {"score": hook_score, "max": 25, "checks": hook_checks},
            "structure": {"score": structure_score, "max": 25, "checks": structure},
            "clarity": {"score": clarity_score, "max": 15, "measures": clarity},
            "rhythm": {"score": rhythm_score, "max": 15,
                       "lengthVariety": round(variety, 2), "hasPunchline": has_punch},
            "payoffCta": {"score": payoff_score, "max": 10, "ctaLines": cta_lines},
            "concision": {"score": concision_score, "max": 10,
                          "fillers": filler_hits, "redundantLines": redundant},
        },
        estimatedSecondsAt150wpm=duration,
        fixes=fixes or ["no structural fixes needed: direct the edit, not the script"],
        lines=len(texts),
        words=total,
        source="python (hook/structure/clarity/rhythm/cta/concision checks)",
    )


# ---------------------------------------------------------------------------
# 8. detect_sarcasm
# ---------------------------------------------------------------------------

_SARCASM_MARKERS = re.compile(
    r'([""„]|sure|obviously|yeah right|totally|exactly what i|just what i|'
    r'how convenient|thanks a lot|big surprise|shocking|love that for you|'
    r'greatest idea|brilliant|genius|perfect|fantastic)', re.IGNORECASE)
_POSITIVE = set("good great amazing awesome love best fantastic wonderful "
                "perfect brilliant genius beautiful incredible".split())
_NEGATIVE = set("bad worst hate terrible awful boring broken fail failed "
                "wrong stupid mess disaster painful ugly".split())


def _detect_sarcasm(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    audio = spec.get("audio") or spec.get("path")
    if audio and not Path(str(audio)).is_file():
        return {"ok": False, "error": f"no such audio file: {audio}"}

    # The audio cross-check: a positive word delivered flat or dropping is
    # the classic irony cue. Only when word timings + audio both exist.
    series = []
    ff = None
    if audio:
        ff, _ = _vs._ffmpeg(spec)
        try:
            series = _vs._momentary_series(ff, audio)
        except Exception:
            series = []

    per_line = []
    prev_sentiment = 0
    for idx, line in enumerate(lines):
        text = str(line.get("text") or "").strip()
        toks = _tokens(text)
        cues = []
        weight = 0.0
        m = _SARCASM_MARKERS.search(" " + text.lower() + " ")
        if m:
            cues.append(f"marker: {m.group(1).strip()}")
            weight += 0.35
        pos = sum(1 for t in toks if t in _POSITIVE)
        neg = sum(1 for t in toks if t in _NEGATIVE)
        if pos and prev_sentiment < 0:
            cues.append("positive words after a negative run")
            weight += 0.3
        elif neg and prev_sentiment > 0:
            cues.append("negative words after a positive run")
            weight += 0.2
        if pos and neg:
            cues.append(f"mixed sentiment in one line ({pos}+/{neg}-)")
            weight += 0.25
        # Audio incongruity: flat energy on the line's emphasized positive word.
        if series:
            start, end = line.get("start"), line.get("end")
            if start is not None and end is not None:
                vals = [mm for t, mm, _ in series if mm is not None and start - 0.2 <= t <= end + 0.2]
                if len(vals) >= 3 and pos:
                    spread = max(vals) - min(vals)
                    if spread < 3.0:  # under 3 LU of movement: flat delivery
                        cues.append(f"flat delivery on positive words ({spread:.1f} LU range)")
                        weight += 0.3
        sentiment = (pos - neg)
        prev_sentiment = sentiment if sentiment else prev_sentiment
        band = "high" if weight >= 0.55 else "medium" if weight >= 0.3 else "low"
        per_line.append({
            "line": idx,
            "text": text[:160],
            "start": line.get("start"),
            "end": line.get("end"),
            "sarcasm": band,
            "weight": round(min(1.0, weight), 2),
            "cues": cues,
        })
    flagged = [p for p in per_line if p["sarcasm"] != "low"]
    return _ok(
        "detect_sarcasm",
        lines=len(lines),
        perLine=per_line,
        flagged=flagged,
        note="a heuristic, never a verdict: sarcasm is context-dependent and this says which cues fired",
        directorHints=[
            "a sarcastic line earns irony cuts: cheerful stock over bitter words",
            "keep the deadpan pause after a sarcastic beat - the laugh lives there",
        ] if flagged else ["no sarcasm cues fired: treat the read as sincere"],
        source="python (marker lexicon + sentiment contrast" +
               (" + energy flatness)" if series else ")"),
    )


# ---------------------------------------------------------------------------
# 9. detect_code_switching
# ---------------------------------------------------------------------------

_SCRIPT_FONTS = {
    "bengali": "Noto Sans Bengali", "devanagari": "Noto Sans Devanagari",
    "arabic": "Noto Sans Arabic", "cyrillic": "Noto Sans",
    "cjk": "Noto Sans SC", "hangul": "Noto Sans KR", "thai": "Noto Sans Thai",
    "tamil": "Noto Sans Tamil", "latin": "Noto Sans",
}
_ROMANIZED_BENGALI = set("""
ami tumi apni bhai bon ma baba khub bhalo bhalobashi valobashi dukkho kanna
kotha kotha bola hocche ache nai ache-na ashbe jabe korbe pare dynamo
mone hoy kemon acho achis bhalo acho onek ektu ekhon ekhane khane
""".split())


def _char_script(ch):
    try:
        name = unicodedata.name(ch)
    except ValueError:
        return None
    for script in ("BENGALI", "DEVANAGARI", "ARABIC", "CYRILLIC", "CJK",
                   "HANGUL", "HIRAGANA", "KATAKANA", "THAI", "TAMIL", "LATIN"):
        if script in name:
            return script.lower()
    return None


def _detect_code_switching(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    per_line = []
    switches = []
    prev_lang = None
    lang_counts = {}
    for idx, line in enumerate(lines):
        text = str(line.get("text") or "")
        counts = {}
        for ch in text:
            sc = _char_script(ch)
            if sc:
                counts[sc] = counts.get(sc, 0) + 1
        total_alpha = sum(counts.values()) or 1
        scripts = sorted(counts.items(), key=lambda kv: -kv[1])
        dominant = scripts[0][0] if scripts else None
        minor = [s for s, n in scripts[1:] if n >= max(2, 0.2 * total_alpha)]
        # romanized borrowing: latin tokens that are romanized Bengali words
        toks = _tokens(text)
        borrowing = [t for t in toks if t in _ROMANIZED_BENGALI] if dominant != "latin" or True else []
        lang = dominant or "unknown"
        lang_counts[lang] = lang_counts.get(lang, 0) + 1
        entry = {
            "line": idx,
            "text": text[:120],
            "start": line.get("start"),
            "end": line.get("end"),
            "dominantScript": lang,
            "scripts": {s: n for s, n in scripts if n},
            "mixedWith": minor,
            "romanizedBorrowings": sorted(set(borrowing))[:6],
        }
        per_line.append(entry)
        if prev_lang and lang != prev_lang:
            switches.append({"line": idx, "from": prev_lang, "to": lang})
        prev_lang = lang

    scripts_needed = sorted({p["dominantScript"] for p in per_line if p["dominantScript"]})
    fonts = [_SCRIPT_FONTS.get(s, "Noto Sans") for s in scripts_needed]
    mixed = len(switches) > 0 or any(p["mixedWith"] for p in per_line)
    return _ok(
        "detect_code_switching",
        lines=len(lines),
        languageCounts=lang_counts,
        perLine=per_line,
        switches=switches,
        mixedTake=mixed,
        typography={"scripts": scripts_needed, "fonts": fonts,
                    "note": "a mixed-script caption needs per-script font fallback or one pan-script font"},
        tts={"note": "a mixed take needs a multilingual voice or per-language segments; "
                     "single-voice TTS will mangle the switches"} if mixed else {"note": "single language: any voice works"},
        directorHints=[
            "each switch point is a natural scene or caption-style change - the language turn IS the beat",
            "give romanized borrowings the same visual weight as their native-script siblings",
        ] if mixed else ["single-script take; no switching handling needed"],
        source="python (Unicode block scan per line and per token)",
    )


# ---------------------------------------------------------------------------
# 10. detect_cross_lingual_emotion
# ---------------------------------------------------------------------------

# Native-script and romanized lexicons for the languages the English
# lexicon cannot hear. Each maps onto FUNCUT's seven emotions. These are
# high-precision cores, not exhaustive dictionaries: a line with no hit
# reads neutral with an honest low confidence, never a guess.
_XLING_LEXICON = {
    "joy": {
        "bengali": "আনন্দ খুশি সুখ হাসি হাসিখুশি ভালো দারুণ মজা প্রিয় আনন্দিত চমৎকার উৎসাহ",
        "romanized_bengali": "khushi anondo sukho hashi darun moja priyo bhalo-lagche",
        "hindi": "खुशी हँसी प्यार उम्मीद अच्छा बहुत बढ़िया मज़ा",
        "spanish": "feliz alegre alegría amor encanta genial encanta risa",
        "french": "heureux joie amour génial formidable rire merveilleux",
    },
    "sadness": {
        "bengali": "দুঃখ কষ্ট কান্না শোক বেদনা হতাশ কান্নাকাটি দুর্বল",
        "romanized_bengali": "dukkho koshto kanna shok bedona hodash",
        "hindi": "दुख दर्द गम आँसू उदास तोड़",
        "spanish": "triste tristeza llorar lágrimas dolor pena sufrir",
        "french": "triste tristesse pleurer larmes douleur chagrin peine",
    },
    "fear": {
        "bengali": "ভয় আতঙ্ক চিন্তা শংকা ভীত ডর ভয়ংকর",
        "romanized_bengali": "bhoy atonko chinta shonka bhito bhoionkor",
        "hindi": "डर आतंक चिंता डरावना भय",
        "spanish": "miedo temer pánico terror susto angustia preocupación",
        "french": "peur panique terreur angoisse effrayant crainte",
    },
    "anger": {
        "bengali": "রাগ ক্রোধ ঘৃণা অসন্তুষ্ট ক্ষুব্ধ জ্বালা",
        "romanized_bengali": "rag krodh ghrina osontushto khubdho",
        "hindi": "गुस्सा क्रोध नफ़रत जलाल गुस्सै",
        "spanish": "enfado rabia furia odio enojado coraje indignación",
        "french": "colère rage fureur haine furieux énervé",
    },
    "disgust": {
        "bengali": "ঘৃণা বিতৃষ্ণা জঘন্য নোংরা বিশ্রী",
        "romanized_bengali": "ghrina bityristha jognyo nongra bishri",
        "hindi": "घृणा जघन्य गंदा बीभत्स",
        "spanish": "asco repugnancia horrible asqueroso nauseabundo",
        "french": "dégoût répugnant horrible écœurant nausée",
    },
    "surprise": {
        "bengali": "অবাক বিস্ময় চমক হঠাৎ আশ্চর্য",
        "romanized_bengali": "obak bismoy chomok hothat ashchorjo",
        "hindi": "हैरान चौंक आश्चर्य अचानक वाह",
        "spanish": "sorpresa asombro increible inesperado vaya",
        "french": "surprise étonnement incroyable soudain wow",
    },
}
_XLING_BY_LANG = {}
for _emotion, _langs in _XLING_LEXICON.items():
    for _lang, _blob in _langs.items():
        for _word in _blob.split():
            _XLING_BY_LANG.setdefault(_lang, {}).setdefault(_word, _emotion)


def _detect_cross_lingual_emotion(spec):
    lines = _lines(spec)
    if not lines:
        return {"ok": False, "error": "no lines: pass transcript, lines or script"}
    hint = str(spec.get("language") or "").lower()

    per_line = []
    for idx, line in enumerate(lines):
        text = str(line.get("text") or "")
        kind = _script_kind(text)
        # pick the lexicon by script, romanized for latin+Bengali context
        lexicon = None
        lang_used = None
        if "bengali" in kind or hint in ("bn", "ben", "bengali"):
            lexicon, lang_used = _XLING_BY_LANG["bengali"], "bengali"
        elif "devanagari" in kind or hint in ("hi", "hin", "hindi"):
            lexicon, lang_used = _XLING_BY_LANG["hindi"], "hindi"
        elif kind.startswith("mixed:latin") and hint in ("bn", "ben", "bengali"):
            lexicon, lang_used = _XLING_BY_LANG["romanized_bengali"], "romanized_bengali"
        elif "cyrillic" in kind or hint in ("ru", "rus", "russian"):
            lexicon, lang_used = None, "cyrillic"
        else:
            # romanized words ride in latin script: scan them too
            lexicon, lang_used = _XLING_BY_LANG["romanized_bengali"], "romanized_bengali"
        scores = {"anger": 0, "disgust": 0, "fear": 0, "joy": 0,
                  "sadness": 0, "surprise": 0}
        hits = 0
        tokens = set(_tokens(text))
        native_text = text
        if lexicon:
            for word, emotion in lexicon.items():
                if word in native_text or word in tokens:
                    scores[emotion] += 1
                    hits += 1
        total_tokens = len(tokens) or 1
        coverage = round(hits / max(1, min(6, total_tokens)), 3) if hits else 0.0
        if hits == 0:
            emotion, intensity, confidence = "neutral", 0.0, 0.3
        else:
            best = max(scores, key=lambda k: scores[k])
            emotion = best
            intensity = round(min(1.0, 0.45 + 0.2 * scores[best]), 2)
            confidence = round(min(0.8, 0.4 + 0.35 * coverage), 2)
        per_line.append({
            "line": idx,
            "text": text[:120],
            "start": line.get("start"),
            "end": line.get("end"),
            "script": kind,
            "emotion": emotion,
            "intensity": intensity,
            "confidence": confidence,
            "lexiconHits": hits,
            "language": lang_used,
        })

    dist = {}
    for p in per_line:
        dist[p["emotion"]] = dist.get(p["emotion"], 0) + 1
    low = [p for p in per_line if p["lexiconHits"] == 0]
    return _ok(
        "detect_cross_lingual_emotion",
        lines=len(lines),
        perLine=per_line,
        distribution=dist,
        coverage={
            "linesWithHits": len(per_line) - len(low),
            "linesWithoutHits": len(low),
            "note": "the offline lexicons are precision cores; no-hit lines read neutral "
                    "at low confidence - for depth use the hosted multilingual model "
                    "(FUNCUT_EMOTION_API_KEY) or WhisperX + translation",
        },
        directorHints=[
            "per-line emotion feeds pacing: joy lines cut faster, fear/sadness lines hold",
            "low-coverage lines should default to the brief's tone, not to 'neutral' visuals",
        ],
        source="python (Bengali / romanized-Bengali / Hindi / Spanish / French emotion cores)",
    )


# ---------------------------------------------------------------------------
# The dispatch table, selftest and entry points
# ---------------------------------------------------------------------------

TOOLS = {
    "analyze_discourse": _analyze_discourse,
    "segment_topics": _segment_topics,
    "detect_speech_acts": _detect_speech_acts,
    "classify_speech_acts": _classify_speech_acts,
    "score_readability": _score_readability,
    "extract_claim_graph": _extract_claim_graph,
    "score_script_quality": _score_script_quality,
    "detect_sarcasm": _detect_sarcasm,
    "detect_code_switching": _detect_code_switching,
    "detect_cross_lingual_emotion": _detect_cross_lingual_emotion,
}

_SELFTEST_SCRIPT = """Most people think saving money is about willpower. But the data says otherwise: it is about systems.
First, automate the transfer on payday. Studies show people who automate save twice as much.
Second, cut the three silent leaks. Because nobody tracks subscriptions, the average house loses $219 a month.
Now imagine what that money becomes in ten years. $38,000. No extra work, no extra income.
But here is the catch nobody mentions. Inflation eats lazy cash. So the last step is the important one.
Let me show you the exact order I use. Follow for part two, and try the free tracker in the link."""

_SELFTEST_BENGALI = """আজ আমরা একটা দারুণ গল্প শুনব। এই শহরে সবাই ভয় পায় সেই পুরনো বাড়িটা দেখলে।
কিন্তু রাহিম চাচা হাসিখুশি ছিলেন সবসময়। তার কান্না ছিল না, ছিল শুধু আনন্দ।"""


def selftest(only=None):
    """Prove every tool this module serves on a script that has all the
    cues: markers, topics, acts, claims, sarcasm, switches, and emotion."""
    checks = []
    spec = {"script": _SELFTEST_SCRIPT}
    for name, handler in TOOLS.items():
        if only and name != only:
            continue
        try:
            out = handler(spec)
            checks.append({
                "tool": name,
                "ok": bool(out.get("ok")),
                "lines": out.get("lines"),
                "error": out.get("error"),
            })
        except Exception as exc:  # noqa: BLE001
            checks.append({"tool": name, "ok": False, "error": f"{type(exc).__name__}: {exc}"})
    # the Bengali path: code switching + cross-lingual emotion
    try:
        mixed = handler_spec = {"script": _SELFTEST_SCRIPT + "\n" + _SELFTEST_BENGALI}
        out = _detect_code_switching(mixed)
        checks.append({"tool": "detect_code_switching (bengali mix)",
                       "ok": bool(out.get("ok")) and out.get("mixedTake") is True,
                       "error": out.get("error")})
        out = _detect_cross_lingual_emotion({"script": _SELFTEST_BENGALI, "language": "bn"})
        hits = sum(1 for p in out.get("perLine", []) if p["lexiconHits"])
        checks.append({"tool": "detect_cross_lingual_emotion (bengali)",
                       "ok": bool(out.get("ok")) and hits > 0,
                       "hits": hits, "error": out.get("error")})
    except Exception as exc:  # noqa: BLE001
        checks.append({"tool": "bengali path", "ok": False,
                       "error": f"{type(exc).__name__}: {exc}"})
    failed = [c for c in checks if not c.get("ok")]
    return {
        "ok": not failed,
        "schema": SCHEMA,
        "action": "selftest",
        "checks": checks,
        "failed": failed,
        "error": f"{len(failed)} tool(s) failed: " + "; ".join(
            f"{c['tool']}: {c.get('error')}" for c in failed) if failed else None,
    }


def command(spec):
    """The sense.py proxy entry: spec names the tool; action extract or
    selftest. Never raises - the contract is one JSON object out."""
    tool = str(spec.get("tool") or "")
    action = str(spec.get("action", "extract"))
    try:
        if action == "selftest":
            return selftest(tool if tool in TOOLS else None)
        handler = TOOLS.get(tool)
        if handler is None:
            return {"ok": False,
                    "error": f"unknown tool {tool!r}: one of {sorted(TOOLS)}"}
        return handler(spec)
    except Exception as exc:  # noqa: BLE001  the contract: never crash
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def main():
    parser = argparse.ArgumentParser(description="The script read as language.")
    parser.add_argument("cmd", choices=["extract", "selftest"])
    parser.add_argument("--spec", default="{}")
    args = parser.parse_args()
    raw = sys.stdin.read() if args.spec == "-" else args.spec
    try:
        spec = json.loads(raw)
        if not isinstance(spec, dict):
            raise ValueError("spec must be a JSON object")
    except ValueError as exc:
        print(json.dumps({"ok": False, "error": f"bad spec: {exc}"}))
        sys.exit(2)
    spec.setdefault("action", args.cmd)
    out = command(spec)
    print(json.dumps(out, indent=1))
    sys.exit(0 if out.get("ok") else 4)


if __name__ == "__main__":
    main()
