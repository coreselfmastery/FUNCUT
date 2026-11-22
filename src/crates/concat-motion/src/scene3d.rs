// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! A small CPU 3D scene: meshes, a camera, lights and a z-buffer.
//!
//! This is the "Three.js shape" a comp can carry without a browser, a GPU
//! or a JavaScript runtime: a scene graph of primitives (box, plane,
//! sphere, cylinder, torus), a perspective camera that reuses the comp's
//! own [`Camera3D`] flight path, ambient/directional/point lights, and a
//! per-pixel z-buffer. Everything is deterministic: the same scene, time
//! and frame size produce the same pixels.
//!
//! The renderer is deliberately a rasteriser, not a raytracer. It shades
//! each triangle per pixel (Phong normals), with a simple metal/rough
//! response, and writes premultiplied sRGB into a `tiny_skia::Pixmap` so
//! the native compositor can place it like any other layer. `samples: 2`
//! renders at twice the size and box-filters down, which is the cheap
//! anti-aliasing this CPU path can afford.

use std::sync::OnceLock;

use glam::{DMat3, DMat4, DQuat, DVec3, DVec4, EulerRot};
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;

use crate::camera::{AxisStop, Camera3D, CameraStop};
use crate::comp::Rgba;
use crate::curve::Curve;

/// A 3D scene a comp can carry.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Scene {
    /// The eye. A resting camera gets a sensible three-quarter view.
    #[serde(default)]
    pub camera: Camera3D,
    /// The lights. Empty means one ambient and one key light.
    #[serde(default)]
    pub lights: Vec<Light>,
    /// The meshes, drawn in order with a shared z-buffer.
    #[serde(default)]
    pub objects: Vec<Object>,
    /// The scene's own clear colour. `None` is transparent, so the comp's
    /// background and the layers under it show through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Rgba>,
    /// 1 for one sample a pixel, 2 for a 2x2 supersample. Clamped to 1..=2.
    #[serde(default = "one_u32")]
    pub samples: u32,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            camera: Camera3D::default(),
            lights: Vec::new(),
            objects: Vec::new(),
            background: None,
            samples: 1,
        }
    }
}

/// A light in the scene.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Light {
    /// A constant fill from every direction.
    Ambient {
        /// The light's colour.
        #[serde(default = "white")]
        color: Rgba,
        /// How strong it is; `0` is off.
        #[serde(default = "one")]
        intensity: f64,
    },
    /// A sun: parallel rays travelling along `direction`.
    Directional {
        /// The direction the light travels, in scene space.
        direction: [f64; 3],
        /// The light's colour.
        #[serde(default = "white")]
        color: Rgba,
        /// How strong it is.
        #[serde(default = "one")]
        intensity: f64,
    },
    /// A lamp at a point, falling off with distance.
    Point {
        /// Where the lamp stands.
        position: [f64; 3],
        /// The light's colour.
        #[serde(default = "white")]
        color: Rgba,
        /// How strong it is.
        #[serde(default = "one")]
        intensity: f64,
        /// The distance at which the falloff halves; a larger range reaches
        /// farther.
        #[serde(default = "default_range")]
        range: f64,
    },
}

/// One mesh in the scene.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Object {
    /// A stable id, for diagnostics.
    #[serde(default)]
    pub id: String,
    /// The shape.
    #[serde(default)]
    pub geometry: Geometry,
    /// Where its centre sits, in comp pixels.
    #[serde(default)]
    pub position: [f64; 3],
    /// Degrees about x, y and z, applied in that order.
    #[serde(default)]
    pub rotation: [f64; 3],
    /// Scale about its centre.
    #[serde(default = "one3")]
    pub scale: [f64; 3],
    /// Degrees a second added to `rotation`, so a scene can turn without a
    /// keyframe on every object.
    #[serde(default)]
    pub spin: [f64; 3],
    /// The base colour.
    #[serde(default = "white")]
    pub color: Rgba,
    /// 0 is a dielectric, 1 a metal.
    #[serde(default)]
    pub metalness: f64,
    /// 0 is a mirror, 1 is chalk.
    #[serde(default = "default_roughness")]
    pub roughness: f64,
    /// How much the object's own colour is added unlit, 0..=1.
    #[serde(default)]
    pub emissive: f64,
}

impl Default for Object {
    fn default() -> Self {
        Object {
            id: String::new(),
            geometry: Geometry::default(),
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
            spin: [0.0; 3],
            color: white(),
            metalness: 0.0,
            roughness: 0.5,
            emissive: 0.0,
        }
    }
}

/// A primitive mesh.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Geometry {
    /// A box, centred on the object's position.
    Box {
        /// Width, height and depth.
        size: [f64; 3],
    },
    /// A plane facing +z.
    Plane {
        /// Width.
        width: f64,
        /// Height.
        height: f64,
    },
    /// A UV sphere.
    Sphere {
        /// Radius.
        radius: f64,
        /// Longitude segments.
        #[serde(default = "default_segments")]
        segments: u32,
        /// Latitude rings.
        #[serde(default = "default_rings")]
        rings: u32,
    },
    /// A capped cylinder standing on the z axis.
    Cylinder {
        /// Radius.
        radius: f64,
        /// Height.
        height: f64,
        /// Radial segments.
        #[serde(default = "default_segments")]
        segments: u32,
    },
    /// A torus in the xy plane.
    Torus {
        /// Distance from the centre to the tube's centre.
        radius: f64,
        /// The tube's radius.
        tube: f64,
        /// Segments around the ring.
        #[serde(default = "default_segments")]
        segments: u32,
        /// Segments around the tube.
        #[serde(default = "default_sides")]
        sides: u32,
    },
}

impl Default for Geometry {
    fn default() -> Self {
        Geometry::Box {
            size: [100.0, 100.0, 100.0],
        }
    }
}

