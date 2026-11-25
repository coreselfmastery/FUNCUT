// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The models that are downloaded rather than compiled in: what they are
//! called, where they come from, and where they live once fetched.
//!
//! Each is one ONNX file in `<app data>/cutout-models/`, named as the
//! table says; a file that is there and whole is an installed model, and
//! a fetch lands in a `.part` beside it and renames at the end, so a
//! killed download leaves nothing that could be mistaken for one. The
//! fetching itself is the host's, since it is the host that has a network
//! and a job slot; this crate only knows the table.
//!
//! A model is fetched from Concat's own mirror, and `upstream` is where
//! that mirror was filled from and the second thing tried. The table is
//! the engine's half of `models/manifest.toml`, which is what fills the
//! mirror; `scripts/models.py --check` keeps the two saying the same
//! thing.

use std::path::{Path, PathBuf};

/// The downloadable models.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModelId {
    /// Robust Video Matting on MobileNetV3: a person, with a true alpha
    /// edge, carrying state from one frame to the next.
    Person,
    /// IS-Net from the DIS paper: the one thing a picture is of, whatever
    /// it is.
    Object,
    /// SlimSAM's image encoder: a picture into the embeddings the brush
    /// decoder reads.
    BrushEncoder,
    /// SlimSAM's prompt encoder and mask decoder: points on the picture
    /// into the mask of what is under them.
    BrushDecoder,
    /// Real-ESRGAN's compact general model: a picture four times its size
    /// with its noise and compression gone, what Enhance is made of.
    Enhance,
    /// CLAP's audio tower: a sound into the 512-d space it shares with
    /// text, so "a deep cinematic braam" can be compared with the file.
    ClapAudio,
    /// CLAP's text tower: the other half of that space.
    ClapText,
    /// CLAP's tokenizer: text into the ids the text tower reads.
    ClapTokenizer,
    /// YAMNet: 521 AudioSet classes, what a sound file contains.
    Yamnet,
    /// YAMNet's class names, in the order the model scores them.
    YamnetNames,
    /// TinyAudio-MF's text encoder: a prompt into the 77-token hidden
    /// states the flow step is conditioned on.
    SfxSmallText,
    /// TinyAudio-MF's mean-flow step: one denoising pass, guidance
    /// applied inside the graph, 431 x 64 latents per 10 s.
    SfxSmallFlow,
    /// TinyAudio-MF's decoder: latents into 44.1 kHz audio.
    SfxSmallVae,
    /// TinyAudio-MF's tokenizer: words into the ids the text encoder reads.
    SfxSmallTokenizer,
    /// TinyAudio-MF's pipeline description: rates, shapes and step count.
    SfxSmallPipeline,
    /// Stable Audio Open Small's T5 encoder: a prompt into 64 x 768.
    SfxLargeT5,
    /// The T5 encoder's weights, stored beside its graph.
    SfxLargeT5Data,
    /// Stable Audio Open Small's distilled rectified-flow DiT step.
    SfxLargeDit,
    /// The DiT step's weights, stored beside its graph.
    SfxLargeDitData,
    /// Stable Audio Open Small's Oobleck decoder: 64 x 256 latents into
    /// 11.9 s of 44.1 kHz stereo.
    SfxLargeVae,
    /// The Oobleck decoder's weights, stored beside its graph.
    SfxLargeVaeData,
    /// Stable Audio Open Small's `seconds_total` conditioner: the learned
    /// Fourier embedding of the requested length.
    SfxLargeSeconds,
    /// Stable Audio Open Small's T5 tokenizer.
    SfxLargeTokenizer,
    /// Stable Audio Open Small's own description of its bundle: sample
    /// rate, latent shape, segment length and step count.
    SfxLargeMeta,
    /// TinyMusician's text encoder: a prompt into MusicGen's 768-d states.
    BgmText,
    /// TinyMusician's MusicGen decoder: four codebooks, one token a step.
    BgmDecoder,
    /// TinyMusician's EnCodec decoder: codebook tokens into 32 kHz audio.
    BgmCodec,
    /// TinyMusician's T5 tokenizer.
    BgmTokenizer,
    /// TinyMusician's MusicGen configuration: rate, codebooks, layer count.
    BgmConfig,
}

