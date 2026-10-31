<p align="center">
  <img src="https://cdn.jsdelivr.net/gh/jub0t/Concat@main/assets/concat-mark-dark.png" alt="Concat" width="100" />
</p>

<h1 align="center">Concat</h1>
<h3 align="center">البديل المجاني ومفتوح المصدر لـ CapCut على لينكس.</h3>

<div align="center">

<img src="https://cdn.jsdelivr.net/gh/jub0t/Concat@main/assets/editor.png" alt="Concat editor" width="100%" />

<p align="center">
  <a href="https://github.com/coreselfmastery/FUNCUT/releases"><img src="https://img.shields.io/github/downloads/coreselfmastery/FUNCUT/total?style=flat&logo=github&logoColor=F8F8F8&label=Downloads&labelColor=212123&color=0568FD" alt="Total Downloads" /></a>
  <a href="https://github.com/coreselfmastery/FUNCUT/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/coreselfmastery/FUNCUT/ci.yml?style=flat&logo=githubactions&logoColor=F8F8F8&label=Build&labelColor=000000" alt="Build Status" /></a>
  <a href="https://github.com/coreselfmastery/FUNCUT/releases"><img src="https://img.shields.io/badge/Version-0.2.8-0568FD?style=flat&logo=semver&logoColor=F8F8F8&labelColor=212123" alt="Concat Version 0.2.8" /></a>
  <a href="https://discord.gg/DVuPfpXfqP"><img src="https://img.shields.io/badge/Discord-Join%20the%20server-5865F2?style=flat&logo=discord&logoColor=F8F8F8&labelColor=000000" alt="Join Concat Discord" /></a>
  <a href="../LICENSE"><img src="https://img.shields.io/badge/License-AGPL%20v3-0568FD?style=flat&logo=gnu&logoColor=F8F8F8&labelColor=212123" alt="License: AGPL-3.0-or-later" /></a>
</p>

<p align="center">
  <a href="https://www.star-history.com/jub0t/concat"><img src="https://api.star-history.com/badge?repo=jub0t/concat&type=rank" alt="Star History Rank" height="62" /></a>
  <a href="https://trendshift.io/repositories/191379?utm_source=trendshift-badge&amp;utm_medium=badge&amp;utm_campaign=badge-trendshift-191379" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/trendshift/repositories/191379/daily?language=Rust" alt="jub0t%2FConcat | Trendshift" width="250" height="55"/></a>
  <a href="https://concatenate.pages.dev/#download"><img src="../assets/download_button.svg" alt="Download Concat" width="251" /></a>
</p>

[🇬🇧 English](../README.md) · [🇩🇪 Deutsch](README.de.md) · [🇪🇸 Español](README.es.md) · [🇫🇷 Français](README.fr.md) · [🇮🇹 Italiano](README.it.md) · [🇳🇱 Nederlands](README.nl.md) · [🇵🇱 Polski](README.pl.md) · [🇧🇷 Português (Brasil)](README.pt-BR.md) · [🇷🇺 Русский](README.ru.md) · [🇺🇦 Українська](README.uk.md) · [🇹🇷 Türkçe](README.tr.md) · [🇦🇿 Azərbaycanca](README.az.md) · [🇭🇷 Hrvatski](README.hr.md) · [🇮🇩 Bahasa Indonesia](README.id.md) · [🇻🇳 Tiếng Việt](README.vi.md) · [🇹🇭 ไทย](README.th.md) · [🇯🇵 日本語](README.ja.md) · [🇰🇷 한국어](README.ko.md) · [🇨🇳 简体中文](README.zh-Hans.md) · [🇹🇼 繁體中文](README.zh-TW.md) · **🇸🇦 العربية** · [🇮🇱 עברית](README.he.md) · [🇮🇷 فارسی](README.fa.md) · [🇮🇳 हिन्दी](README.hi.md) · [🇵🇰 اردو](README.ur.md) · [🇧🇩 বাংলা](README.bn.md)

> مترجم من ملف README الإنجليزي بتاريخ 11 أكتوبر 2026. النسخة الإنجليزية هي التي تُحدَّث باستمرار.

</div>


## نبذة

Concat محرر فيديو مجاني ومفتوح المصدر وبديل لـ CapCut على Linux. يغطي ما يفتح الناس CapCut من أجله فعلًا: الترجمة التلقائية، تحويل النص إلى كلام، إزالة الخلفية، التحريك بالإطارات المفتاحية، المؤثرات والعناوين، القص متعدد المسارات، التصدير بدقة 4K. وبلا أي فخاخ: لا علامة مائية، لا حساب، لا اشتراك، لا رفع للملفات.

كل شيء يعمل محليًا على محرك Rust أصلي مع مركّب رسوميات على بطاقة الرسوميات. ثبّته، أسقط لقطاتك، وقصّ. نماذج الذكاء الاصطناعي للترجمة والأصوات والقص تُنزَّل مرة واحدة من الإعدادات ثم تعمل دون اتصال. لقطاتك لا تغادر قرصك أبدًا.

**مناسب لـ:** TikTok وReels وShorts، فيديوهات YouTube، الشروحات وتسجيلات الشاشة، مقاطع البودكاست، الميمز.

**وللآلات أيضًا:** واجهة برمجية JSON-RPC، لتتمكن السكربتات ووكلاء الذكاء الاصطناعي من قص الفيديو به كذلك.

## أبرز الميزات