impl Geometry {
    /// The first thing wrong with the shape, if anything.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |value: f64, name: &str| -> Result<(), String> {
            if value.is_finite() && value > 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be a positive number"))
            }
        };
        match self {
            Geometry::Box { size } => {
                for (index, value) in size.iter().enumerate() {
                    positive(*value, ["width", "height", "depth"][index])?;
                }
            }
            Geometry::Plane { width, height } => {
                positive(*width, "width")?;
                positive(*height, "height")?;
            }
            Geometry::Sphere {
                radius,
                segments,
                rings,
            } => {
                positive(*radius, "radius")?;
                check_segments(*segments, 3, 128, "segments")?;
                check_segments(*rings, 2, 128, "rings")?;
            }
            Geometry::Cylinder {
                radius,
                height,
                segments,
            } => {
                positive(*radius, "radius")?;
                positive(*height, "height")?;
                check_segments(*segments, 3, 128, "segments")?;
            }
            Geometry::Torus {
                radius,
                tube,
                segments,
                sides,
            } => {
                positive(*radius, "radius")?;
                positive(*tube, "tube")?;
                check_segments(*segments, 3, 128, "segments")?;
                check_segments(*sides, 3, 64, "sides")?;
            }
        }
        Ok(())
    }
}

impl Scene {
    /// The first thing wrong with the scene, if anything.
    pub fn validate(&self) -> Result<(), String> {
        if self.objects.is_empty() {
            return Err("a 3D scene needs at least one object".into());
        }
        if self.samples == 0 || self.samples > 2 {
            return Err("samples must be 1 or 2".into());
        }
        for (index, stop) in self.camera.path.iter().enumerate() {
            if !stop.at.is_finite()
                || !stop.x.is_finite()
                || !stop.y.is_finite()
                || !stop.z.is_finite()
            {
                return Err(format!(
                    "camera stop {index}: at, x, y and z must be finite"
                ));
            }
        }
        for (name, axis) in [
            ("pitch", &self.camera.pitch),
            ("yaw", &self.camera.yaw),
            ("roll", &self.camera.roll),
            ("fov", &self.camera.fov),
        ] {
            for (index, stop) in axis.iter().enumerate() {
                if !stop.at.is_finite() || !stop.value.is_finite() {
                    return Err(format!(
                        "camera {name} stop {index}: at and value must be finite"
                    ));
                }
            }
        }
        for (index, object) in self.objects.iter().enumerate() {
            let name = if object.id.is_empty() {
                format!("object {index}")
            } else {
                format!("object {:?}", object.id)
            };
            if !object.position.iter().all(|value| value.is_finite()) {
                return Err(format!("{name}: position must be finite"));
            }
            if !object.rotation.iter().all(|value| value.is_finite())
                || !object.spin.iter().all(|value| value.is_finite())
            {
                return Err(format!("{name}: rotation and spin must be finite"));
            }
            if object
                .scale
                .iter()
                .any(|value| !value.is_finite() || *value == 0.0)
            {
                return Err(format!("{name}: scale must be finite and non-zero"));
            }
            if !(0.0..=1.0).contains(&object.metalness) {
                return Err(format!("{name}: metalness must be 0..=1"));
            }
            if !(0.0..=1.0).contains(&object.roughness) {
                return Err(format!("{name}: roughness must be 0..=1"));
            }
            if !(0.0..=1.0).contains(&object.emissive) {
                return Err(format!("{name}: emissive must be 0..=1"));
            }
            if ![object.color.r, object.color.g, object.color.b]
                .iter()
                .all(|value| value.is_finite())
                || !(0.0..=1.0).contains(&object.color.a)
            {
                return Err(format!(
                    "{name}: colour channels must be finite and alpha 0..=1"
                ));
            }
            object
                .geometry
                .validate()
                .map_err(|why| format!("{name}: {why}"))?;
        }
        for (index, light) in self.lights.iter().enumerate() {
            let (name, intensity) = match light {
                Light::Ambient { intensity, .. } => ("ambient light", *intensity),
                Light::Directional { intensity, .. } => ("directional light", *intensity),
                Light::Point { intensity, .. } => ("point light", *intensity),
            };
            if !intensity.is_finite() || intensity < 0.0 {
                return Err(format!(
                    "{name} {index}: intensity must be a non-negative number"
                ));
            }
            match light {
                Light::Directional { direction, .. } => {
                    if !direction.iter().all(|value| value.is_finite())
                        || DVec3::from_array(*direction).length_squared() < 1e-12
                    {
                        return Err(format!(
                            "directional light {index}: direction must be a non-zero vector"
                        ));
                    }
                }
                Light::Point {
                    position, range, ..
                } => {
                    if !position.iter().all(|value| value.is_finite())
                        || !range.is_finite()
                        || *range <= 0.0
                    {
                        return Err(format!(
                            "point light {index}: position must be finite and range positive"
                        ));
                    }
                }
                Light::Ambient { .. } => {}
            }
        }
        Ok(())
    }