/// One model as the table describes it.
#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    /// Which model.
    pub id: ModelId,
    /// The file name on disk, what a mask store records, and what the
    /// file is called on the mirror.
    pub file: &'static str,
    /// Where the mirror was filled from, and the second place a download
    /// tries; the host's `models` module is what puts the two in order.
    pub upstream: &'static str,
    /// Its size, for a progress bar before the server says.
    pub bytes: u64,
    /// What a finished download must hash to; a download with nothing to
    /// check against is refused.
    pub sha256: &'static str,
    /// The licence it comes under, for the notices.
    pub licence: &'static str,
}

/// Every downloadable model.
pub const MODELS: [ModelSpec; 29] = [
    ModelSpec {
        id: ModelId::Person,
        file: "rvm-mobilenetv3.onnx",
        upstream: "https://github.com/PeterL1n/RobustVideoMatting/releases/download/v1.0.0/rvm_mobilenetv3_fp32.onnx",
        bytes: 14_975_696,
        sha256: "88d4531297118f595bf2fd60f6f566aec2e559393802d1f436c380f0cbbd2828",
        licence: "GPL-3.0",
    },
    ModelSpec {
        id: ModelId::Object,
        file: "isnet-general-use.onnx",
        upstream: "https://github.com/danielgatis/rembg/releases/download/v0.0.0/isnet-general-use.onnx",
        bytes: 178_648_008,
        sha256: "60920e99c45464f2ba57bee2ad08c919a52bbf852739e96947fbb4358c0d964a",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::BrushEncoder,
        file: "slimsam-77-encoder.onnx",
        upstream: "https://huggingface.co/Xenova/slimsam-77-uniform/resolve/5850ab45f587c112167512ffef949107115e26a0/onnx/vision_encoder.onnx",
        bytes: 23_276_014,
        sha256: "9f8433273a6750b587779baa0cf5508111001bf7e7acfcf585d370139fd366d0",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::BrushDecoder,
        file: "slimsam-77-decoder.onnx",
        upstream: "https://huggingface.co/Xenova/slimsam-77-uniform/resolve/5850ab45f587c112167512ffef949107115e26a0/onnx/prompt_encoder_mask_decoder.onnx",
        bytes: 16_557_892,
        sha256: "f4514391764fbd56e08e119060d874ecd7d52994bfb1968af159e12d4943b5bb",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::Enhance,
        file: "realesr-general-x4v3.onnx",
        upstream: "https://huggingface.co/CoderViking/realesr-general-x4v3-onnx/resolve/c6a971706797c7502945a2b4c4274fce4900d4ab/realesr-general-x4v3.onnx",
        bytes: 4_866_417,
        sha256: "1940a93ee08283a0a7286183186357b1688fe9fa8ede74604b424586aaddf112",
        licence: "BSD-3-Clause",
    },
    // The sound models. Apache-2.0 throughout, which is why they were
    // chosen over the stronger music models that carry a non-commercial
    // licence (docs/SOUND-MODEL.md section 2).
    ModelSpec {
        id: ModelId::ClapAudio,
        file: "clap_audio.onnx",
        upstream: "https://huggingface.co/Xenova/clap-htsat-unfused/resolve/c28f2883575e590e04d3146ff0713c2448d691ba/onnx/audio_model_quantized.onnx",
        bytes: 34_301_667,
        sha256: "3fcff2c8824e7bcb83a983f2a49edab3b60cbcf4872ac70efee517355173bd1f",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::ClapText,
        file: "clap_text.onnx",
        upstream: "https://huggingface.co/Xenova/clap-htsat-unfused/resolve/c28f2883575e590e04d3146ff0713c2448d691ba/onnx/text_model_quantized.onnx",
        bytes: 126_603_263,
        sha256: "1a3df8b197e249816e08415fd040434c44762b2eea7eb7bf8a48a0f0bf3c14e5",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::ClapTokenizer,
        file: "clap_tokenizer.json",
        upstream: "https://huggingface.co/Xenova/clap-htsat-unfused/resolve/c28f2883575e590e04d3146ff0713c2448d691ba/tokenizer.json",
        bytes: 2_108_774,
        sha256: "dc239041d98de27ffc3975473a1a23e3db4c937b23c138c38bbc66588bd247e5",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::Yamnet,
        file: "yamnet.onnx",
        // A community ONNX conversion of Google's Apache-2.0 weights:
        // record both, and check the conversion against the original
        // before trusting a label it produces.
        upstream: "https://huggingface.co/zeropointnine/yamnet-onnx/resolve/ac2ca3bd45d12ec1f19f1144205ea529b4e9dedf/yamnet.onnx",
        bytes: 16_093_603,
        sha256: "1510041dce24a2e9e84ec546807ac408ae496da6d1ed41bc3ccba649623f8e19",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::YamnetNames,
        file: "yamnet_class_map.csv",
        upstream: "https://huggingface.co/zeropointnine/yamnet-onnx/resolve/ac2ca3bd45d12ec1f19f1144205ea529b4e9dedf/yamnet_class_map.csv",
        bytes: 14_096,
        sha256: "cdf24d193e196d9e95912a2667051ae203e92a2ba09449218ccb40ef787c6df2",
        licence: "Apache-2.0",
    },
    // ── Sound generation ─────────────────────────────────────────────
    // Three models, each a set of files: a text encoder, a generator and
    // an audio decoder. They are listed here beside the readers because
    // this is the one table the settings sheet and the sidecar both read,
    // and because `scripts/models.py` checks a mirror row against the
    // file names in this table. Their `kind` is "generate", so the sound
    // reader's set is still exactly the five files above.
    //
    // TinyAudio-MF (MIT): 87 M parameters, one mean-flow step, 10 s at
    // 44.1 kHz. The lightest of the three by a wide margin, and the one
    // the small SFX row downloads.
    ModelSpec {
        id: ModelId::SfxSmallTokenizer,
        file: "sfx-small-tokenizer.json",
        upstream: "https://huggingface.co/junxiliu/TinyAudio/resolve/28e4c990d18044131e77e8528d5d79c3e7714891/cpu/standard/tokenizer/tokenizer.json",
        bytes: 2_132_967,
        sha256: "9fd55248d51d33976b324fc11592e28071da7d41e0e9401dfb7082e30574b7b1",
        licence: "MIT",
    },
    ModelSpec {
        id: ModelId::SfxSmallPipeline,
        file: "sfx-small-pipeline.json",
        upstream: "https://huggingface.co/junxiliu/TinyAudio/resolve/28e4c990d18044131e77e8528d5d79c3e7714891/cpu/standard/pipeline.json",
        bytes: 670,
        sha256: "c01a8b8fbff62edcac4a3c2be02affa425a85444a4f1ebff6321cd3675cf4a39",
        licence: "MIT",
    },
    ModelSpec {
        id: ModelId::SfxSmallText,
        file: "sfx-small-text.onnx",
        upstream: "https://huggingface.co/junxiliu/TinyAudio/resolve/28e4c990d18044131e77e8528d5d79c3e7714891/cpu/standard/text_encoder.onnx",
        bytes: 127_777_800,
        sha256: "a02961c9e08e7a1091786be95ead5be443ca11dc2fccca376ee40d11c75d2323",
        licence: "MIT",
    },
    ModelSpec {
        id: ModelId::SfxSmallVae,
        file: "sfx-small-vae.onnx",
        upstream: "https://huggingface.co/junxiliu/TinyAudio/resolve/28e4c990d18044131e77e8528d5d79c3e7714891/cpu/standard/vae_decode.onnx",
        bytes: 83_011_126,
        sha256: "1aff9ec512d2aca3e1b3dc0632e419497a7f9b62fab68b407e87268418229b92",
        licence: "MIT",
    },
    ModelSpec {
        id: ModelId::SfxSmallFlow,
        file: "sfx-small-flow.onnx",
        upstream: "https://huggingface.co/junxiliu/TinyAudio/resolve/28e4c990d18044131e77e8528d5d79c3e7714891/cpu/standard/meanflow_step.onnx",
        bytes: 142_610_964,
        sha256: "2fff20b9ac9055da2734d2d9cb4ad1ab6b2775d6001cdc03e729c7bfacd1a847",
        licence: "MIT",
    },
    // Stable Audio Open Small (Stability AI Community License): 341 M
    // parameters, eight distilled rectified-flow steps, 11.9 s of stereo.
    // The heavier SFX row, and the one whose licence asks a reader to
    // check the revenue threshold before monetizing.
    ModelSpec {
        id: ModelId::SfxLargeTokenizer,
        file: "sfx-large-tokenizer.json",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/t5_tokenizer_json.json",
        bytes: 2_424_332,
        sha256: "30ecc5c7c3f2a5e24f1837df50ad6288e12aef9c78a12eede8f67489faec1c45",
        licence: "Stability AI Community License",
    },
    ModelSpec {
        id: ModelId::SfxLargeSeconds,
        file: "sfx-large-seconds.json",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/seconds_embedder.json",
        bytes: 4_405_016,
        sha256: "0e08dd6200826d602e55ed20655f9fc30cdc46ef831d2dbde9575ce60495e250",
        licence: "Stability AI Community License",
    },
    ModelSpec {
        id: ModelId::SfxLargeMeta,
        file: "sfx-large-meta.json",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/model_weights_meta.json",
        bytes: 835,
        sha256: "22206201697ebdb7bc6496c489e0185c17251b3d24c1fe40902c9c58d8e1d8e3",
        licence: "Stability AI Community License",
    },
    ModelSpec {
        id: ModelId::SfxLargeT5,
        file: "sfx-large-t5.onnx",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/t5_text_encoder.onnx",
        bytes: 330_376,
        sha256: "95d8b885c5d7a855366f5061abc6aafe08a378ab22ecb86c3bad57e9ee673784",
        licence: "Stability AI Community License",
    },
    // The encoder's weights keep the name its graph was exported with:
    // ONNX stores that name as the external-data location, so renaming
    // the file here would leave a graph that cannot find its own bytes.
    ModelSpec {
        id: ModelId::SfxLargeT5Data,
        file: "t5_text_encoder.onnx.data",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/t5_text_encoder.onnx.data",
        bytes: 110_216_576,
        sha256: "d48426eaafeacd0e20ebd48b67179a7278ee2df6e6ad3801ee4c2bcdc371941e",
        licence: "Stability AI Community License",
    },
    ModelSpec {
        id: ModelId::SfxLargeDit,
        file: "sfx-large-dit.onnx",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/dit_step.onnx",
        bytes: 930_150,
        sha256: "1b52abe1ac0f0b5c8208bed89db538ca860e030ebc09e8a1bb338ea11044531e",
        licence: "Stability AI Community License",
    },
    ModelSpec {
        id: ModelId::SfxLargeDitData,
        file: "dit_step.onnx.data",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/dit_step.onnx.data",
        bytes: 341_714_944,
        sha256: "9ccaac2be9c02fe6b320a6d4af2a90509b2b76ed61e70c6519ef298d75fe1bb2",
        licence: "Stability AI Community License",
    },
    ModelSpec {
        id: ModelId::SfxLargeVae,
        file: "sfx-large-vae.onnx",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/vae_decoder.onnx",
        bytes: 250_288,
        sha256: "896a3aa833b84a0a314d11b5b2630c73a1fbe86495e498c375d163d038acafb5",
        licence: "Stability AI Community License",
    },
    ModelSpec {
        id: ModelId::SfxLargeVaeData,
        file: "vae_decoder.onnx.data",
        upstream: "https://huggingface.co/fernhoof/stable-audio-open-small/resolve/3278b34d2e551b21bc36787f3331106220c08559/vae_decoder.onnx.data",
        bytes: 78_289_664,
        sha256: "a30eded351e419f97a2808d3e7bbe0ab683ba03f864b4dc717abc1798e33761a",
        licence: "Stability AI Community License",
    },
    // TinyMusician (no licence file in the upstream repository): MusicGen
    // at 15 layers, four codebooks, 32 kHz mono. The BGM row, and the one
    // an unattended run never spends on a monetized cut.
    ModelSpec {
        id: ModelId::BgmTokenizer,
        file: "bgm-tokenizer.json",
        upstream: "https://huggingface.co/itsmax/TinyMusician/resolve/792d2e061a079802d192b9ab82933ce5406103fd/onnx/tokenizer.json",
        bytes: 2_422_169,
        sha256: "71c33c3bf57bf2e19b4f8ac686ad8d182a79f7cd1a4921889437c453420223f1",
        licence: "CC-BY-NC-4.0 (MusicGen-small weights)",
    },
    ModelSpec {
        id: ModelId::BgmConfig,
        file: "bgm-config.json",
        upstream: "https://huggingface.co/itsmax/TinyMusician/resolve/792d2e061a079802d192b9ab82933ce5406103fd/onnx/config.json",
        bytes: 8_264,
        sha256: "b2568a15382bf35ae5af135917f28ac5204d715da6b53e60b19a83c80a52d2ef",
        licence: "CC-BY-NC-4.0 (MusicGen-small weights)",
    },
    ModelSpec {
        id: ModelId::BgmText,
        file: "bgm-text.onnx",
        upstream: "https://huggingface.co/itsmax/TinyMusician/resolve/792d2e061a079802d192b9ab82933ce5406103fd/onnx/text_encoder.onnx",
        bytes: 110_076_078,
        sha256: "5a7e4354596a15c47ed1669926f1ff8398b195e0a914f4e6a92b97618e7162d5",
        licence: "CC-BY-NC-4.0 (MusicGen-small weights)",
    },
    ModelSpec {
        id: ModelId::BgmCodec,
        file: "bgm-codec.onnx",
        upstream: "https://huggingface.co/itsmax/TinyMusician/resolve/792d2e061a079802d192b9ab82933ce5406103fd/onnx/encodec_decode.onnx",
        bytes: 59_796_655,
        sha256: "4d87a6f86dd84fc967c9b45206acc96796aa93eb14cf7a9346fd06e04a3ad31e",
        licence: "CC-BY-NC-4.0 (MusicGen-small weights)",
    },
    ModelSpec {
        id: ModelId::BgmDecoder,
        file: "bgm-decoder.onnx",
        upstream: "https://huggingface.co/itsmax/TinyMusician/resolve/792d2e061a079802d192b9ab82933ce5406103fd/onnx/decoder_model_merged.onnx",
        bytes: 274_356_314,
        sha256: "e1a4481a24ec947c4c7cc6a7d742c0c03a9ccdf2a18c6589d4d2d157c8e60d70",
        licence: "CC-BY-NC-4.0 (MusicGen-small weights)",
    },
];

