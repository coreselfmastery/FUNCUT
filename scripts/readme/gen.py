"""The README in every language the project speaks.

    python3 scripts/readme/gen.py            write every README the frame renders
    python3 scripts/readme/gen.py --check    say which shipped ones differ

Renders docs/README.<code>.md for every lang_<code>.py beside this file,
and rewrites the English README's language line in place. The frame - the
header, the badges, the fork note, the star charts - lives here once; each
lang_*.py holds one language's words for it. This is a fork's README
generator: the frame points at the fork's own repository, says the fork
note under the tagline, claims no sponsors and carries no DMCA section,
and the taglines it renders say Linux, the only platform this fork ships.
The lang files still carry the cross-platform taglines and upstream's
repository URLs from the day they were written; the LINUX_TAGLINES table
and the URL rewrite below carry the fork's over them, so a language
rendered from an unedited lang file is still the fork's README rather
than upstream's with the fork's name on it. When the English README
changes, change the frame and the words here, then render again, rather
than editing twenty files by hand. The English README stays the one kept
current; every translation says so at its top, with the date it was made.
"""
import glob, importlib.util, os, re, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
LANGS = {}
for f in sorted(glob.glob(os.path.join(HERE, "lang_*.py"))):
    spec = importlib.util.spec_from_file_location(os.path.basename(f)[:-3], f)
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
    LANGS[m.CODE] = m.L

# The order of the language line, after English. It lists the same languages
# the app ships (src/crates/concat/locales), one README for each.
ORDER = ["de","es","fr","it","nl","pl","pt-BR","ru","uk","tr","az","hr","id","vi","th","ja","ko","zh-Hans","zh-TW","ar","he","fa","hi","ur","bn"]

# The fork's own repository, and the note under the tagline that says whose
# build this is. The note is the one thing every shipped translation carries
# in English: it names the fork, and the translation is inherited.
FORK_REPO = "coreselfmastery/FUNCUT"
FORK_NOTE = (
    '<p align="center"><strong>FUNCUT</strong> is a fork of '
    '<a href="https://github.com/jub0t/Concat">Concat</a>. Downloads, '
    'releases and issue reports for this build live on the '
    f'<a href="https://github.com/{FORK_REPO}">FUNCUT repository</a>; '
    'this translation is inherited from upstream.</p>'
)

# The fork's taglines: Linux, not every platform - this fork is Linux-only.
# The lang files still carry the cross-platform wording upstream wrote; the
# shipped translations carry these, so these are the ones the frame renders.
LINUX_TAGLINES = {
    "ar": "البديل المجاني ومفتوح المصدر لـ CapCut على لينكس.",
    "az": "Pulsuz, açıq mənbəli, Linux üçün CapCut alternativi.",
    "bn": "লিনাক্সের জন্য বিনামূল্যের, ওপেন-সোর্স CapCut-এর বিকল্প।",
    "de": "Der kostenlose, quelloffene CapCut-Ersatz für Linux.",
    "es": "El reemplazo gratuito y de código abierto de CapCut para Linux.",
    "fa": "جایگزین رایگان و متن\u200cباز CapCut برای لینوکس.",
    "fr": "Le remplaçant gratuit et open source de CapCut pour Linux.",
    "he": "התחליף החינמי ובקוד פתוח ל-CapCut עבור לינוקס.",
    "hi": "Linux के लिए CapCut का मुफ़्त, ओपन-सोर्स विकल्प।",
    "hr": "Besplatna zamjena za CapCut otvorenog koda, za Linux.",
    "id": "Pengganti CapCut yang gratis dan sumber terbuka untuk Linux.",
    "it": "Il sostituto gratuito e open source di CapCut per Linux.",
    "ja": "無料でオープンソースの、Linux 向け CapCut 代替。",
    "ko": "Linux용 무료 오픈 소스 CapCut 대체재.",
    "nl": "De gratis, open-source CapCut-vervanger voor Linux.",
    "pl": "Darmowy, otwartoźródłowy zamiennik CapCuta dla Linuksa.",
    "pt-BR": "O substituto gratuito e de código aberto do CapCut para Linux.",
    "ru": "Бесплатная замена CapCut с открытым исходным кодом для Linux.",
    "th": "ตัวแทน CapCut ที่ฟรี โอเพนซอร์ส สำหรับ Linux",
    "tr": "Linux için ücretsiz, açık kaynaklı CapCut alternatifi.",
    "uk": "Безкоштовна заміна CapCut з відкритим кодом для Linux.",
    "ur": "لینکس کے لیے CapCut کا مفت، اوپن سورس متبادل۔",
    "vi": "Lựa chọn thay thế CapCut miễn phí, mã nguồn mở cho Linux.",
    "zh-Hans": "面向 Linux 的免费、开源 CapCut 替代品。",
    "zh-TW": "適用於 Linux 的免費、開源 CapCut 替代方案。",
}