    /// The showcase scene: a box, a sphere and a torus over a floor, with a
    /// slow camera move. Sizes are in comp pixels.
    pub fn showcase(width: u32, height: u32) -> Scene {
        let w = f64::from(width.max(1));
        let h = f64::from(height.max(1));
        let resting = (h / 2.0) / (45.0_f64 / 2.0).to_radians().tan();
        Scene {
            camera: Camera3D {
                path: vec![
                    CameraStop {
                        at: 0.0,
                        x: w * 0.5,
                        y: h * 0.52,
                        z: resting * 0.72,
                        curve: Some(Curve::EASE_IN_OUT),
                    },
                    CameraStop {
                        at: 0.5,
                        x: w * 0.58,
                        y: h * 0.47,
                        z: resting * 0.62,
                        curve: Some(Curve::EASE_IN_OUT),
                    },
                    CameraStop {
                        at: 1.0,
                        x: w * 0.42,
                        y: h * 0.55,
                        z: resting * 0.72,
                        curve: None,
                    },
                ],
                fov: vec![AxisStop {
                    at: 0.0,
                    value: 45.0,
                    curve: None,
                }],
                ..Camera3D::default()
            },
            lights: vec![
                Light::Ambient {
                    color: Rgba::new(0.55, 0.62, 0.75, 1.0),
                    intensity: 0.35,
                },
                Light::Directional {
                    direction: [-0.45, -0.8, -0.6],
                    color: Rgba::new(1.0, 0.96, 0.9, 1.0),
                    intensity: 1.15,
                },
                Light::Point {
                    position: [w * 0.18, h * 0.18, 620.0],
                    color: Rgba::new(0.3, 0.8, 1.0, 1.0),
                    intensity: 0.85,
                    range: 1800.0,
                },
            ],
            objects: vec![
                Object {
                    id: "floor".into(),
                    geometry: Geometry::Plane {
                        width: w * 0.92,
                        height: h * 0.56,
                    },
                    position: [w * 0.5, h * 0.73, -140.0],
                    rotation: [90.0, 0.0, 0.0],
                    color: Rgba::new(0.055, 0.07, 0.095, 1.0),
                    roughness: 0.92,
                    ..Object::default()
                },
                Object {
                    id: "box".into(),
                    geometry: Geometry::Box {
                        size: [220.0, 220.0, 220.0],
                    },
                    position: [w * 0.38, h * 0.55, 0.0],
                    rotation: [18.0, -28.0, 12.0],
                    spin: [0.0, 22.0, 0.0],
                    color: Rgba::new(0.95, 0.32, 0.22, 1.0),
                    metalness: 0.25,
                    roughness: 0.35,
                    ..Object::default()
                },
                Object {
                    id: "sphere".into(),
                    geometry: Geometry::Sphere {
                        radius: 120.0,
                        segments: 32,
                        rings: 18,
                    },
                    position: [w * 0.62, h * 0.5, 80.0],
                    spin: [0.0, -18.0, 0.0],
                    color: Rgba::new(0.25, 0.72, 0.95, 1.0),
                    metalness: 0.1,
                    roughness: 0.18,
                    ..Object::default()
                },
                Object {
                    id: "torus".into(),
                    geometry: Geometry::Torus {
                        radius: 150.0,
                        tube: 38.0,
                        segments: 48,
                        sides: 18,
                    },
                    position: [w * 0.5, h * 0.33, -60.0],
                    rotation: [68.0, 0.0, 0.0],
                    spin: [0.0, 0.0, 35.0],
                    color: Rgba::new(0.98, 0.78, 0.25, 1.0),
                    metalness: 0.65,
                    roughness: 0.2,
                    ..Object::default()
                },
            ],
            background: Some(Rgba::new(0.025, 0.035, 0.05, 1.0)),
            samples: 1,
        }
    }

    /// A product shot: one hero cylinder on a plane, slow push in.
    pub fn product(width: u32, height: u32) -> Scene {
        let w = f64::from(width.max(1));
        let h = f64::from(height.max(1));
        let resting = (h / 2.0) / (45.0_f64 / 2.0).to_radians().tan();
        Scene {
            camera: Camera3D {
                path: vec![
                    CameraStop {
                        at: 0.0,
                        x: w * 0.5,
                        y: h * 0.5,
                        z: resting * 0.8,
                        curve: Some(Curve::EASE_OUT),
                    },
                    CameraStop {
                        at: 1.0,
                        x: w * 0.5,
                        y: h * 0.48,
                        z: resting * 0.62,
                        curve: None,
                    },
                ],
                fov: vec![AxisStop {
                    at: 0.0,
                    value: 42.0,
                    curve: None,
                }],
                ..Camera3D::default()
            },
            lights: vec![
                Light::Ambient {
                    color: Rgba::new(0.5, 0.58, 0.72, 1.0),
                    intensity: 0.4,
                },
                Light::Directional {
                    direction: [-0.3, -0.9, -0.5],
                    color: Rgba::new(1.0, 0.97, 0.92, 1.0),
                    intensity: 1.25,
                },
                Light::Point {
                    position: [w * 0.72, h * 0.25, 500.0],
                    color: Rgba::new(0.35, 0.75, 1.0, 1.0),
                    intensity: 0.7,
                    range: 1500.0,
                },
            ],
            objects: vec![
                Object {
                    id: "floor".into(),
                    geometry: Geometry::Plane {
                        width: w * 1.1,
                        height: h * 0.7,
                    },
                    position: [w * 0.5, h * 0.78, -80.0],
                    rotation: [90.0, 0.0, 0.0],
                    color: Rgba::new(0.04, 0.05, 0.07, 1.0),
                    roughness: 0.9,
                    ..Object::default()
                },
                Object {
                    id: "hero".into(),
                    geometry: Geometry::Cylinder {
                        radius: 150.0,
                        height: 300.0,
                        segments: 48,
                    },
                    position: [w * 0.5, h * 0.5, 0.0],
                    rotation: [90.0, 0.0, 0.0],
                    spin: [0.0, 0.0, 18.0],
                    color: Rgba::new(0.2, 0.75, 0.95, 1.0),
                    metalness: 0.55,
                    roughness: 0.2,
                    ..Object::default()
                },
            ],
            background: Some(Rgba::new(0.02, 0.03, 0.045, 1.0)),
            samples: 1,
        }
    }

    /// A named scene an op can ask for, built for a comp of this size.
    pub fn preset_at(name: &str, width: u32, height: u32) -> Option<Scene> {
        match name {
            "showcase" => Some(Scene::showcase(width, height)),
            "product" => Some(Scene::product(width, height)),
            _ => None,
        }
    }

    /// A named scene at the library's reference size.
    pub fn preset(name: &str) -> Option<Scene> {
        Self::preset_at(name, 1920, 1080)
    }