impl ModelId {
    /// The table's row for this model.
    pub fn spec(self) -> &'static ModelSpec {
        MODELS
            .iter()
            .find(|spec| spec.id == self)
            .expect("every model is in the table")
    }

    /// Which layer of the app a model serves. The settings sheet groups
    /// its rows by this, and the sidecars ask for their own kind, so a
    /// download lands where the reader that needs it already looks.
    pub fn kind(self) -> &'static str {
        match self {
            ModelId::Person
            | ModelId::Object
            | ModelId::BrushEncoder
            | ModelId::BrushDecoder
            | ModelId::Enhance => "vision",
            ModelId::ClapAudio
            | ModelId::ClapText
            | ModelId::ClapTokenizer
            | ModelId::Yamnet
            | ModelId::YamnetNames => "sound",
            // The generators are their own kind: they are not part of the
            // reader's set, and the settings sheet gives each of the three
            // its own row and its own download.
            ModelId::SfxSmallText
            | ModelId::SfxSmallFlow
            | ModelId::SfxSmallVae
            | ModelId::SfxSmallTokenizer
            | ModelId::SfxSmallPipeline
            | ModelId::SfxLargeT5
            | ModelId::SfxLargeT5Data
            | ModelId::SfxLargeDit
            | ModelId::SfxLargeDitData
            | ModelId::SfxLargeVae
            | ModelId::SfxLargeVaeData
            | ModelId::SfxLargeSeconds
            | ModelId::SfxLargeTokenizer
            | ModelId::SfxLargeMeta
            | ModelId::BgmText
            | ModelId::BgmDecoder
            | ModelId::BgmCodec
            | ModelId::BgmTokenizer
            | ModelId::BgmConfig => "generate",
        }
    }

    /// The name a mask store records for masks this model made.
    pub fn name(self) -> &'static str {
        match self {
            ModelId::Person => "rvm-mobilenetv3",
            ModelId::Object => "isnet-1024",
            ModelId::BrushEncoder => "slimsam-77-encoder",
            ModelId::BrushDecoder => "slimsam-77-decoder",
            ModelId::Enhance => "realesr-general-x4v3",
            ModelId::ClapAudio => "clap-htsat-audio",
            ModelId::ClapText => "clap-htsat-text",
            ModelId::ClapTokenizer => "clap-htsat-tokenizer",
            ModelId::Yamnet => "yamnet",
            ModelId::YamnetNames => "yamnet-classes",
            ModelId::SfxSmallText => "sfx-small-text",
            ModelId::SfxSmallFlow => "sfx-small-flow",
            ModelId::SfxSmallVae => "sfx-small-vae",
            ModelId::SfxSmallTokenizer => "sfx-small-tokenizer",
            ModelId::SfxSmallPipeline => "sfx-small-pipeline",
            ModelId::SfxLargeT5 => "sfx-large-t5",
            ModelId::SfxLargeT5Data => "sfx-large-t5-data",
            ModelId::SfxLargeDit => "sfx-large-dit",
            ModelId::SfxLargeDitData => "sfx-large-dit-data",
            ModelId::SfxLargeVae => "sfx-large-vae",
            ModelId::SfxLargeVaeData => "sfx-large-vae-data",
            ModelId::SfxLargeSeconds => "sfx-large-seconds",
            ModelId::SfxLargeTokenizer => "sfx-large-tokenizer",
            ModelId::SfxLargeMeta => "sfx-large-meta",
            ModelId::BgmText => "bgm-text",
            ModelId::BgmDecoder => "bgm-decoder",
            ModelId::BgmCodec => "bgm-codec",
            ModelId::BgmTokenizer => "bgm-tokenizer",
            ModelId::BgmConfig => "bgm-config",
        }
    }
}

/// Where the models live: `<app data>/cutout-models/`.
pub fn models_dir(data: &Path) -> PathBuf {
    data.join("cutout-models")
}

/// The file a model is, once fetched.
pub fn model_file(data: &Path, id: ModelId) -> PathBuf {
    models_dir(data).join(id.spec().file)
}

/// Whether a model is on disk and whole.
pub fn installed(data: &Path, id: ModelId) -> bool {
    std::fs::metadata(model_file(data, id)).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_has_its_own_file_and_name() {
        let files: std::collections::HashSet<_> = MODELS.iter().map(|m| m.file).collect();
        assert_eq!(files.len(), MODELS.len());
        let names: std::collections::HashSet<_> = MODELS.iter().map(|m| m.id.name()).collect();
        assert_eq!(names.len(), MODELS.len());
        assert!(!installed(Path::new("/nowhere"), ModelId::Person));
    }
}