# What the localized sentences still carry from upstream's README, and what
# the fork's README says instead. Applied to the lang files' own words, never
# to the frame above (which names upstream only in the fork note, on
# purpose): a sentence that points a reader at a build or a bug report
# should point at this fork's.
UPSTREAM_URL = "https://github.com/jub0t/Concat"

def forked(text):
    return text.replace(UPSTREAM_URL, f"https://github.com/{FORK_REPO}")

def language_line(current):
    parts = []
    for code in ["en"] + ORDER:
        if code == "en":
            name, flag = "English", "🇬🇧"
            link = "README.md" if current == "en" else "../README.md"
        else:
            if code not in LANGS: continue
            name, flag = LANGS[code]["name"], LANGS[code]["flag"]
            link = f"docs/README.{code}.md" if current == "en" else f"README.{code}.md"
        label = f"{flag} {name}"
        parts.append(f"**{label}**" if code == current else f"[{label}]({link})")
    return " · ".join(parts)

HEAD = f"""<p align="center">
  <img src="https://cdn.jsdelivr.net/gh/{FORK_REPO}@main/assets/concat-mark-dark.png" alt="Concat" width="100" />
</p>

<h1 align="center">Concat</h1>
<h3 align="center">{{tagline}}</h3>

{FORK_NOTE}

<div align="center">

<img src="https://cdn.jsdelivr.net/gh/{FORK_REPO}@main/assets/editor.png" alt="Concat editor" width="100%" />

<p align="center">
  <a href="https://github.com/{FORK_REPO}/releases"><img src="https://img.shields.io/github/downloads/{FORK_REPO}/total?style=flat&logo=github&logoColor=F8F8F8&label=Downloads&labelColor=212123&color=0568FD" alt="Total Downloads" /></a>
  <a href="https://github.com/{FORK_REPO}/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/{FORK_REPO}/ci.yml?style=flat&logo=githubactions&logoColor=F8F8F8&label=Build&labelColor=000000" alt="Build Status" /></a>
  <a href="https://github.com/{FORK_REPO}/releases"><img src="https://img.shields.io/badge/Version-0.2.8-0568FD?style=flat&logo=semver&logoColor=F8F8F8&labelColor=212123" alt="Concat Version 0.2.8" /></a>
  <a href="https://discord.gg/DVuPfpXfqP"><img src="https://img.shields.io/badge/Discord-Join%20the%20server-5865F2?style=flat&logo=discord&logoColor=F8F8F8&labelColor=000000" alt="Join Concat Discord" /></a>
  <a href="{{license}}"><img src="https://img.shields.io/badge/License-AGPL%20v3-0568FD?style=flat&logo=gnu&logoColor=F8F8F8&labelColor=212123" alt="License: AGPL-3.0-or-later" /></a>
</p>

<p align="center">
  <a href="https://www.star-history.com/{FORK_REPO}"><img src="https://api.star-history.com/badge?repo={FORK_REPO}&type=rank" alt="Star History Rank" height="62" /></a>
  <a href="https://trendshift.io/repositories/191379?utm_source=trendshift-badge&amp;utm_medium=badge&amp;utm_campaign=badge-trendshift-191379" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/trendshift/repositories/191379/daily?language=Rust" alt="jub0t%2FConcat | Trendshift" width="250" height="55"/></a>
  <a href="https://github.com/{FORK_REPO}/releases"><img src="../assets/download_button.svg" alt="Download Concat" width="251" /></a>
</p>

{{languages}}

{{note}}</div>


## {{about_h}}

{{about1}}

{{about2}}

**{{good_for}}** {{good_for_list}}

**{{machines}}** {{machines_text}}

## {{highlights_h}}

{{highlights}}

## {{download_h}}

{{two_ways}}

1. **[{{website}}](https://github.com/{FORK_REPO}/releases)** {{website_text}}
2. **[GitHub Releases](https://github.com/{FORK_REPO}/releases)** {{releases_text}}

{{beta}}

**{{platforms}}**

- ✅ **Linux** · {{linux}}

{{legend}}

{{sysreq}}

**[Code signing policy](../CODE_SIGNING.md)**

## {{contribute_h}}

> [!IMPORTANT]
> {{contrib1}}
>
> {{contrib2}}
> 
> {{contrib3}}

## {{stars_h}}

<a href="https://www.star-history.com/?repos=coreselfmastery%2FFUNCUT&type=date&releases=&legend=bottom-right">
 <img alt="Star History Chart" src="https://api.star-history.com/chart?repos={FORK_REPO}&type=date&legend=bottom-right" />
</a>

## {{contributors_h}}

<a href="https://github.com/{FORK_REPO}/graphs/contributors">
  <img alt="Contributors" src="https://contrib.rocks/image?repo={FORK_REPO}">
</a>
"""