    /// The names [`preset`](Self::preset) answers to.
    pub const PRESET_NAMES: &'static [&'static str] = &["showcase", "product"];

    /// Draws the scene at `at` seconds into a `width` by `height` pixmap.
    ///
    /// `scale` is output pixels per comp pixel, the same scale the native
    /// scene uses; the camera is evaluated in comp pixels and the result is
    /// multiplied up, so a half-size bake is the same picture.
    pub fn render(&self, width: u32, height: u32, at: f64, duration: f64, scale: f64) -> Pixmap {
        let width = width.max(1);
        let height = height.max(1);
        let samples = self.samples.clamp(1, 2);
        let out_w = width * samples;
        let out_h = height * samples;
        let comp_w = f64::from(width) / scale.max(1e-6);
        let comp_h = f64::from(height) / scale.max(1e-6);
        let camera = if self.camera.is_resting() {
            default_camera(comp_w, comp_h)
        } else {
            self.camera.clone()
        };
        let vp = camera.view_projection(at, duration, comp_w, comp_h);
        let eye = camera.eye(at, duration, comp_w, comp_h);
        let pixel_scale = scale * f64::from(samples);

        let mut pixels = vec![[0.0_f32; 4]; (out_w as usize) * (out_h as usize)];
        if let Some(background) = self.background {
            let color = [
                srgb_to_linear(background.r as f32),
                srgb_to_linear(background.g as f32),
                srgb_to_linear(background.b as f32),
            ];
            for pixel in &mut pixels {
                *pixel = [color[0], color[1], color[2], background.a as f32];
            }
        }
        let mut depth = vec![f64::INFINITY; pixels.len()];

        let lights = if self.lights.is_empty() {
            default_lights()
        } else {
            self.lights.clone()
        };
        let uniforms = lights.iter().map(LightUniform::new).collect::<Vec<_>>();

        for object in &self.objects {
            let rotation = [
                object.rotation[0] + object.spin[0] * at,
                object.rotation[1] + object.spin[1] * at,
                object.rotation[2] + object.spin[2] * at,
            ];
            let model = DMat4::from_scale_rotation_translation(
                DVec3::from_array(object.scale),
                DQuat::from_euler(
                    EulerRot::XYZ,
                    rotation[0].to_radians(),
                    rotation[1].to_radians(),
                    rotation[2].to_radians(),
                ),
                DVec3::from_array(object.position),
            );
            let normal_matrix = DMat3::from_mat4(model).inverse().transpose();
            let mesh = object.geometry.mesh();
            let clipped = mesh
                .vertices
                .iter()
                .map(|vertex| {
                    let world = model.transform_point3(vertex.position);
                    Clipped {
                        clip: vp * world.extend(1.0),
                        normal: normal_matrix.mul_vec3(vertex.normal).normalize_or_zero(),
                        world,
                    }
                })
                .collect::<Vec<_>>();
            let base = [
                srgb_to_linear(object.color.r as f32),
                srgb_to_linear(object.color.g as f32),
                srgb_to_linear(object.color.b as f32),
            ];
            let metal = object.metalness.clamp(0.0, 1.0) as f32;
            let diffuse = [
                base[0] * (1.0 - metal),
                base[1] * (1.0 - metal),
                base[2] * (1.0 - metal),
            ];
            let specular = [
                0.04 + (base[0] - 0.04) * metal,
                0.04 + (base[1] - 0.04) * metal,
                0.04 + (base[2] - 0.04) * metal,
            ];
            let shininess =
                ((2.0 / object.roughness.clamp(0.02, 1.0).powi(4) - 2.0).clamp(2.0, 512.0)) as f32;
            let alpha = object.color.a as f32;
            if alpha <= 0.0 {
                continue;
            }
            for triangle in mesh.indices.chunks_exact(3) {
                let polygon = clip_near([
                    clipped[triangle[0] as usize],
                    clipped[triangle[1] as usize],
                    clipped[triangle[2] as usize],
                ]);
                let mut screen = [Projected::ZERO; 4];
                for (index, vertex) in polygon.vertices[..polygon.len].iter().enumerate() {
                    screen[index] = project(vertex, pixel_scale);
                }
                for index in 1..polygon.len.saturating_sub(1) {
                    raster_triangle(
                        &mut pixels,
                        &mut depth,
                        out_w,
                        out_h,
                        &screen[0],
                        &screen[index],
                        &screen[index + 1],
                        eye,
                        &uniforms,
                        &diffuse,
                        &specular,
                        shininess,
                        alpha,
                        object.emissive as f32,
                    );
                }
            }
        }

        if samples > 1 {
            pixels = downsample(&pixels, out_w, out_h, width, height);
        }
        pixmap_of(&pixels, width, height)
    }
}

fn check_segments(value: u32, min: u32, max: u32, name: &str) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("{name} must be {min}..={max}"))
    }
}

fn default_camera(width: f64, height: f64) -> Camera3D {
    let fov: f64 = 45.0;
    let resting = (height / 2.0) / (fov / 2.0).to_radians().tan();
    Camera3D {
        path: vec![CameraStop {
            at: 0.0,
            x: width / 2.0,
            y: height / 2.0,
            z: resting * 0.72,
            curve: None,
        }],
        fov: vec![AxisStop {
            at: 0.0,
            value: fov,
            curve: None,
        }],
        ..Camera3D::default()
    }
}

fn default_lights() -> Vec<Light> {
    vec![
        Light::Ambient {
            color: Rgba::new(0.55, 0.62, 0.75, 1.0),
            intensity: 0.35,
        },
        Light::Directional {
            direction: [-0.45, -0.8, -0.6],
            color: Rgba::new(1.0, 0.96, 0.9, 1.0),
            intensity: 1.15,
        },
    ]
}

struct LightUniform {
    ambient: bool,
    point: bool,
    direction: DVec3,
    position: DVec3,
    color: [f32; 3],
    intensity: f32,
    range: f32,
}