- 🚫 **لا علامات مائية. لا حساب. لا جدار دفع.** أبدًا.
- 🔒 **محلي 100 %.** لا شيء يُرفع. يعمل دون اتصال.
- 💬 **ترجمة تلقائية.** Whisper محلي. اختر حجم النموذج واحصل على ترجمات منسّقة على الخط الزمني.
- 🗣️ **تحويل النص إلى كلام + استنساخ الصوت.** أصوات محلية مجانية، أو أي صوت من بضع ثوانٍ من التسجيل.
- 🧍 **إزالة الخلفية.** الأشخاص، الأشياء، أو ارسم القناع بنفسك.
- 🎞️ **إطارات مفتاحية.** الموضع، الحجم، الدوران، الشفافية، مستوى الصوت، معاملات المؤثرات. محرر منحنيات مدمج.
- ✨ **100 مؤثر ومرشح وانتقال.** تُرسم على بطاقة الرسوميات، مباشرة في المعاينة.
- ✂️ **قصّ سريع.** تقسيم، تشذيب، إزاحة، دمج، تجميد الإطار، السرعة. خط زمني مغناطيسي إن أردت.
- 🎚️ **مسارات متعددة، خطوط زمنية متعددة.** عدة مونتاجات في مشروع واحد. أنماط المزج، الاقتصاص، القلب.
- 📝 **عناوين.** خطوط، حدود، ظل، لوحة خلفية. إعدادات مسبقة للبدء منها.
- 🎙️ **تنقية الصوت بمفتاح واحد.** إزالة الضوضاء، تحسين الصوت، موازنة الارتفاع. إضافة إلى السنجاب والروبوت والهاتف وغيرها.
- 📤 **تصدير.** H.264 وHEVC وAV1. حتى 4K بمعدل 60، ألوان 10 بت.
- 🦀 **محرك Rust أصلي.** مركّب على بطاقة الرسوميات، ملفات وسيطة، فك ترميز عتادي. تمرير 4K سلس.
- 🤖 **قابل للبرمجة.** واجهة JSON-RPC يمكن للسكربتات ووكلاء الذكاء الاصطناعي قص الفيديو بها. والبناء من الشيفرة المصدرية يضيف gRPC وأداة سطر أوامر.
- 🖥️ **Linux.** 26 لغة. التطبيق نفسه، ملفات المشروع نفسها.

## التنزيل

مدخلان:

1. **[الموقع](https://concatenate.pages.dev/#download)** يعطيك الإصدار المناسب لجهازك. ابدأ من هنا.
2. **[GitHub Releases](https://github.com/coreselfmastery/FUNCUT/releases)** يضم كل إصدار لكل منصة، مع المثبّتات والحزم ومجاميع التحقق. حين تريد الاختيار بنفسك.

Concat في مرحلة **بيتا**: يعمل، وما زالت فيه حواف. [أخبرنا](https://github.com/jub0t/Concat/issues) حين تجد واحدة.

**المنصات**

- ✅ **Linux** · x86_64 وARM. ‏`.deb` و`.rpm` و`.AppImage` وحزمة Arch. وأيضًا Flatpak على **[Flatpark](https://flatpark.org/apps/app.concat.editor/)**، وهو مستودع Flatpak مجتمعي يغلّف ملف `.deb` لمعمارية x86_64 من كل إصدار ويتحدّث معه

✅ مدعوم · 🚧 قيد العمل · 🧪 بانتظار الاختبار

**متطلبات النظام** وأحجام النماذج الاختيارية على [الموقع](https://concatenate.pages.dev/guides/system-requirements).

**[Code signing policy](../CODE_SIGNING.md)**

## كيف تساهم

> [!IMPORTANT]
> أفضل طريقة للمساهمة أن تأخذ إصدارًا من صفحة [Releases](https://github.com/coreselfmastery/FUNCUT/releases) وتستخدمه: اعثر على ما يتعطّل، وقل أين يمكن أن يكون أفضل.
>
> مستعد لكتابة الشيفرة؟ يغطي [CONTRIBUTING.md](../CONTRIBUTING.md) الإعداد وبنية الشجرة والفحوصات التي تُشغَّل وكيفية ترخيص المساهمات. تقود Concat من سكربت أو خدمة أو وكيل؟ تغطي [وثائق المطوّرين](https://concatenate.pages.dev/docs) واجهة Concat ووسائل نقلها: JSON-RPC وgRPC. [هذه المناقشة](https://github.com/jub0t/Concat/discussions/3) هي حيث أُعلن عن المشروع.
> 
> يمكن للمساهمين الحصول على دور `@Contributor` في خادم Discord: فقط اطلبه.

## تاريخ النجوم

<a href="https://www.star-history.com/?repos=jub0t%2Fconcat&type=date&releases=&legend=bottom-right">
 <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=jub0t/concat&type=date&legend=bottom-right" />
</a>

## المساهمون

<a href="https://github.com/jub0t/Concat/graphs/contributors">
  <img alt="Contributors" src="https://contrib.rocks/image?repo=jub0t/concat">
</a>

## DMCA

If you believe something in this repository, its releases, or a package or model it distributes infringes your copyright, send a notice through [GitHub's DMCA takedown process](https://docs.github.com/en/site-policy/content-removal-policies/dmca-takedown-policy), which covers this repository's content and releases; a valid notice is acted on through that process. Material this repository inherits from upstream Concat can also be reported to upstream at jub0trd@gmail.com, the address upstream's README publishes for the purpose.
