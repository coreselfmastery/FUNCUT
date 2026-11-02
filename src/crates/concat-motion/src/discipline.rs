// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The motion principles, as a lint instead of a mood - moved into
//! [`crate::lint`], where the four other families live beside them under
//! stable codes.
//!
//! This module stays as the name the plan's warnings were written against:
//! [`audit`] here is [`crate::lint::sentences`], the same `Vec<String>` of
//! motion findings it always was, so a caller - and every reader of a
//! warning - sees the same sentences, byte for byte.
//!
//! 1. **Entrances arrive; they do not creep.** A value rising out of
//!    nothing should spend its energy early and settle - `ease-out`,
//!    `emphasis`, a spring. An entrance on `ease-in` or `linear` is the
//!    single most robotic read a comp can make, and the one this lint
//!    exists to catch. `steps()` is exempt: a mechanical reveal is a
//!    *choice*, and the typewriter is its poster child.
//! 2. **Exits leave; they do not settle.** A value falling to nothing
//!    should accelerate away (`ease-in`) or step out - an exit that
//!    eases *out* hangs in the doorway, and an exit that travels
//!    linearly reads as a switch turning off.
//! 3. **Overshoot is punctuation, not prose.** A spring that bounces or a
//!    `linear()` that overshoots is a playful moment's curve; on a calm
//!    scene it is a comp that cannot hold still. The lint holds this one
//!    against the scene's personality: a comp wearing the `punchy` style
//!    is *allowed* its bounce, which is what the style is for - the
//!    reserve is against wearing bounce everywhere, not against wearing it
//!    on purpose.
//! 4. **A drift is not an entrance.** A plate panning slowly across a
//!    scene on `linear` is honest motion - the whole point of linear is
//!    steady travel. The lint polices curves only in the windows that are
//!    entrances and exits (a value rising out of, or falling to, nothing),
//!    never the steady stretches between.
//!
//! The audit is **advice, not refusal**: its findings ride the plan's
//! warnings, where the critic can read them and an agent can fix them,
//! because a comp that breaks a principle on purpose is a comp that
//! should render - and be told.

pub use crate::lint::sentences as audit;