impl LightUniform {
    fn new(light: &Light) -> LightUniform {
        match light {
            Light::Ambient { color, intensity } => LightUniform {
                ambient: true,
                point: false,
                direction: DVec3::ZERO,
                position: DVec3::ZERO,
                color: linear_color(*color),
                intensity: *intensity as f32,
                range: 1.0,
            },
            Light::Directional {
                direction,
                color,
                intensity,
            } => LightUniform {
                ambient: false,
                point: false,
                direction: -DVec3::from_array(*direction).normalize_or_zero(),
                position: DVec3::ZERO,
                color: linear_color(*color),
                intensity: *intensity as f32,
                range: 1.0,
            },
            Light::Point {
                position,
                color,
                intensity,
                range,
            } => LightUniform {
                ambient: false,
                point: true,
                direction: DVec3::ZERO,
                position: DVec3::from_array(*position),
                color: linear_color(*color),
                intensity: *intensity as f32,
                range: (*range).max(1.0) as f32,
            },
        }
    }
}

fn linear_color(color: Rgba) -> [f32; 3] {
    [
        srgb_to_linear(color.r as f32),
        srgb_to_linear(color.g as f32),
        srgb_to_linear(color.b as f32),
    ]
}

/// A vertex before the perspective divide, in the camera's homogeneous
/// clip space. `clip.w` is view depth, so the near plane is `w >= NEAR`.
#[derive(Clone, Copy)]
struct Clipped {
    clip: DVec4,
    normal: DVec3,
    world: DVec3,
}

struct ClipPolygon {
    vertices: [Clipped; 4],
    len: usize,
}

#[derive(Clone, Copy)]
struct Projected {
    x: f64,
    y: f64,
    inv_w: f64,
    normal: DVec3,
    world: DVec3,
}

impl Projected {
    const ZERO: Projected = Projected {
        x: 0.0,
        y: 0.0,
        inv_w: 0.0,
        normal: DVec3::ZERO,
        world: DVec3::ZERO,
    };
}

/// Clip a triangle against the near plane. A triangle crossing the plane
/// leaves a quad, so the result is at most four vertices.
fn clip_near(triangle: [Clipped; 3]) -> ClipPolygon {
    const NEAR: f64 = 1.0;
    let mut out = ClipPolygon {
        vertices: [triangle[0]; 4],
        len: 0,
    };
    for index in 0..3 {
        let current = triangle[index];
        let next = triangle[(index + 1) % 3];
        let current_in = current.clip.w >= NEAR;
        let next_in = next.clip.w >= NEAR;
        if current_in {
            out.vertices[out.len] = current;
            out.len += 1;
        }
        if current_in != next_in {
            let t = (NEAR - current.clip.w) / (next.clip.w - current.clip.w);
            out.vertices[out.len] = Clipped {
                clip: current.clip + (next.clip - current.clip) * t,
                normal: current.normal + (next.normal - current.normal) * t,
                world: current.world + (next.world - current.world) * t,
            };
            out.len += 1;
        }
    }
    out
}

fn project(vertex: &Clipped, pixel_scale: f64) -> Projected {
    let inv_w = 1.0 / vertex.clip.w.max(1e-9);
    Projected {
        x: vertex.clip.x * inv_w * pixel_scale,
        y: vertex.clip.y * inv_w * pixel_scale,
        inv_w,
        normal: vertex.normal,
        world: vertex.world,
    }
}

#[allow(clippy::too_many_arguments)]
fn raster_triangle(
    pixels: &mut [[f32; 4]],
    depth: &mut [f64],
    width: u32,
    height: u32,
    a: &Projected,
    b: &Projected,
    c: &Projected,
    eye: DVec3,
    lights: &[LightUniform],
    diffuse: &[f32; 3],
    specular: &[f32; 3],
    shininess: f32,
    alpha: f32,
    emissive: f32,
) {
    let area = edge([a.x, a.y], [b.x, b.y], [c.x, c.y]);
    if area.abs() < 1e-9 {
        return;
    }
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as i64;
    let max_x = a.x.max(b.x).max(c.x).ceil().min(f64::from(width)) as i64;
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as i64;
    let max_y = a.y.max(b.y).max(c.y).ceil().min(f64::from(height)) as i64;
    if min_x >= max_x || min_y >= max_y {
        return;
    }
    let stride = width as usize;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let w0 = edge([b.x, b.y], [c.x, c.y], [px, py]) / area;
            let w1 = edge([c.x, c.y], [a.x, a.y], [px, py]) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < -1e-6 || w1 < -1e-6 || w2 < -1e-6 {
                continue;
            }
            let inv_w = w0 * a.inv_w + w1 * b.inv_w + w2 * c.inv_w;
            if inv_w <= 0.0 {
                continue;
            }
            let z = 1.0 / inv_w;
            let index = y as usize * stride + x as usize;
            if z >= depth[index] {
                continue;
            }
            let normal =
                (a.normal * (w0 * a.inv_w) + b.normal * (w1 * b.inv_w) + c.normal * (w2 * c.inv_w))
                    / inv_w;
            let world =
                (a.world * (w0 * a.inv_w) + b.world * (w1 * b.inv_w) + c.world * (w2 * c.inv_w))
                    / inv_w;
            let normal = normal.normalize_or_zero();
            let color = shade(
                &normal, world, eye, lights, diffuse, specular, shininess, emissive,
            );
            blend(&mut pixels[index], color, alpha);
            if alpha >= 0.999 {
                depth[index] = z;
            }
        }
    }
}