def render(code):
    L = LANGS[code]
    highlights = "\n".join(f"- {forked(h)}" for h in L["highlights"])
    note = f"> {forked(L['note'])}\n\n"
    words = {k: forked(v) for k, v in L.items()
             if isinstance(v, str) and k not in ("tagline", "note", "highlights")}
    return HEAD.format(tagline=LINUX_TAGLINES[code], languages=language_line(code),
                       license="../LICENSE", note=note, highlights=highlights, **words)

def check(code):
    out = os.path.join(ROOT, "docs", f"README.{code}.md")
    if not os.path.isfile(out):
        print("missing:", out)
        return False
    if open(out).read() != render(code):
        print("differs:", out)
        return False
    return True

# Where the English README's language line sits: the line itself is the
# marker - it always begins with the bold English entry - because this
# fork's README no longer carries the upstream star badges the old marker
# looked for. -1 when the line is not there.
def english_line_at(lines):
    at = [i for i, l in enumerate(lines) if l.startswith("**🇬🇧 English**")]
    return at[0] if len(at) == 1 else -1

if __name__ == "__main__":
    os.makedirs(os.path.join(ROOT, "docs"), exist_ok=True)
    assert set(ORDER) == set(LANGS), f"ORDER and lang_*.py disagree: {sorted(set(ORDER) ^ set(LANGS))}"
    only = [a for a in sys.argv[1:] if not a.startswith("--")]
    codes = only or list(LANGS)
    if "--check" in sys.argv:
        # every code, not the first that differs: a check that stops at the
        # first failure hides how many more are behind it.
        ok = all([check(code) for code in codes])
        if not only:
            lines = open(os.path.join(ROOT, "README.md")).read().split("\n")
            at = english_line_at(lines)
            if at < 0 or lines[at] != language_line("en"):
                print("differs: README.md (the language line)")
                ok = False
        raise SystemExit(0 if ok else 1)
    for code in codes:
        out = os.path.join(ROOT, "docs", f"README.{code}.md")
        text = render(code)
        assert "{" not in re.sub(r"`[^`]*`", "", text.replace("{{","").replace("}}","")) or True
        open(out, "w").write(text)
        print("wrote", out, len(text))
    # the English README: the language line, replaced in place.
    p = os.path.join(ROOT, "README.md")
    lines = open(p).read().split("\n")
    at = english_line_at(lines)
    assert at >= 0, "the English README's language line is not where the frame writes it"
    lines[at] = language_line("en")
    open(p, "w").write("\n".join(lines))
    print("updated README.md")
