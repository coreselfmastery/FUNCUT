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
    /// TransNetV2: a shot boundary, per frame, from the motion arc rather
    /// than from one frame's histogram. What tells a cut where a shot
    /// actually ends.
    TransNetV2,
    /// Depth Anything V2 Small: a relative depth per pixel. The parallax
    /// layer, the depth-aware caption band and the real seam a cut wants.
    DepthAnything,
    /// DINOv2 Small: a 384-d visual embedding per frame - "looks like this
    /// shot", which CLIP's text space is weaker at.
    Dinov2,
    /// CLIP ViT-B/32's vision tower: a frame into the space CLIP's text
    /// side already ranks stock in.
    ClipVision,
    /// MoveNet singlepose: the body in the frame, as a box and a head.
    MovenetPose,
    /// SegFormer-B0 on Cityscapes: every pixel's class. The only reading
    /// that can say what fills a frame - a subject, a sky, a road - which
    /// is what a caption band and an ambience call stand on.
    Segformer,
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
pub const MODELS: [ModelSpec; 16] = [
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
    // The picture reader's four. `sense_visual` reads them from this same
    // folder, so a set fetched here is the set it measures with.
    ModelSpec {
        id: ModelId::TransNetV2,
        file: "transnetv2.onnx",
        upstream: "https://huggingface.co/elya5/transnetv2/resolve/main/transnetv2.onnx",
        bytes: 31_250_929,
        sha256: "c4d54a682bace32f25136ef83ca2c9d403e8f8193775efeb995172a0d95a8e0c",
        licence: "MIT",
    },
    ModelSpec {
        id: ModelId::DepthAnything,
        file: "depth-anything-v2-small.onnx",
        upstream: "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model_quantized.onnx",
        bytes: 27_258_801,
        sha256: "fcf51f1b230362b28690bb9d1809bf0431f29cad20534e3f589bd7285547f20d",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::Dinov2,
        file: "dinov2-small.onnx",
        upstream: "https://huggingface.co/onnx-community/dinov2-small/resolve/main/onnx/model_quantized.onnx",
        bytes: 24_446_700,
        sha256: "c179f8f7f592449c4c1bca4cd124a7538021428c5ffb89afde9503935b197efb",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::ClipVision,
        file: "clip-vit-b32-visual.onnx",
        upstream: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main/onnx/vision_model_quantized.onnx",
        bytes: 89_117_001,
        sha256: "583fd1110a514667812fee7d684952aaf82a99b959760c8d7dca7e0ab9839299",
        licence: "MIT",
    },
    ModelSpec {
        id: ModelId::MovenetPose,
        file: "movenet-pose.onnx",
        upstream: "https://huggingface.co/Xenova/movenet-singlepose-lightning/resolve/main/onnx/model_int8.onnx",
        bytes: 2_788_976,
        sha256: "8bef13b3da241fe1302d247fd9363bbbf09ea4f02344372a09f73921b2f9ec01",
        licence: "Apache-2.0",
    },
    ModelSpec {
        id: ModelId::Segformer,
        file: "segformer-b0.onnx",
        upstream: "https://huggingface.co/Xenova/segformer-b0-finetuned-cityscapes-768-768/resolve/main/onnx/model_quantized.onnx",
        bytes: 4_384_797,
        sha256: "e90dc4112c066066110c5aac955990427095a27a88c7dbfd4fcc7d1bbf147147",
        licence: "Apache-2.0",
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
            ModelId::TransNetV2
            | ModelId::DepthAnything
            | ModelId::Dinov2
            | ModelId::ClipVision
            | ModelId::MovenetPose
            | ModelId::Segformer => "picture",
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
            ModelId::TransNetV2 => "transnetv2",
            ModelId::DepthAnything => "depth-anything-v2-small",
            ModelId::Dinov2 => "dinov2-small",
            ModelId::ClipVision => "clip-vit-b32",
            ModelId::MovenetPose => "movenet-pose",
            ModelId::Segformer => "segformer-b0",
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