fn shade(
    normal: &DVec3,
    world: DVec3,
    eye: DVec3,
    lights: &[LightUniform],
    diffuse: &[f32; 3],
    specular: &[f32; 3],
    shininess: f32,
    emissive: f32,
) -> [f32; 3] {
    let mut rgb = [
        diffuse[0] * emissive,
        diffuse[1] * emissive,
        diffuse[2] * emissive,
    ];
    let view = (eye - world).normalize_or_zero();
    let view = if view.length_squared() < 1e-12 {
        DVec3::Z
    } else {
        view
    };
    for light in lights {
        if light.ambient {
            for channel in 0..3 {
                rgb[channel] += diffuse[channel] * light.color[channel] * light.intensity;
            }
            continue;
        }
        let (direction, attenuation) = if light.point {
            let to_light = light.position - world;
            let distance = to_light.length().max(1e-6);
            (
                to_light / distance,
                1.0 / (1.0 + (distance / f64::from(light.range)).powi(2)),
            )
        } else {
            (light.direction, 1.0)
        };
        let ndl = normal.dot(direction).max(0.0);
        if ndl <= 0.0 {
            continue;
        }
        let half = (direction + view).normalize_or_zero();
        let spec = normal.dot(half).max(0.0).powf(shininess as f64);
        let strength = (light.intensity as f64) * attenuation * ndl;
        for channel in 0..3 {
            rgb[channel] += diffuse[channel] * light.color[channel] * strength as f32;
            rgb[channel] += specular[channel]
                * light.color[channel]
                * (spec * light.intensity as f64 * attenuation) as f32;
        }
    }
    rgb
}

fn blend(pixel: &mut [f32; 4], source: [f32; 3], alpha: f32) {
    let dst_a = pixel[3];
    let out_a = alpha + dst_a * (1.0 - alpha);
    if out_a <= 1e-6 {
        *pixel = [0.0, 0.0, 0.0, 0.0];
        return;
    }
    for channel in 0..3 {
        pixel[channel] = (source[channel] * alpha + pixel[channel] * dst_a * (1.0 - alpha)) / out_a;
    }
    pixel[3] = out_a;
}

fn edge(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
}

fn downsample(
    pixels: &[[f32; 4]],
    width: u32,
    height: u32,
    out_width: u32,
    out_height: u32,
) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0_f32; 4]; (out_width as usize) * (out_height as usize)];
    let scale_x = f64::from(width) / f64::from(out_width.max(1));
    let scale_y = f64::from(height) / f64::from(out_height.max(1));
    for y in 0..out_height {
        for x in 0..out_width {
            let x0 = (f64::from(x) * scale_x).floor() as u32;
            let x1 = ((f64::from(x) + 1.0) * scale_x).ceil() as u32;
            let y0 = (f64::from(y) * scale_y).floor() as u32;
            let y1 = ((f64::from(y) + 1.0) * scale_y).ceil() as u32;
            let mut sum = [0.0_f32; 4];
            let mut count = 0.0_f32;
            for sy in y0..y1.min(height) {
                for sx in x0..x1.min(width) {
                    let pixel = pixels[sy as usize * width as usize + sx as usize];
                    // Premultiplied, so a transparent edge does not darken
                    // the colour it is averaged with.
                    for channel in 0..3 {
                        sum[channel] += pixel[channel] * pixel[3];
                    }
                    sum[3] += pixel[3];
                    count += 1.0;
                }
            }
            if count > 0.0 {
                let alpha = sum[3] / count;
                let slot = &mut out[y as usize * out_width as usize + x as usize];
                if sum[3] > 1e-6 {
                    for channel in 0..3 {
                        slot[channel] = sum[channel] / sum[3];
                    }
                }
                slot[3] = alpha;
            }
        }
    }
    out
}

fn pixmap_of(pixels: &[[f32; 4]], width: u32, height: u32) -> Pixmap {
    let mut bytes = vec![0_u8; (width as usize) * (height as usize) * 4];
    let lut = linear_to_srgb_lut();
    for (index, pixel) in pixels.iter().enumerate() {
        let alpha = pixel[3].clamp(0.0, 1.0);
        for channel in 0..3 {
            let linear = pixel[channel].clamp(0.0, 1.0);
            let encoded = lut[(linear * 4095.0).round() as usize] as f32 / 255.0;
            bytes[index * 4 + channel] = (encoded * alpha * 255.0).round() as u8;
        }
        bytes[index * 4 + 3] = (alpha * 255.0).round() as u8;
    }
    Pixmap::from_vec(
        bytes,
        tiny_skia::IntSize::from_wh(width, height)
            .unwrap_or_else(|| tiny_skia::IntSize::from_wh(1, 1).expect("1x1")),
    )
    .unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1"))
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_lut() -> &'static [u8; 4096] {
    static LUT: OnceLock<[u8; 4096]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut lut = [0_u8; 4096];
        for (index, slot) in lut.iter_mut().enumerate() {
            let linear = index as f32 / 4095.0;
            let encoded = if linear <= 0.003_130_8 {
                linear * 12.92
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            *slot = (encoded.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        lut
    })
}

fn one() -> f64 {
    1.0
}

fn one_u32() -> u32 {
    1
}

fn one3() -> [f64; 3] {
    [1.0, 1.0, 1.0]
}

fn white() -> Rgba {
    Rgba::new(1.0, 1.0, 1.0, 1.0)
}

fn default_roughness() -> f64 {
    0.5
}

fn default_range() -> f64 {
    1000.0
}

fn default_segments() -> u32 {
    24
}

fn default_rings() -> u32 {
    12
}

fn default_sides() -> u32 {
    12
}

struct Vertex {
    position: DVec3,
    normal: DVec3,
}

struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl Mesh {
    fn new() -> Mesh {
        Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn vertex(&mut self, position: DVec3, normal: DVec3) -> u32 {
        let index = self.vertices.len() as u32;
        self.vertices.push(Vertex { position, normal });
        index
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }
}

impl Geometry {
    fn mesh(&self) -> Mesh {
        match self {
            Geometry::Box { size } => box_mesh(*size),
            Geometry::Plane { width, height } => plane_mesh(*width, *height),
            Geometry::Sphere {
                radius,
                segments,
                rings,
            } => sphere_mesh(*radius, *segments, *rings),
            Geometry::Cylinder {
                radius,
                height,
                segments,
            } => cylinder_mesh(*radius, *height, *segments),
            Geometry::Torus {
                radius,
                tube,
                segments,
                sides,
            } => torus_mesh(*radius, *tube, *segments, *sides),
        }
    }
}

fn box_mesh(size: [f64; 3]) -> Mesh {
    let [w, h, d] = size.map(|value| value / 2.0);
    let mut mesh = Mesh::new();
    let faces = [
        (
            [
                DVec3::new(-w, -h, d),
                DVec3::new(w, -h, d),
                DVec3::new(w, h, d),
                DVec3::new(-w, h, d),
            ],
            DVec3::Z,
        ),
        (
            [
                DVec3::new(w, -h, -d),
                DVec3::new(-w, -h, -d),
                DVec3::new(-w, h, -d),
                DVec3::new(w, h, -d),
            ],
            -DVec3::Z,
        ),
        (
            [
                DVec3::new(w, -h, d),
                DVec3::new(w, -h, -d),
                DVec3::new(w, h, -d),
                DVec3::new(w, h, d),
            ],
            DVec3::X,
        ),
        (
            [
                DVec3::new(-w, -h, -d),
                DVec3::new(-w, -h, d),
                DVec3::new(-w, h, d),
                DVec3::new(-w, h, -d),
            ],
            -DVec3::X,
        ),
        (
            [
                DVec3::new(-w, h, d),
                DVec3::new(w, h, d),
                DVec3::new(w, h, -d),
                DVec3::new(-w, h, -d),
            ],
            DVec3::Y,
        ),
        (
            [
                DVec3::new(-w, -h, -d),
                DVec3::new(w, -h, -d),
                DVec3::new(w, -h, d),
                DVec3::new(-w, -h, d),
            ],
            -DVec3::Y,
        ),
    ];
    for (positions, normal) in faces {
        let a = mesh.vertex(positions[0], normal);
        let b = mesh.vertex(positions[1], normal);
        let c = mesh.vertex(positions[2], normal);
        let d = mesh.vertex(positions[3], normal);
        mesh.triangle(a, b, c);
        mesh.triangle(a, c, d);
    }
    mesh
}

fn plane_mesh(width: f64, height: f64) -> Mesh {
    let mut mesh = Mesh::new();
    let (w, h) = (width / 2.0, height / 2.0);
    let normal = DVec3::Z;
    let a = mesh.vertex(DVec3::new(-w, -h, 0.0), normal);
    let b = mesh.vertex(DVec3::new(w, -h, 0.0), normal);
    let c = mesh.vertex(DVec3::new(w, h, 0.0), normal);
    let d = mesh.vertex(DVec3::new(-w, h, 0.0), normal);
    mesh.triangle(a, b, c);
    mesh.triangle(a, c, d);
    mesh
}

fn sphere_mesh(radius: f64, segments: u32, rings: u32) -> Mesh {
    let segments = segments.clamp(3, 128);
    let rings = rings.clamp(2, 128);
    let mut mesh = Mesh::new();
    for ring in 0..=rings {
        let v = ring as f64 / rings as f64;
        let lat = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * v;
        let (sin_lat, cos_lat) = lat.sin_cos();
        for segment in 0..=segments {
            let u = segment as f64 / segments as f64;
            let lon = std::f64::consts::TAU * u;
            let (sin_lon, cos_lon) = lon.sin_cos();
            let normal = DVec3::new(cos_lat * cos_lon, sin_lat, cos_lat * sin_lon);
            mesh.vertex(normal * radius, normal);
        }
    }
    let row = segments + 1;
    for ring in 0..rings {
        for segment in 0..segments {
            let a = ring * row + segment;
            let b = a + row;
            mesh.triangle(a, b, a + 1);
            mesh.triangle(a + 1, b, b + 1);
        }
    }
    mesh
}

fn cylinder_mesh(radius: f64, height: f64, segments: u32) -> Mesh {
    let segments = segments.clamp(3, 128);
    let half = height / 2.0;
    let mut mesh = Mesh::new();
    for segment in 0..=segments {
        let angle = std::f64::consts::TAU * segment as f64 / segments as f64;
        let (sin, cos) = angle.sin_cos();
        let normal = DVec3::new(cos, sin, 0.0);
        mesh.vertex(DVec3::new(cos * radius, sin * radius, half), normal);
        mesh.vertex(DVec3::new(cos * radius, sin * radius, -half), normal);
    }
    let row = 2;
    for segment in 0..segments {
        let a = segment * row;
        mesh.triangle(a, a + 2, a + 1);
        mesh.triangle(a + 1, a + 2, a + 3);
    }
    let top = mesh.vertex(DVec3::new(0.0, 0.0, half), DVec3::Z);
    let bottom = mesh.vertex(DVec3::new(0.0, 0.0, -half), -DVec3::Z);
    let base = mesh.vertices.len() as u32;
    for segment in 0..segments {
        let angle = std::f64::consts::TAU * segment as f64 / segments as f64;
        let (sin, cos) = angle.sin_cos();
        mesh.vertex(DVec3::new(cos * radius, sin * radius, half), DVec3::Z);
        mesh.vertex(DVec3::new(cos * radius, sin * radius, -half), -DVec3::Z);
    }
    for segment in 0..segments {
        let a = base + segment * 2;
        mesh.triangle(top, a, a + 2);
        mesh.triangle(bottom, a + 3, a + 1);
    }
    mesh
}

fn torus_mesh(radius: f64, tube: f64, segments: u32, sides: u32) -> Mesh {
    let segments = segments.clamp(3, 128);
    let sides = sides.clamp(3, 64);
    let mut mesh = Mesh::new();
    for segment in 0..=segments {
        let u = std::f64::consts::TAU * segment as f64 / segments as f64;
        let (sin_u, cos_u) = u.sin_cos();
        for side in 0..=sides {
            let v = std::f64::consts::TAU * side as f64 / sides as f64;
            let (sin_v, cos_v) = v.sin_cos();
            let normal = DVec3::new(cos_v * cos_u, cos_v * sin_u, sin_v);
            let position = DVec3::new(
                (radius + tube * cos_v) * cos_u,
                (radius + tube * cos_v) * sin_u,
                tube * sin_v,
            );
            mesh.vertex(position, normal);
        }
    }
    let row = sides + 1;
    for segment in 0..segments {
        for side in 0..sides {
            let a = segment * row + side;
            let b = a + row;
            mesh.triangle(a, b, a + 1);
            mesh.triangle(a + 1, b, b + 1);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_validates_and_renders_its_objects() {
        let scene = Scene::showcase(640, 360);
        scene.validate().expect("the showcase is valid");
        let frame = scene.render(640, 360, 1.0, 8.0, 1.0);
        assert_eq!(frame.width(), 640);
        assert_eq!(frame.height(), 360);
        let painted = frame
            .data()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .count();
        assert!(painted > 1000, "the scene draws something");
    }

    #[test]
    fn an_empty_scene_is_refused() {
        let scene = Scene::default();
        assert!(scene.validate().is_err());
    }

    #[test]
    fn a_bad_geometry_is_refused() {
        let mut scene = Scene::product(320, 180);
        scene.objects[1].geometry = Geometry::Sphere {
            radius: 0.0,
            segments: 8,
            rings: 8,
        };
        assert!(scene.validate().is_err());
    }

    #[test]
    fn the_camera_move_changes_the_frame() {
        let scene = Scene::showcase(320, 180);
        let first = scene.render(320, 180, 0.0, 8.0, 1.0);
        let later = scene.render(320, 180, 4.0, 8.0, 1.0);
        assert_ne!(first.data(), later.data());
    }

    #[test]
    fn the_near_plane_clips_triangles_behind_the_eye() {
        let vertex = |w| Clipped {
            clip: DVec4::new(0.0, 0.0, 0.0, w),
            normal: DVec3::Z,
            world: DVec3::ZERO,
        };
        assert_eq!(clip_near([vertex(2.0), vertex(3.0), vertex(4.0)]).len, 3);
        let crossing = clip_near([vertex(-1.0), vertex(2.0), vertex(3.0)]);
        assert!(crossing.len >= 3);
        assert!(
            crossing.vertices[..crossing.len]
                .iter()
                .all(|vertex| vertex.clip.w >= 1.0 - 1e-9),
            "every clipped vertex sits in front of the near plane"
        );
        assert_eq!(clip_near([vertex(-1.0), vertex(-2.0), vertex(-3.0)]).len, 0);
    }

    #[test]
    fn a_camera_inside_a_box_clips_instead_of_folding() {
        let scene = Scene {
            camera: Camera3D {
                path: vec![CameraStop {
                    at: 0.0,
                    x: 160.0,
                    y: 90.0,
                    z: 0.0,
                    curve: None,
                }],
                ..Camera3D::default()
            },
            objects: vec![Object {
                geometry: Geometry::Box {
                    size: [200.0, 200.0, 200.0],
                },
                position: [160.0, 90.0, 0.0],
                color: Rgba::new(1.0, 0.2, 0.1, 1.0),
                ..Object::default()
            }],
            background: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
            ..Scene::default()
        };
        let frame = scene.render(320, 180, 0.0, 1.0, 1.0);
        assert!(
            frame
                .data()
                .chunks_exact(4)
                .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0),
            "the box is drawn around the eye"
        );
    }

    #[test]
    fn a_scene_reads_from_the_documented_json() {
        let scene: Scene = serde_json::from_str(
            r##"{
                "background": "#070a0f",
                "objects": [{
                    "id": "box",
                    "geometry": { "kind": "box", "size": [180, 180, 180] },
                    "position": [0, 0, 0],
                    "rotation": [18, 32, 0],
                    "color": "#ffb84d",
                    "metalness": 0.35,
                    "roughness": 0.28
                }],
                "lights": [
                    { "kind": "ambient", "color": "#4b5b78", "intensity": 0.6 },
                    { "kind": "directional", "direction": [-0.4, 0.8, -0.5], "intensity": 1.4 },
                    { "kind": "point", "position": [300, 260, 420], "color": "#ffd9a0", "intensity": 1.1 }
                ],
                "camera": { "path": [
                    { "at": 0, "x": 0, "y": -120, "z": 900 },
                    { "at": 1, "x": 180, "y": -60, "z": 700 }
                ] }
            }"##,
        )
        .expect("the documented scene reads");
        scene.validate().expect("the documented scene validates");
    }

    #[test]
    fn a_point_light_at_the_camera_lights_a_sphere() {
        let scene = Scene {
            camera: default_camera(320.0, 180.0),
            lights: vec![Light::Point {
                position: [160.0, 90.0, 400.0],
                color: Rgba::new(1.0, 1.0, 1.0, 1.0),
                intensity: 2.0,
                range: 1000.0,
            }],
            objects: vec![Object {
                geometry: Geometry::Sphere {
                    radius: 50.0,
                    segments: 24,
                    rings: 12,
                },
                position: [160.0, 90.0, 0.0],
                color: Rgba::new(1.0, 0.2, 0.1, 1.0),
                roughness: 0.5,
                ..Object::default()
            }],
            background: None,
            ..Scene::default()
        };
        let frame = scene.render(320, 180, 0.0, 1.0, 1.0);
        let centre = frame.pixel(160, 90).expect("the centre pixel");
        assert!(centre.alpha() > 0);
        assert!(centre.red() > 20, "{centre:?}");
    }
}
